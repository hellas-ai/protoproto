//! Simplified MorpheusProcess using the ProcessState pattern
//!
//! This is now a thin orchestrator that:
//! 1. Holds the unified ProcessState
//! 2. Calls pure logic functions
//! 3. Applies effects to mutate state
//! 4. Handles persistence via Storage

use crate::logic::*;
use crate::*;
use fastbloom::BloomFilter;
use std::collections::{BTreeSet, VecDeque};
use std::sync::Arc;

#[derive(Clone, derivative::Derivative)]
#[derivative(Debug)]
pub struct MorpheusProcess<Tr: Transaction> {
    // Core identity
    pub kb: KeyBook,
    pub chainid: [u8; 32],
    pub id: Identity,
    pub n: u32,
    pub f: u32,
    pub delta: u128,

    // The single source of truth for all protocol state
    pub state: ProcessState<Tr>,

    // Message deduplication (kept separate as it's not protocol state)
    pub seen_messages: BloomFilter,
    pub seen_message_hashes: BTreeSet<[u8; 32]>,
}

impl<Tr: Transaction> MorpheusProcess<Tr> {
    /// Create a new process
    pub fn new(
        keybook: KeyBook,
        id: Identity,
        n: u32,
        f: u32,
    ) -> Result<Self, String> {
        // Create genesis block and QC
        let genesis_block = Arc::new(Signed {
            data: Block {
                key: GEN_BLOCK_KEY,
                prev: Vec::new(),
                one: Arc::new(ThreshSigned {
                    data: VoteData {
                        z: 1,
                        for_which: GEN_BLOCK_KEY,
                    },
                    signature: hints::Signature::default(),
                }),
                data: BlockData::Genesis,
            },
            author: Identity(u32::MAX),
            signature: hints::PartialSignature::default(),
        });

        let genesis_qc = Arc::new(ThreshSigned {
            data: VoteData {
                z: 1,
                for_which: GEN_BLOCK_KEY,
            },
            signature: hints::Signature::default(),
        });

        // Create unified state
        let state = ProcessState::new(genesis_block.clone(), genesis_qc.clone());

        Ok(Self {
            kb: keybook,
            chainid: [0; 32],
            id,
            n,
            f,
            delta: 10,
            state,
            seen_messages: BloomFilter::with_num_bits(8 * 1024 * 16)
                .seed(&0x8F3A57D2C19E4B7F0123456789ABCDEF)
                .expected_items(100_000),
            seen_message_hashes: BTreeSet::new(),
        })
    }

    /// Process a message - main entry point
    #[tracing::instrument(skip(self, message, sender), fields(process_id = ?self.id.0))]
    pub fn process_message(
        &mut self,
        message: Message<Tr>,
        sender: Identity,
    ) -> Result<Vec<(Message<Tr>, Option<Identity>)>, String> {
        // Deduplication
        if self.seen_messages.contains(&message) {
            let bytes = postcard::to_stdvec(&message).unwrap();
            let hash = blake3::hash(&bytes);
            if self.seen_message_hashes.contains(hash.as_bytes()) {
                // tracing::error!(
                //     target: "duplicate_message",
                //     sender = ?sender,
                //     full_message = format::format_message(&message, false),
                //     "Ignoring duplicate message"
                // );
                return Ok(Vec::new());
            }
        }

        // Mark as seen
        self.seen_messages.insert(&message);
        let bytes = postcard::to_stdvec(&message).unwrap();
        let hash = blake3::hash(&bytes);
        self.seen_message_hashes.insert(*hash.as_bytes());

        // Process the action
        self.handle_action(Action::ProcessMessage {
            sender,
            payload: message,
        })
    }

    /// Set ready transactions
    pub fn set_ready_transactions(
        &mut self,
        transactions: Vec<Tr>,
    ) -> Result<Vec<(Message<Tr>, Option<Identity>)>, String> {
        self.handle_action(Action::SetReadyTransactions(transactions))
    }

    /// Set current time
    pub fn set_now(&mut self, now: u128) -> Result<Vec<(Message<Tr>, Option<Identity>)>, String> {
        self.handle_action(Action::SetTime(now))
    }

    /// Check timeouts
    pub fn check_timeouts(&mut self) -> Result<Vec<(Message<Tr>, Option<Identity>)>, String> {
        self.handle_action(Action::CheckTimeouts)
    }

    /// Try to produce blocks
    pub fn try_produce_blocks(&mut self) -> Result<Vec<(Message<Tr>, Option<Identity>)>, String> {
        self.handle_action(Action::CheckProduceBlocks)
    }

    /// Handle an action using the Action/Effect pattern with proper effect queue
    #[tracing::instrument(skip(self), fields(id = self.id.0))]
    fn handle_action(
        &mut self,
        action: Action<Tr>,
    ) -> Result<Vec<(Message<Tr>, Option<Identity>)>, String> {
        // Step 1: Initialize effect queue with initial effects
        let initial_effects = crate::logic::process_action(
            &self.state,
            &action,
            &self.kb,
            &self.id,
            self.n,
            self.f,
            self.delta,
        )
        .map_err(|e| format!("Processing error: {}", e))?;

        let mut effect_queue: VecDeque<Effect<Tr>> = initial_effects.into();
        let mut messages = Vec::new();

        // Step 2: Process effects until queue is empty
        while let Some(effect) = effect_queue.pop_front() {
            // Apply effect immediately to update state
            self.state.apply(&effect, &self.id);

            // Generate any outgoing messages from this effect
            messages.extend(self.process_effect_for_messages(&effect)?);

            // Check for consequential effects after state change
            let consequential_effects = self
                .check_consequential_effects(&effect)
                .map_err(|e| format!("Processing consequential effects: {}", e))?;

            // Add any new effects to the back of the queue
            for new_effect in consequential_effects {
                effect_queue.push_back(new_effect);
            }
        }

        Ok(messages)
    }

