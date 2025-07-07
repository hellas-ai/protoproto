use crate::*;
use std::marker::PhantomData;
use std::sync::Arc;

/// Processor that handles actions and produces effects
/// This struct contains the pure logic for state transitions
#[derive(Clone, Default, Debug)]
pub struct ActionProcessor<Tr: Transaction> {
    /// Process identity
    pub id: Identity,

    /// Total number of processes
    pub n: u32,

    /// Maximum number of faulty processes
    pub f: u32,

    /// Network delay parameter
    pub delta: u128,

    /// Keybook for cryptographic operations
    pub kb: KeyBook,

    /// Phantom data to use type parameter
    _phantom: PhantomData<Tr>,
}

impl<Tr: Transaction> ActionProcessor<Tr> {
    pub fn new(id: Identity, n: u32, f: u32, delta: u128, kb: KeyBook) -> Self {
        Self {
            id,
            n,
            f,
            delta,
            kb,
            _phantom: PhantomData,
        }
    }

    /// Process an action and produce effects
    /// This is a pure function that doesn't mutate state
    pub fn process_action(
        &self,
        action: &Action<Tr>,
        state: &dyn ProcessState<Tr>,
    ) -> Vec<Effect<Tr>> {
        match action {
            Action::ProcessMessage { sender, payload } => {
                self.process_message(sender, payload, state)
            }
            Action::SetTime(time) => {
                vec![Effect::TimeUpdated(*time)]
            }
            Action::SetReadyTransactions(transactions) => {
                vec![Effect::TransactionsUpdated {
                    transactions: transactions.clone(),
                }]
            }
            Action::CheckTimeouts => self.check_timeouts(state),
            Action::CheckProduceBlocks => self.check_produce_blocks(state),
        }
    }

    /// Process an incoming message and produce effects
    fn process_message(
        &self,
        sender: &Identity,
        message: &Message<Tr>,
        state: &dyn ProcessState<Tr>,
    ) -> Vec<Effect<Tr>> {
        let mut effects = Vec::new();

        match message {
            Message::Block(block) => {
                effects.extend(self.process_block(block, state));
            }
            Message::NewVote(vote) => {
                effects.extend(self.process_vote(sender, vote, state));
            }
            Message::QC(qc) => {
                effects.extend(self.process_qc(qc, state));
            }
            Message::EndView(end_view) => {
                effects.extend(self.process_end_view(sender, end_view, state));
            }
            Message::EndViewCert(cert) => {
                effects.extend(self.process_end_view_cert(cert, state));
            }
            Message::StartView(start_view) => {
                effects.extend(self.process_start_view(sender, start_view, state));
            }
        }

        // After processing any message, check if we can vote on pending blocks
        effects.extend(self.check_pending_votes(state));

        effects
    }

    /// Process a block and produce effects
    fn process_block(
        &self,
        block: &Arc<Signed<Block<Tr>>>,
        state: &dyn ProcessState<Tr>,
    ) -> Vec<Effect<Tr>> {
        let mut effects = Vec::new();

        // Validate the block first
        if self.validate_block(block, state).is_err() {
            return effects; // Invalid block, no effects
        }

        // Record the block
        effects.push(Effect::BlockRecorded {
            block: block.clone(),
        });

        // If it's a transaction block, send 0-vote
        if block.data.key.type_ == BlockType::Tr {
            effects.push(Effect::VoteSent {
                vote_type: 0,
                block_key: block.data.key.clone(),
                target: Some(block.data.key.author.clone().unwrap()),
            });
        }

        // Record any QCs in the block
        for qc in &block.data.prev {
            effects.extend(self.process_qc(qc, state));
        }
        effects.extend(self.process_qc(&block.data.one, state));

        effects
    }

