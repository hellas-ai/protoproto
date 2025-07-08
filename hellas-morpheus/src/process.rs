//! Simplified MorpheusProcess using the ProcessState pattern
//!
//! This is now a thin orchestrator that:
//! 1. Holds the unified ProcessState
//! 2. Calls pure logic functions
//! 3. Applies effects to mutate state
//! 4. Handles persistence via Storage

use crate::state::ProcessState;
use crate::storage::Storage;
use crate::*;
use fastbloom::BloomFilter;
use std::collections::BTreeSet;
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

    // Persistence layer
    #[derivative(Debug = "ignore")]
    pub storage: Storage<Tr>,

    // Message deduplication (kept separate as it's not protocol state)
    pub seen_messages: BloomFilter,
    pub seen_message_hashes: BTreeSet<[u8; 32]>,
}

impl<Tr: Transaction> MorpheusProcess<Tr> {
    /// Create a new process
    pub fn new(
        db: Arc<redb::Database>,
        keybook: KeyBook,
        id: Identity,
        n: u32,
        f: u32,
    ) -> Result<Self, String> {
        // Register with tracing
        crate::tracing_setup::register_process(&id, n, f);

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

        // Create storage
        let storage = Storage::new(db, genesis_block.clone(), genesis_qc.clone())?;

        Ok(Self {
            kb: keybook,
            chainid: [0; 32],
            id,
            n,
            f,
            delta: 10,
            state,
            storage,
            seen_messages: BloomFilter::with_num_bits(8 * 1024 * 16)
                .seed(&0x8F3A57D2C19E4B7F0123456789ABCDEF)
                .expected_items(100_000),
            seen_message_hashes: BTreeSet::new(),
        })
    }

    /// Process a message - main entry point
    pub fn process_message(
        &mut self,
        message: Message<Tr>,
        sender: Identity,
    ) -> Result<Vec<(Message<Tr>, Option<Identity>)>, String> {
        // Deduplication
        if !self.storage.journal.replaying && self.seen_messages.contains(&message) {
            let bytes = postcard::to_stdvec(&message).unwrap();
            let hash = blake3::hash(&bytes);
            if self.seen_message_hashes.contains(hash.as_bytes()) {
                tracing::error!(
                    target: "duplicate_message",
                    sender = ?sender,
                    full_message = format::format_message(&message, false),
                    "Ignoring duplicate message"
                );
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

    /// Handle an action using the Action/Effect pattern
    fn handle_action(
        &mut self,
        action: Action<Tr>,
    ) -> Result<Vec<(Message<Tr>, Option<Identity>)>, String> {
        // Step 1: Process the action with pure logic to get effects
        let effects = crate::logic::process_action(
            &self.state,
            &action,
            &self.kb,
            &self.id,
            self.n,
            self.f,
            self.delta,
        );

        // Step 2: Apply effects to mutate state and collect messages
        let mut messages = Vec::new();
        for effect in &effects {
            // Apply to our in-memory state
            self.state.apply(effect, &self.id, self.n, self.f);

            // Collect any outgoing messages
            messages.extend(self.process_effect_for_messages(effect)?);

            // Apply to storage if needed
            self.apply_effect_to_storage(effect)?;
        }

        // Step 3: Record in journal for replay
        self.storage
            .journal
            .record(action, effects, self.state.current_time)?;

        Ok(messages)
    }

    /// Process an effect to generate outgoing messages
    fn process_effect_for_messages(
        &self,
        effect: &Effect<Tr>,
    ) -> Result<Vec<(Message<Tr>, Option<Identity>)>, String> {
        let mut messages = Vec::new();

        match effect {
            Effect::VoteSent {
                vote_type,
                block_key,
                target,
            } => {
                let vote = Arc::new(ThreshPartial::from_data(
                    VoteData {
                        z: *vote_type,
                        for_which: block_key.clone(),
                    },
                    &self.kb,
                ));
                messages.push((Message::NewVote(vote), target.clone()));
            }
            Effect::MessageSent { message, target } => {
                messages.push((message.clone(), target.clone()));
            }
            Effect::ComplaintSent { qc, target } => {
                messages.push((Message::QC(qc.clone()), Some(target.clone())));
            }
            Effect::EndViewSent { view } => {
                let end_view = Arc::new(ThreshPartial::from_data(*view, &self.kb));
                messages.push((Message::EndView(end_view), None));
            }
            _ => {} // Other effects don't generate messages
        }

        Ok(messages)
    }

    /// Apply effects to storage
    fn apply_effect_to_storage(&mut self, effect: &Effect<Tr>) -> Result<(), String> {
        match effect {
            Effect::BlockRecorded { block } => {
                self.storage.store_block(block)?;
            }
            Effect::QcRecorded { qc, .. } => {
                self.storage.store_qc(qc)?;
            }
            Effect::ViewChanged { .. } => {
                // View changes are handled by ProcessState, storage doesn't need to track this
            }
            Effect::BlockProduced {
                block_key,
                block_type,
            } => {
                crate::tracing_setup::block_created(
                    &self.id,
                    if *block_type == BlockType::Lead {
                        "leader"
                    } else {
                        "transaction"
                    },
                    block_key,
                );
            }
            _ => {} // Other effects don't affect storage
        }

        Ok(())
    }

    /// Replay events from journal to restore state
    pub fn replay_from_journal(&mut self, start_position: u64) -> Result<(), String> {
        self.storage
            .journal
            .replay_from(start_position, |position, entry| {
                tracing::info!(
                    target: "replay",
                    position = position,
                    action = ?entry.action,
                    "Replaying journal entry"
                );

                // Apply effects to state
                for effect in &entry.effects {
                    self.state.apply(effect, &self.id, self.n, self.f);
                }

                Ok(())
            })
    }
}