    /// Check for effects that are triggered as a consequence of the applied effect
    fn check_consequential_effects(&self, effect: &Effect<Tr>) -> Result<Vec<Effect<Tr>>, String> {
        let mut new_effects = Vec::new();

        match effect {
            // View certificate formed - check if it triggers view change
            // QC recorded - check if it enables voting or finalizes blocks
            Effect::QcRecorded { qc, .. } => {
                // First check if max_view has advanced beyond current view (like reference implementation)
                if self.state.max_view.0 > self.state.current_view {
                    // Need to catch up to the max view we've seen
                    new_effects.extend(
                        trigger_view_change(&self.state, self.state.max_view.0, &self.id, self.n, &self.kb)
                            .map_err(|e| format!("Triggering view change from max_view: {}", e))?,
                    );
                }
                
                // Then check pending votes
                new_effects.extend(
                    check_pending_votes(&self.state, &self.kb, &self.id, self.n)
                        .map_err(|e| format!("Checking pending votes: {}", e))?,
                );
            }

            // Block recorded - check if it enables voting
            Effect::BlockRecorded { .. } => {
                new_effects.extend(
                    check_pending_votes(&self.state, &self.kb, &self.id, self.n)
                        .map_err(|e| format!("Checking pending votes: {}", e))?,
                );
            }

            // Vote recorded - check if we've reached quorum
            Effect::VoteRecorded { vote, .. } => {
                // Check if this vote completes a quorum
                let vote_count = self
                    .state
                    .vote_tracker
                    .get(&vote.data)
                    .map(|votes| votes.len())
                    .unwrap_or(0);

                if vote_count >= (self.n - self.f) as usize {
                    // We have quorum - form QC
                    if let Ok(qc) =
                        form_qc_from_state(&self.state, &vote.data, &self.kb, self.n, self.f)
                    {
                        new_effects.push(Effect::QuorumReached { qc_formed: qc });
                    }
                }
            }

            // Vote sent - also record it locally and check for quorum
            Effect::VoteSent { vote, .. } => {
                // Record our own vote locally
                new_effects.push(Effect::VoteRecorded {
                    voter: self.id.clone(),
                    vote: vote.clone(),
                });
            }

            // End-view vote recorded - check if we can form a view certificate
            Effect::EndViewRecorded { view, .. } => {
                // Check if we can form a view certificate
                if let Ok(Some(cert)) = check_view_cert_formation(&self.state, *view, &self.kb, self.n, self.f) {
                    new_effects.push(Effect::ViewCertFormed { cert: cert.clone() });
                    // Also process the certificate to trigger view change
                    new_effects.extend(
                        process_end_view_cert(&self.state, &cert, &self.kb, &self.id, self.n, self.f)
                            .map_err(|e| format!("Processing end-view cert: {}", e))?,
                    );
                }
            }

            // View changed - mark pending votes as dirty for re-evaluation
            Effect::ViewChanged { new_view, .. } => {
                // The view change effect itself handles most state updates
                // But we should check pending votes for the new view
                // First mark it dirty
                if let Some(pending) = self.state.pending_votes.get(new_view) {
                    // We can't mutate here, but the apply already set dirty = true for new views
                }
                new_effects.extend(
                    check_pending_votes(&self.state, &self.kb, &self.id, self.n)
                        .map_err(|e| format!("Checking pending votes after view change: {}", e))?,
                );
            }

            Effect::QuorumReached { qc_formed } => {
                // Check if we need to broadcast the QC
                if let Some(author) = &qc_formed.data.for_which.author {
                    // For 0-QCs, only broadcast if it's our own block
                    if qc_formed.data.z == 0 && author == &self.id {
                        new_effects.push(Effect::MessageSent {
                            message: Message::QC(qc_formed.clone()),
                            target: None,
                        });
                    }
                }

                // After the QC has been applied to state, check for view changes?

                // Check if this enables any new votes
                new_effects.extend(
                    check_pending_votes(&self.state, &self.kb, &self.id, self.n)
                        .map_err(|e| format!("Checking pending votes: {}", e))?,
                );
            }
            _ => {} // Other effects don't trigger consequential effects
        }

        Ok(new_effects)
    }

    /// Process an effect to generate outgoing messages
    fn process_effect_for_messages(
        &self,
        effect: &Effect<Tr>,
    ) -> Result<Vec<(Message<Tr>, Option<Identity>)>, String> {
        let mut messages = Vec::new();

        match effect {
            Effect::VoteSent { vote, target } => {
                messages.push((Message::NewVote(vote.clone()), target.clone()));
            }
            Effect::BlockProduced { block } => {
                messages.push((Message::Block(block.clone()), None));
            }
            Effect::MessageSent { message, target } => {
                messages.push((message.clone(), target.clone()));
            }
            _ => {} // Other effects don't generate messages
        }

        Ok(messages)
    }
}