    /// Process a vote and produce effects
    fn process_vote(
        &self,
        sender: &Identity,
        vote: &Arc<ThreshPartial<VoteData>>,
        state: &dyn ProcessState<Tr>,
    ) -> Vec<Effect<Tr>> {
        let mut effects = Vec::new();

        // Validate signature
        if !vote.valid_signature(&self.kb) {
            return effects;
        }

        // Check if this is a duplicate vote from the same sender
        if state.has_vote_from(sender, &vote.data) {
            return effects;
        }

        // Record the vote
        effects.push(Effect::VoteRecorded {
            voter: sender.clone(),
            vote_data: vote.data.clone(),
        });

        // Check if we have a quorum
        let vote_count = state.count_votes(&vote.data) + 1;
        if vote_count >= (self.n - self.f) as usize {
            // Get all votes for QC formation
            let votes = state.get_votes_for(&vote.data);
            let mut all_votes = votes.clone();
            all_votes.push(vote.clone());

            // Form QC
            let qc = self.form_qc_from_votes(&vote.data, &all_votes);
            effects.push(Effect::QuorumReached {
                vote_data: vote.data.clone(),
                qc_formed: qc.clone(),
            });

            // If it's a 0-QC for our block, broadcast it
            if vote.data.z == 0 && vote.data.for_which.author == Some(self.id.clone()) {
                effects.push(Effect::MessageSent {
                    message: Message::QC(qc),
                    target: None,
                });
            }
        }

        effects
    }

    /// Process a QC and produce effects
    fn process_qc(&self, qc: &FinishedQC, state: &dyn ProcessState<Tr>) -> Vec<Effect<Tr>> {
        let mut effects = Vec::new();

        // Validate signature
        if !qc.valid_signature(&self.kb, self.n - self.f) {
            return effects;
        }

        // Check if we already have this QC
        if state.has_qc(qc) {
            return effects;
        }

        // Find blocks finalized by this QC
        let finalized_blocks = self.find_finalized_blocks(qc, state);

        // Record the QC
        effects.push(Effect::QcRecorded {
            qc: qc.clone(),
            finalized_blocks,
        });

        // Check if this triggers a view change
        if qc.data.for_which.view > state.current_view() {
            effects.push(Effect::ViewChanged {
                old_view: state.current_view(),
                new_view: qc.data.for_which.view,
                cause: format!("QC for view {}", qc.data.for_which.view.0),
            });

            // Send StartView to new leader
            effects.push(Effect::MessageSent {
                message: Message::StartView(Arc::new(Signed::from_data(
                    StartView {
                        view: qc.data.for_which.view,
                        qc: state.max_1qc().clone(),
                    },
                    &self.kb,
                ))),
                target: Some(self.leader(qc.data.for_which.view)),
            });
        }

        effects
    }

    /// Process an end-view message
    fn process_end_view(
        &self,
        _sender: &Identity,
        end_view: &Arc<ThreshPartial<ViewNum>>,
        state: &dyn ProcessState<Tr>,
    ) -> Vec<Effect<Tr>> {
        let mut effects = Vec::new();

        // Validate signature
        if !end_view.valid_signature(&self.kb) {
            return effects;
        }

        // Check if we have enough end-view messages
        let count = state.count_end_views(&end_view.data) + 1;
        if count >= (self.f + 1) as usize && end_view.data >= state.current_view() {
            // Form view certificate
            effects.push(Effect::ViewCertificateFormed {
                view: end_view.data.incr(),
            });

            let cert = self.form_view_certificate(&end_view.data, state);
            effects.push(Effect::MessageSent {
                message: Message::EndViewCert(cert),
                target: None,
            });
        }

        effects
    }

