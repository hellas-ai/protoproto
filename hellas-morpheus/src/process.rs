use std::{
    collections::BTreeSet,
    sync::Arc,
};

use crate::block_producer::BlockProducer;
use crate::effects::Effect;
use crate::event_log::EventLog;
use crate::processor::{ActionProcessor, ProcessState};
use crate::state_tracking::StateIndex;
use crate::timeout_manager::TimeoutManager;
use crate::view_manager::ViewManager;
use crate::vote_manager::VoteManager;
use crate::*;
use fastbloom::BloomFilter;
use redb::{ReadableTableMetadata, TableDefinition};
use serde::{Deserialize, Serialize};

/// MorpheusProcess represents a single process (p_i) in the Morpheus protocol
///
/// This struct now uses a component-based architecture with event sourcing
#[derive(Clone, derive_more::Debug, Serialize, Deserialize, derivative::Derivative)]
#[derivative(PartialEq)]
pub struct MorpheusProcess<Tr: Transaction> {
    // Core identity and configuration
    #[debug(skip)]
    pub kb: KeyBook,
    pub chainid: [u8; 32],
    pub id: Identity,
    pub n: u32,
    pub f: u32,

    // Component managers
    pub view_manager: ViewManager,
    pub vote_manager: VoteManager,
    #[serde(bound(serialize = "Tr: Transaction", deserialize = "Tr: Transaction"))]
    pub block_producer: BlockProducer<Tr>,
    pub timeout_manager: TimeoutManager,

    // State tracking
    #[serde(bound(serialize = "Tr: Transaction", deserialize = "Tr: Transaction"))]
    pub index: StateIndex<Tr>,

    // Genesis data
    #[serde(bound(serialize = "Tr: Transaction", deserialize = "Tr: Transaction"))]
    pub genesis: Arc<Signed<Block<Tr>>>,
    pub genesis_qc: FinishedQC,

    // Message deduplication
    #[debug(skip)]
    pub seen_messages: BloomFilter,
    #[debug(skip)]
    pub seen_message_hashes: BTreeSet<[u8; 32]>,

    // Event sourcing
    #[serde(bound(serialize = "Tr: Transaction", deserialize = "Tr: Transaction"))]
    pub event_log: EventLog<Tr>,

    // Action processor for pure logic
    #[serde(skip)]
    #[derivative(PartialEq = "ignore")]
    pub processor: ActionProcessor<Tr>,
}

impl<Tr: Transaction> MorpheusProcess<Tr> {
    pub fn new(db: &redb::Database, keybook: KeyBook, id: Identity, n: u32, f: u32) -> Self {
        crate::tracing_setup::register_process(&id, n, f);

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

        let delta = 10; // 10 ... "units"

        let processor = ActionProcessor::new(id.clone(), n, f, delta, keybook.clone());

        let mut p = MorpheusProcess {
            kb: keybook,
            chainid: [0; 32],
            id,
            n,
            f,
            view_manager: ViewManager::new(n, delta),
            vote_manager: VoteManager::new(),
            block_producer: BlockProducer::new(),
            timeout_manager: TimeoutManager::new(delta),
            index: StateIndex::new(genesis_qc.clone(), genesis_block.clone()),
            genesis: genesis_block.clone(),
            genesis_qc: genesis_qc.clone(),
            seen_messages: BloomFilter::with_num_bits(8 * 1024 * 16)
                .seed(&0x8F3A57D2C19E4B7F0123456789ABCDEF)
                .expected_items(100_000),
            seen_message_hashes: BTreeSet::new(),
            event_log: EventLog::new(db),
            processor,
        };

        // Process genesis block and QC
        p.handle_action(
            db,
            Action::ProcessMessage {
                sender: Identity(u32::MAX),
                payload: Message::Block(genesis_block.clone()),
            },
        );
        p.handle_action(
            db,
            Action::ProcessMessage {
                sender: Identity(u32::MAX),
                payload: Message::QC(genesis_qc.clone()),
            },
        );

        p
    }

    /// Initialize processor after deserialization
    pub fn init_processor(&mut self) {
        let delta = 10; // Same as in new()
        self.processor = ActionProcessor::new(
            self.id.clone(),
            self.n,
            self.f,
            delta,
            self.kb.clone(),
        );
    }