    /// Process an end-view certificate
    fn process_end_view_cert(
        &self,
        cert: &Arc<ThreshSigned<ViewNum>>,
        state: &dyn ProcessState<Tr>,
    ) -> Vec<Effect<Tr>> {
        let mut effects = Vec::new();

        // Validate signature
        if !cert.valid_signature(&self.kb, self.f + 1) {
            return effects;
        }

        let new_view = cert.data.incr();
        if new_view > state.current_view() {
            effects.push(Effect::ViewChanged {
                old_view: state.current_view(),
                new_view,
                cause: format!("End-view certificate for view {}", cert.data.0),
            });

            // Send StartView to new leader
            effects.push(Effect::MessageSent {
                message: Message::StartView(Arc::new(Signed::from_data(
                    StartView {
                        view: new_view,
                        qc: state.max_1qc().clone(),
                    },
                    &self.kb,
                ))),
                target: Some(self.leader(new_view)),
            });
        }

        effects
    }

    /// Process a start-view message
    fn process_start_view(
        &self,
        _sender: &Identity,
        start_view: &Arc<Signed<StartView>>,
        _state: &dyn ProcessState<Tr>,
    ) -> Vec<Effect<Tr>> {
        let effects = Vec::new();

        // Validate signature
        if !start_view.valid_signature(&self.kb) {
            return effects;
        }

        // Only process if we're the leader
        if self.id != self.leader(start_view.data.view) {
            return effects;
        }

        // Store the start view message (will be used for block justification)
        // This is handled by the state tracking, no specific effect needed

        effects
    }

    /// Check for timeouts and produce effects
    fn check_timeouts(&self, state: &dyn ProcessState<Tr>) -> Vec<Effect<Tr>> {
        let mut effects = Vec::new();

        let time_in_view = state.time_in_view();

        // Complaint timeout
        if time_in_view >= self.delta * 6 {
            if let Some(qc) = state.find_maximal_unfinalized() {
                if !state.has_complained(qc) {
                    effects.push(Effect::ComplaintSent {
                        qc: qc.clone(),
                        target: self.leader(state.current_view()),
                    });
                }
            }
        }

        // End-view timeout
        if time_in_view >= self.delta * 12 && state.has_unfinalized() {
            effects.push(Effect::EndViewSent {
                view: state.current_view(),
            });
            effects.push(Effect::MessageSent {
                message: Message::EndView(Arc::new(ThreshPartial::from_data(
                    state.current_view(),
                    &self.kb,
                ))),
                target: None,
            });
        }

        // Also check pending votes after timeout check
        effects.extend(self.check_pending_votes(state));

        effects
    }

    /// Check if we should produce blocks
    fn check_produce_blocks(&self, state: &dyn ProcessState<Tr>) -> Vec<Effect<Tr>> {
        let mut effects = Vec::new();

        // Check transaction block production
        if state.can_produce_tr_block() {
            let (block, block_effects) = self.produce_tr_block(state);
            effects.extend(block_effects);
            effects.push(Effect::MessageSent {
                message: Message::Block(block),
                target: None,
            });
        }

        // Check leader block production
        if self.id == self.leader(state.current_view())
            && state.can_produce_lead_block()
            && state.current_phase() == Phase::High
            && state.tips_count() > 1
        {
            let (block, block_effects) = self.produce_lead_block(state);
            effects.extend(block_effects);
            effects.push(Effect::MessageSent {
                message: Message::Block(block),
                target: None,
            });
        }

        effects
    }

    /// Check pending votes and produce voting effects
    fn check_pending_votes(&self, state: &dyn ProcessState<Tr>) -> Vec<Effect<Tr>> {
        let mut effects = Vec::new();
        let current_view = state.current_view();
        let current_phase = state.current_phase();

        // Check transaction block votes if conditions are met
        if state.contains_lead_in_view(current_view)
            && state.has_unfinalized_lead_in_view(current_view)
        {
            // Check for eligible 1-votes on transaction blocks
            for (block_key, _block) in self.get_pending_tr_blocks(state, current_view) {
                if state.is_eligible_for_tr_1_vote(&block_key) {
                    effects.push(Effect::VoteSent {
                        vote_type: 1,
                        block_key: block_key.clone(),
                        target: None,
                    });
                    effects.push(Effect::PhaseChanged {
                        view: current_view,
                        old_phase: Phase::High,
                        new_phase: Phase::Low,
                    });
                }
            }

            // Check for eligible 2-votes on transaction blocks
            for qc in self.get_pending_tr_1qcs(state, current_view) {
                if state.is_eligible_for_tr_2_vote(&qc.data.for_which) {
                    effects.push(Effect::VoteSent {
                        vote_type: 2,
                        block_key: qc.data.for_which.clone(),
                        target: None,
                    });
                    effects.push(Effect::PhaseChanged {
                        view: current_view,
                        old_phase: Phase::High,
                        new_phase: Phase::Low,
                    });
                }
            }
        }

        // Check leader block votes if still in high phase
        if current_phase == Phase::High {
            // Check for eligible 1-votes on leader blocks
            for (block_key, _block) in self.get_pending_lead_blocks(state, current_view) {
                if block_key.view == current_view {
                    effects.push(Effect::VoteSent {
                        vote_type: 1,
                        block_key: block_key.clone(),
                        target: None,
                    });
                }
            }

            // Check for eligible 2-votes on leader blocks
            for qc in self.get_pending_lead_1qcs(state, current_view) {
                if qc.data.for_which.view == current_view {
                    effects.push(Effect::VoteSent {
                        vote_type: 2,
                        block_key: qc.data.for_which.clone(),
                        target: None,
                    });
                }
            }
        }

        effects
    }

    // Helper methods

    fn leader(&self, view: ViewNum) -> Identity {
        Identity((view.0 as u32 % self.n) + 1)
    }

    fn validate_block(
        &self,
        block: &Arc<Signed<Block<Tr>>>,
        state: &dyn ProcessState<Tr>,
    ) -> Result<(), String> {
        // Create a temporary wrapper to validate blocks
        struct ValidationContext<'a> {
            kb: &'a KeyBook,
            _n: u32,
            _f: u32,
            genesis_qc: &'a FinishedQC,
        }

        impl<'a> ValidationContext<'a> {
            fn block_valid<Tr: Transaction>(
                &self,
                signed_block: &Signed<Block<Tr>>,
            ) -> Result<(), crate::block_validation::BlockValidationError> {
                use crate::block_validation::BlockValidationError;
                let block = &signed_block.data;

                // validate the genesis block, otherwise extract the author
                let _author = if let BlockType::Genesis = block.key.type_ {
                    if block.key == GEN_BLOCK_KEY
                        && block.prev.is_empty()
                        && block.one == *self.genesis_qc
                        && block.data == BlockData::Genesis
                    {
                        return Ok(());
                    } else {
                        return Err(BlockValidationError::InvalidGenesisBlock {
                            key: block.key.clone(),
                        });
                    }
                } else if let Some(auth) = block.key.author.clone() {
                    auth
                } else {
                    return Err(BlockValidationError::MissingAuthor {
                        key: block.key.clone(),
                    });
                };

                if !signed_block.valid_signature(self.kb) {
                    return Err(BlockValidationError::InvalidSignature);
                }

                if block.prev.is_empty() {
                    return Err(BlockValidationError::EmptyPrevPointers);
                }

                // Basic structural validation only - the full validation would require more state
                Ok(())
            }
        }

        let context = ValidationContext {
            kb: &self.kb,
            _n: self.n,
            _f: self.f,
            genesis_qc: state.genesis_qc(),
        };