    /// Handle an action by processing it and applying effects
    pub fn handle_action(
        &mut self,
        db: &redb::Database,
        action: Action<Tr>,
    ) -> Vec<(Message<Tr>, Option<Identity>)> {
        // Process the action to get effects
        let effects = self.processor.process_action(&action, self);

        // Apply the effects to update state
        let messages = self.apply_effects(&effects);

        // Record the action and effects to the event log
        let entry = LogEntry { action, effects };
        self.event_log.record_entry(db, entry);

        messages
    }

    /// Apply effects to update the process state
    fn apply_effects(&mut self, effects: &[Effect<Tr>]) -> Vec<(Message<Tr>, Option<Identity>)> {
        let mut messages = Vec::new();

        for effect in effects {
            match effect {
                Effect::TimeUpdated(time) => {
                    self.timeout_manager.set_time(*time);
                }
                Effect::ViewChanged {
                    old_view,
                    new_view,
                    cause,
                } => {
                    self.view_manager
                        .enter_view(*new_view, self.timeout_manager.current_time);
                    crate::tracing_setup::protocol_transition(
                        &self.id,
                        "view_change",
                        old_view,
                        new_view,
                        Some(cause),
                    );
                }
                Effect::PhaseChanged {
                    view: _,
                    old_phase: _,
                    new_phase,
                } => {
                    self.view_manager.set_phase(*new_phase);
                }
                Effect::BlockRecorded { block } => {
                    self.record_block(block);
                }
                Effect::QcRecorded {
                    qc,
                    finalized_blocks: _,
                } => {
                    self.record_qc(qc.clone());
                    // Handle finalized blocks if needed
                }
                Effect::VoteSent {
                    vote_type,
                    block_key,
                    target,
                } => {
                    self.vote_manager.record_vote_sent(
                        *vote_type,
                        block_key.type_,
                        block_key.slot,
                        block_key.author.clone().unwrap(),
                    );
                    let vote = Arc::new(ThreshPartial::from_data(
                        VoteData {
                            z: *vote_type,
                            for_which: block_key.clone(),
                        },
                        &self.kb,
                    ));
                    messages.push((Message::NewVote(vote), target.clone()));
                }
                Effect::VoteRecorded { voter: _, vote_data: _ } => {
                    // Vote recording is handled internally by vote tracking
                }
                Effect::QuorumReached {
                    vote_data: _,
                    qc_formed: _,
                } => {
                    // QC formation is handled by the processor
                }
                Effect::TransactionsUpdated { transactions } => {
                    self.block_producer
                        .set_ready_transactions(transactions.clone());
                }
                Effect::BlockProduced {
                    block_type,
                    block_key,
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
                Effect::MessageSent { message, target } => {
                    messages.push((message.clone(), target.clone()));
                }
                Effect::ComplaintSent { qc, target } => {
                    self.view_manager.mark_complained(qc.clone());
                    messages.push((Message::QC(qc.clone()), Some(target.clone())));
                }
                Effect::EndViewSent { view } => {
                    let end_view = Arc::new(ThreshPartial::from_data(*view, &self.kb));
                    messages.push((Message::EndView(end_view), None));
                }
                Effect::ViewCertificateFormed { view: _ } => {
                    // Certificate formation is handled by the processor
                }
                Effect::SlotAdvanced {
                    slot_type,
                    new_slot: _,
                } => {
                    if *slot_type == BlockType::Lead {
                        self.block_producer.advance_lead_slot();
                    } else {
                        self.block_producer.advance_tr_slot();
                    }
                }
                Effect::LeaderBlockProducedInView { view } => {
                    self.block_producer.mark_lead_produced(*view);
                }
            }
        }

        messages
    }

    /// Process a message using the event sourcing pattern
    pub fn process_message(
        &mut self,
        db: &redb::Database,
        message: Message<Tr>,
        sender: Identity,
    ) -> Vec<(Message<Tr>, Option<Identity>)> {
        // Skip duplicate detection during replay
        if !self.event_log.replaying {
            if self.seen_messages.contains(&message) {
                let bytes = postcard::to_stdvec(&message).unwrap();
                let hash = blake3::hash(&bytes);
                if self.seen_message_hashes.contains(hash.as_bytes()) {
                    tracing::error!(
                        target: "duplicate_message",
                        sender = ?sender,
                        full_message = format::format_message(&message, false),
                        "Ignoring duplicate message: why did we receive it?"
                    );
                    return Vec::new();
                }
            }
        }

        // Add to seen messages
        self.seen_messages.insert(&message);
        let bytes = postcard::to_stdvec(&message).unwrap();
        let hash = blake3::hash(&bytes);
        self.seen_message_hashes.insert(*hash.as_bytes());

        // Handle the action
        self.handle_action(
            db,
            Action::ProcessMessage {
                sender,
                payload: message,
            },
        )
    }

    /// Set ready transactions
    pub fn set_ready_transactions(
        &mut self,
        db: &redb::Database,
        transactions: Vec<Tr>,
    ) -> Vec<(Message<Tr>, Option<Identity>)> {
        self.handle_action(db, Action::SetReadyTransactions(transactions))
    }

    /// Set current time
    pub fn set_now(
        &mut self,
        db: &redb::Database,
        now: u128,
    ) -> Vec<(Message<Tr>, Option<Identity>)> {
        self.handle_action(db, Action::SetTime(now))
    }

    /// Check timeouts
    pub fn check_timeouts(&mut self, db: &redb::Database) -> Vec<(Message<Tr>, Option<Identity>)> {
        self.handle_action(db, Action::CheckTimeouts)
    }

    /// Try to produce blocks
    pub fn try_produce_blocks(
        &mut self,
        db: &redb::Database,
    ) -> Vec<(Message<Tr>, Option<Identity>)> {
        self.handle_action(db, Action::CheckProduceBlocks)
    }

    // Helper methods for state updates (called by apply_effects)

    fn record_block(&mut self, block: &Arc<Signed<Block<Tr>>>) {
        // Insert block into DAG
        if !self.index.dag.insert_block(block) {
            return;
        }

        if let Some(author) = &block.data.key.author {
            if block.data.key.type_ == BlockType::Lead && author == &self.id {
                self.block_producer.mark_lead_produced(block.data.key.view);
            }
        }

        // Update view index for leader blocks
        if block.data.key.type_ == BlockType::Lead {
            self.index.view_index.insert_leader_block(&block.data.key);
        }

        // Track voting status
        self.vote_manager
            .mark_pending_votes_dirty(block.data.key.view);

        // Add to pending votes tracking
        let pending = self
            .vote_manager
            .pending_votes
            .entry(block.data.key.view)
            .or_default();
        match block.data.key.type_ {
            BlockType::Lead => {
                pending.lead_1.insert(block.data.key.clone(), true);
            }
            BlockType::Tr => {
                pending.tr_1.insert(block.data.key.clone(), true);
            }
            BlockType::Genesis => {}
        }

        // Record any QCs in the block
        for qc in &block.data.prev {
            self.record_qc(qc.clone())
        }
        self.record_qc(block.data.one.clone());
    }

    fn record_qc(&mut self, qc: FinishedQC) {
        // Insert QC into index
        if !self.index.qc_index.insert_qc(qc.clone()) {
            return;
        }

        if qc.data.for_which.type_ == BlockType::Genesis {
            return;
        }

        // Update latest QCs
        self.index.qc_index.update_latest_qcs(
            &qc,
            &self.id,
            self.block_producer.current_lead_slot(),
            self.block_producer.current_tr_slot(),
        );

        // Update DAG tips
        self.index.dag.update_tips_for_qc(qc.clone());

        // Finalize blocks
        let finalized_here = {
            let dag = &self.index.dag;
            self.index
                .qc_index
                .finalize_blocks_observed_by(&qc, |a, b| dag.observes(a, b))
        };

        // Update view index for finalized blocks
        for finalized in &finalized_here {
            self.index
                .view_index
                .finalize_leader_block(&finalized.data.for_which);
            self.vote_manager
                .mark_pending_votes_dirty(finalized.data.for_which.view);
        }

        // Track 2-votes
        if qc.data.z == 1 {
            self.vote_manager
                .mark_pending_votes_dirty(qc.data.for_which.view);

            // Add to pending 2-votes
            let pending = self
                .vote_manager
                .pending_votes
                .entry(qc.data.for_which.view)
                .or_default();
            match qc.data.for_which.type_ {
                BlockType::Lead => {
                    pending.lead_2.insert(qc.data.for_which.clone(), true);
                }
                BlockType::Tr => {
                    pending.tr_2.insert(qc.data.for_which.clone(), true);
                }
                BlockType::Genesis => {}
            }
        }
    }
}

// Implement ProcessState trait for MorpheusProcess
impl<Tr: Transaction> ProcessState<Tr> for MorpheusProcess<Tr> {
    fn current_view(&self) -> ViewNum {
        self.view_manager.current_view()
    }