        match context.block_valid(block) {
            Ok(()) => Ok(()),
            Err(e) => Err(format!("Block validation failed: {:?}", e)),
        }
    }



    fn form_qc_from_votes(
        &self,
        vote_data: &VoteData,
        votes: &[Arc<ThreshPartial<VoteData>>],
    ) -> FinishedQC {
        // Collect partial signatures indexed by author
        let mut vote_sigs: Vec<(usize, hints::PartialSignature)> = Vec::new();

        for vote in votes {
            if vote.data == *vote_data {
                // Convert Identity to index (identities are 1-indexed)
                let author_index = vote.author.0 as usize - 1;
                vote_sigs.push((author_index, vote.signature.clone()));
            }
        }

        // Make sure we have enough votes
        if vote_sigs.len() < (self.n - self.f) as usize {
            panic!(
                "Not enough votes to form QC: {} < {}",
                vote_sigs.len(),
                self.n - self.f
            );
        }

        // Aggregate the signatures
        let agg = self
            .kb
            .hints_setup
            .as_ref()
            .expect("hints setup should be initialized")
            .aggregator();
        let mut data = Vec::new();
        vote_data.serialize_compressed(&mut data).unwrap();

        let signature = hints::sign_aggregate(
            &agg,
            hints::F::from((self.n - self.f) as u64),
            &vote_sigs,
            &data,
        )
        .expect("Failed to aggregate signatures");

        Arc::new(ThreshSigned {
            data: vote_data.clone(),
            signature,
        })
    }

    fn form_view_certificate(
        &self,
        view: &ViewNum,
        _state: &dyn ProcessState<Tr>,
    ) -> Arc<ThreshSigned<ViewNum>> {
        // TODO: critical!
        // In a real implementation, we would aggregate the threshold signatures
        Arc::new(ThreshSigned {
            data: view.incr(),
            signature: hints::Signature::default(),
        })
    }

    fn find_finalized_blocks(
        &self,
        qc: &FinishedQC,
        _state: &dyn ProcessState<Tr>,
    ) -> Vec<BlockKey> {
        // A block is finalized if it has a 2-QC and we've received the QC
        if qc.data.z == 2 {
            vec![qc.data.for_which.clone()]
        } else {
            vec![]
        }
    }

    fn produce_tr_block(
        &self,
        state: &dyn ProcessState<Tr>,
    ) -> (Arc<Signed<Block<Tr>>>, Vec<Effect<Tr>>) {
        let slot = state.current_tr_slot();
        let view = state.current_view();

        // Determine previous QCs
        let mut prev_qcs = Vec::new();
        if !slot.is_zero() {
            if let Some(tr_qc) = state.latest_tr_qc() {
                if tr_qc.data.for_which.slot.is_pred(slot) {
                    prev_qcs.push(tr_qc.clone());
                }
            }
        } else {
            prev_qcs.push(state.genesis_qc().clone());
        }

        // Add single tip if exists
        if state.tips_count() == 1 {
            let tip = state.tips().first().unwrap();
            if !prev_qcs
                .iter()
                .any(|qc| qc.data.for_which == tip.data.for_which)
            {
                prev_qcs.push(tip.clone());
            }
        }

        let height = prev_qcs
            .iter()
            .map(|qc| qc.data.for_which.height)
            .max()
            .unwrap_or(0)
            + 1;

        let block_key = BlockKey {
            type_: BlockType::Tr,
            view,
            height,
            author: Some(self.id.clone()),
            slot,
            hash: Some(BlockHash(self.id.0 as u64 * 0x100 + slot.0)),
        };

        let block = Block {
            key: block_key.clone(),
            prev: prev_qcs,
            one: state.max_1qc().clone(),
            data: BlockData::Tr {
                transactions: state.take_ready_transactions(),
            },
        };

        let signed_block = Arc::new(Signed::from_data(block, &self.kb));

        let effects = vec![
            Effect::BlockProduced {
                block_type: BlockType::Tr,
                block_key: block_key.clone(),
            },
            Effect::SlotAdvanced {
                slot_type: BlockType::Tr,
                new_slot: SlotNum(slot.0 + 1),
            },
        ];

        (signed_block, effects)
    }

    fn produce_lead_block(
        &self,
        state: &dyn ProcessState<Tr>,
    ) -> (Arc<Signed<Block<Tr>>>, Vec<Effect<Tr>>) {
        let slot = state.current_lead_slot();
        let view = state.current_view();

        // Get tips as previous QCs
        let mut prev_qcs: Vec<FinishedQC> = state.tips().to_vec();

        // Add previous leader block QC if needed
        if !slot.is_zero() {
            if let Some(prev_qc) = state.latest_leader_qc() {
                if prev_qc.data.for_which.slot.is_pred(slot) && !prev_qcs
                    .iter()
                    .any(|qc| qc.data.for_which == prev_qc.data.for_which) {
                    prev_qcs.push(prev_qc.clone());
                }
            }
        }

        let height = prev_qcs
            .iter()
            .map(|qc| qc.data.for_which.height)
            .max()
            .unwrap_or(0)
            + 1;

        let has_produced = state.has_produced_lead_in_view(view);

        let (one_qc, justification) = if !has_produced {
            let view_messages = state
                .get_start_views(view)
                .cloned()
                .unwrap_or_default();

            let max_qc = view_messages
                .iter()
                .map(|msg| &msg.data.qc)
                .max_by(|a, b| a.data.compare_qc(&b.data))
                .cloned()
                .unwrap_or_else(|| state.max_1qc().clone());

            let final_qc =
                if max_qc.data.compare_qc(&state.max_1qc().data) == std::cmp::Ordering::Less {
                    state.max_1qc().clone()
                } else {
                    max_qc
                };

            (final_qc, view_messages)
        } else {
            let prev_qc = state
                .latest_leader_1qc()
                .cloned()
                .unwrap_or_else(|| state.max_1qc().clone());
            (prev_qc, vec![])
        };

        let block_key = BlockKey {
            type_: BlockType::Lead,
            view,
            height,
            author: Some(self.id.clone()),
            slot,
            hash: Some(BlockHash(slot.0)),
        };

        let block = Block {
            key: block_key.clone(),
            prev: prev_qcs,
            one: one_qc,
            data: BlockData::Lead { justification },
        };

        let signed_block = Arc::new(Signed::from_data(block, &self.kb));

        let effects = vec![
            Effect::BlockProduced {
                block_type: BlockType::Lead,
                block_key: block_key.clone(),
            },
            Effect::SlotAdvanced {
                slot_type: BlockType::Lead,
                new_slot: SlotNum(slot.0 + 1),
            },
            Effect::LeaderBlockProducedInView { view },
        ];

        (signed_block, effects)
    }

    // Helper methods for getting pending blocks and QCs
    fn get_pending_tr_blocks(
        &self,
        state: &dyn ProcessState<Tr>,
        view: ViewNum,
    ) -> Vec<(BlockKey, Arc<Signed<Block<Tr>>>)> {
        let pending_keys = state.get_unvoted_blocks(view, 1, BlockType::Tr);
        let mut result = Vec::new();
        for key in pending_keys {
            if !state.has_voted(1, &key) {
                if let Some(block) = state.get_block(&key) {
                    result.push((key, block.clone()));
                }
            }
        }
        result
    }

    fn get_pending_tr_1qcs(&self, state: &dyn ProcessState<Tr>, view: ViewNum) -> Vec<FinishedQC> {
        let pending_keys = state.get_unvoted_blocks(view, 2, BlockType::Tr);
        let mut result = Vec::new();
        for qc in state.get_all_qcs() {
            if qc.data.z == 1
                && qc.data.for_which.type_ == BlockType::Tr
                && qc.data.for_which.view == view
                && pending_keys.contains(&qc.data.for_which)
                && !state.has_voted(2, &qc.data.for_which)
            {
                result.push(qc);
            }
        }
        result
    }

    fn get_pending_lead_blocks(
        &self,
        state: &dyn ProcessState<Tr>,
        view: ViewNum,
    ) -> Vec<(BlockKey, Arc<Signed<Block<Tr>>>)> {
        let pending_keys = state.get_unvoted_blocks(view, 1, BlockType::Lead);
        let mut result = Vec::new();
        for key in pending_keys {
            if !state.has_voted(1, &key) {
                if let Some(block) = state.get_block(&key) {
                    result.push((key, block.clone()));
                }
            }
        }
        result
    }

    fn get_pending_lead_1qcs(
        &self,
        state: &dyn ProcessState<Tr>,
        view: ViewNum,
    ) -> Vec<FinishedQC> {
        let pending_keys = state.get_unvoted_blocks(view, 2, BlockType::Lead);
        let mut result = Vec::new();
        for qc in state.get_all_qcs() {
            if qc.data.z == 1
                && qc.data.for_which.type_ == BlockType::Lead
                && qc.data.for_which.view == view
                && pending_keys.contains(&qc.data.for_which)
                && !state.has_voted(2, &qc.data.for_which)
            {
                result.push(qc);
            }
        }
        result
    }
}