    fn current_phase(&self) -> Phase {
        self.view_manager.phase(self.current_view())
    }

    fn current_time(&self) -> u128 {
        self.timeout_manager.current_time
    }

    fn view_entry_time(&self) -> u128 {
        self.view_manager.view_entry_time
    }

    fn time_in_view(&self) -> u128 {
        self.view_manager.time_in_view(self.current_time())
    }

    fn max_1qc(&self) -> &FinishedQC {
        self.index.max_1qc()
    }

    fn has_qc(&self, qc: &FinishedQC) -> bool {
        self.index.qc_index.contains_qc(qc)
    }

    fn count_votes(&self, vote_data: &VoteData) -> usize {
        self.vote_manager
            .vote_tracker
            .votes
            .get(vote_data)
            .map(|votes| votes.len())
            .unwrap_or(0)
    }

    fn count_end_views(&self, view: &ViewNum) -> usize {
        self.vote_manager
            .end_views
            .votes
            .get(view)
            .map(|votes| votes.len())
            .unwrap_or(0)
    }

    fn find_maximal_unfinalized(&self) -> Option<&FinishedQC> {
        use std::cmp::Ordering;
        self.index
            .unfinalized()
            .iter()
            .flat_map(|(_, qcs)| qcs)
            .max_by(|&qc1, &qc2| {
                let dag = &self.index.dag;
                if dag.observes(&qc1.data, &qc2.data) {
                    Ordering::Greater
                } else if dag.observes(&qc2.data, &qc1.data) {
                    Ordering::Less
                } else {
                    Ordering::Equal
                }
            })
    }

    fn has_complained(&self, qc: &FinishedQC) -> bool {
        self.view_manager.complained_qcs.contains(qc)
    }

    fn has_unfinalized(&self) -> bool {
        !self.index.unfinalized().is_empty()
    }

    fn can_produce_tr_block(&self) -> bool {
        let has_transactions = self.block_producer.has_ready_transactions();
        let slot = self.block_producer.current_tr_slot();

        if !slot.is_zero() {
            let has_prev_qc = self
                .index
                .latest_tr_qc()
                .as_ref()
                .map(|qc| qc.data.for_which.slot.is_pred(slot))
                .unwrap_or(false);
            has_transactions && has_prev_qc
        } else {
            has_transactions
        }
    }

    fn can_produce_lead_block(&self) -> bool {
        let view = self.current_view();
        let slot = self.block_producer.current_lead_slot();
        let has_produced = self.block_producer.has_produced_lead_in_view(view);

        if has_produced {
            self.index
                .latest_leader_1qc()
                .map(|qc| qc.data.for_which.slot.is_pred(slot))
                .unwrap_or(false)
        } else {
            let has_enough_msgs = self.view_manager.has_enough_start_views(view, self.f);
            let has_prev_qc = slot.is_zero()
                || self
                    .index
                    .latest_leader_qc()
                    .map(|qc| qc.data.for_which.slot.is_pred(slot))
                    .unwrap_or(false);
            has_enough_msgs && has_prev_qc
        }
    }

    fn tips_count(&self) -> usize {
        self.index.tips().len()
    }

    // Additional methods
    fn genesis_qc(&self) -> &FinishedQC {
        &self.genesis_qc
    }

    fn tips(&self) -> &Vec<FinishedQC> {
        self.index.tips()
    }

    fn current_tr_slot(&self) -> SlotNum {
        self.block_producer.current_tr_slot()
    }

    fn current_lead_slot(&self) -> SlotNum {
        self.block_producer.current_lead_slot()
    }

    fn latest_tr_qc(&self) -> Option<&FinishedQC> {
        self.index.latest_tr_qc()
    }

    fn latest_leader_qc(&self) -> Option<&FinishedQC> {
        self.index.latest_leader_qc()
    }

    fn latest_leader_1qc(&self) -> Option<&FinishedQC> {
        self.index.latest_leader_1qc()
    }

    fn take_ready_transactions(&self) -> Vec<Tr> {
        // For a read-only interface, we can only clone
        self.block_producer.ready_transactions.clone()
    }