/// Readonly view of process state for pure computations
pub trait ProcessState<Tr: Transaction> {
    fn current_view(&self) -> ViewNum;
    fn current_phase(&self) -> Phase;
    fn current_time(&self) -> u128;
    fn view_entry_time(&self) -> u128;
    fn time_in_view(&self) -> u128;
    fn max_1qc(&self) -> &FinishedQC;
    fn has_qc(&self, qc: &FinishedQC) -> bool;
    fn count_votes(&self, vote_data: &VoteData) -> usize;
    fn count_end_views(&self, view: &ViewNum) -> usize;
    fn find_maximal_unfinalized(&self) -> Option<&FinishedQC>;
    fn has_complained(&self, qc: &FinishedQC) -> bool;
    fn has_unfinalized(&self) -> bool;
    fn can_produce_tr_block(&self) -> bool;
    fn can_produce_lead_block(&self) -> bool;
    fn tips_count(&self) -> usize;

    // Additional methods needed for implementation
    fn genesis_qc(&self) -> &FinishedQC;
    fn tips(&self) -> &Vec<FinishedQC>;
    fn current_tr_slot(&self) -> SlotNum;
    fn current_lead_slot(&self) -> SlotNum;
    fn latest_tr_qc(&self) -> Option<&FinishedQC>;
    fn latest_leader_qc(&self) -> Option<&FinishedQC>;
    fn latest_leader_1qc(&self) -> Option<&FinishedQC>;
    fn take_ready_transactions(&self) -> Vec<Tr>;
    fn has_produced_lead_in_view(&self, view: ViewNum) -> bool;
    fn get_start_views(&self, view: ViewNum) -> Option<&Vec<Arc<Signed<StartView>>>>;