    fn has_produced_lead_in_view(&self, view: ViewNum) -> bool {
        self.block_producer.has_produced_lead_in_view(view)
    }

    fn get_start_views(&self, view: ViewNum) -> Option<&Vec<Arc<Signed<StartView>>>> {
        self.view_manager.get_start_views(view)
    }

    // Voting eligibility implementations
    fn is_eligible_for_tr_1_vote(&self, block_key: &BlockKey) -> bool {
        let has_single_tip = self.block_is_single_tip(block_key);

        if !has_single_tip || !self.index.dag.contains_block(block_key) {
            return false;
        }

        if let Some(block) = self.index.dag.blocks.get(block_key) {
            block
                .data
                .one
                .data
                .compare_qc(&self.index.qc_index.max_1qc.data)
                != std::cmp::Ordering::Less
        } else {
            false
        }
    }

    fn is_eligible_for_tr_2_vote(&self, block_key: &BlockKey) -> bool {
        let has_single_tip = self.index.dag.tips.len() == 1
            && self.index.dag.tips.get(0).map_or(false, |tip| {
                tip.data.z == 1 && tip.data.for_which.eq(block_key)
            });

        let no_higher_blocks = self.index.dag.max_height.0 <= block_key.height;

        has_single_tip && no_higher_blocks
    }

    fn block_is_single_tip(&self, block_key: &BlockKey) -> bool {
        if self.index.dag.tips.len() != 1 {
            return false;
        }
        match self.index.dag.tips.get(0) {
            Some(tip) => self
                .index
                .dag
                .block_pointed_by
                .get(&tip.data.for_which)
                .map_or(false, |parents| {
                    parents.len() == 1 && parents.first().unwrap() == block_key
                }),
            None => false,
        }
    }

    fn contains_lead_in_view(&self, view: ViewNum) -> bool {
        self.index
            .view_index
            .contains_lead_by_view
            .get(&view)
            .copied()
            .unwrap_or(false)
    }

    fn has_unfinalized_lead_in_view(&self, view: ViewNum) -> bool {
        self.index
            .view_index
            .unfinalized_lead_by_view
            .get(&view)
            .map_or(false, |set| !set.is_empty())
    }

    fn get_block(&self, key: &BlockKey) -> Option<&Arc<Signed<Block<Tr>>>> {
        self.index.dag.blocks.get(key)
    }

    // Pending votes tracking implementations
    fn get_unvoted_blocks(
        &self,
        view: ViewNum,
        vote_type: u8,
        block_type: BlockType,
    ) -> Vec<BlockKey> {
        let pending = self.vote_manager.pending_votes.get(&view);
        match (vote_type, block_type) {
            (1, BlockType::Tr) => pending
                .map(|p| p.tr_1.keys().cloned().collect())
                .unwrap_or_default(),
            (2, BlockType::Tr) => pending
                .map(|p| p.tr_2.keys().cloned().collect())
                .unwrap_or_default(),
            (1, BlockType::Lead) => pending
                .map(|p| p.lead_1.keys().cloned().collect())
                .unwrap_or_default(),
            (2, BlockType::Lead) => pending
                .map(|p| p.lead_2.keys().cloned().collect())
                .unwrap_or_default(),
            _ => vec![],
        }
    }

    fn has_voted(&self, vote_type: u8, block_key: &BlockKey) -> bool {
        if let Some(author) = &block_key.author {
            self.vote_manager
                .has_voted(vote_type, block_key.type_, block_key.slot, author.clone())
        } else {
            false
        }
    }

    fn get_all_blocks(&self) -> Vec<(BlockKey, Arc<Signed<Block<Tr>>>)> {
        self.index
            .dag
            .blocks
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    fn get_all_qcs(&self) -> Vec<FinishedQC> {
        self.index.qc_index.qcs.iter().cloned().collect()
    }

    fn get_votes_for(&self, vote_data: &VoteData) -> Vec<Arc<ThreshPartial<VoteData>>> {
        self.vote_manager
            .vote_tracker
            .votes
            .get(vote_data)
            .cloned()
            .unwrap_or_default()
            .values()
            .cloned()
            .collect()
    }

    fn has_vote_from(&self, sender: &Identity, vote_data: &VoteData) -> bool {
        self.vote_manager
            .vote_tracker
            .votes
            .get(vote_data)
            .map(|votes| votes.iter().any(|v| sender == v.0))
            .unwrap_or(false)
    }
}