    // Voting eligibility methods
    fn is_eligible_for_tr_1_vote(&self, block_key: &BlockKey) -> bool;
    fn is_eligible_for_tr_2_vote(&self, block_key: &BlockKey) -> bool;
    fn block_is_single_tip(&self, block_key: &BlockKey) -> bool;
    fn contains_lead_in_view(&self, view: ViewNum) -> bool;
    fn has_unfinalized_lead_in_view(&self, view: ViewNum) -> bool;
    fn get_block(&self, key: &BlockKey) -> Option<&Arc<Signed<Block<Tr>>>>;

    // Pending votes tracking
    fn get_unvoted_blocks(
        &self,
        view: ViewNum,
        vote_type: u8,
        block_type: BlockType,
    ) -> Vec<BlockKey>;
    fn has_voted(&self, vote_type: u8, block_key: &BlockKey) -> bool;
    fn get_all_blocks(&self) -> Vec<(BlockKey, Arc<Signed<Block<Tr>>>)>;
    fn get_all_qcs(&self) -> Vec<FinishedQC>;
    fn get_votes_for(&self, vote_data: &VoteData) -> Vec<Arc<ThreshPartial<VoteData>>>;
    fn has_vote_from(&self, sender: &Identity, vote_data: &VoteData) -> bool;
}
