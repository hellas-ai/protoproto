//! Version 2 of MorpheusProcess using the new storage architecture

use std::{collections::BTreeSet, sync::Arc};

use crate::block_producer::BlockProducer;
use crate::effects::Effect;
use crate::event_log::EventLog;
use crate::processor::{ActionProcessor, ProcessState};
use crate::storage::{
    log_invariant_violations, BlockRef, BulkStore, ConsensusState, InvariantCheckConfig,
    InvariantChecker, LightweightDAGIndex, QCRef, SnapshotStore, ViewCache,
};
use crate::timeout_manager::TimeoutManager;
use crate::view_manager::ViewManager;
use crate::vote_manager::VoteManager;
use crate::*;
use fastbloom::BloomFilter;
use serde::{Deserialize, Serialize};

/// Serializable process state (without storage implementations)
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(bound(serialize = "Tr: Transaction", deserialize = "Tr: Transaction"))]
pub struct ProcessSnapshot<Tr: Transaction> {
    // Core identity and configuration
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

    // Core protocol state
    pub consensus_state: ConsensusState,
    #[serde(bound(serialize = "Tr: Transaction", deserialize = "Tr: Transaction"))]
    pub view_cache: ViewCache<Tr>,
    pub lightweight_dag: LightweightDAGIndex,

    // Genesis data
    pub genesis_ref: BlockRef,
    pub genesis_qc_ref: QCRef,
    pub genesis_qc: Arc<ThreshSigned<VoteData>>, // Keep in memory

    // Message deduplication
    pub seen_message_hashes: BTreeSet<[u8; 32]>,

    // Event sourcing
    #[serde(bound(serialize = "Tr: Transaction", deserialize = "Tr: Transaction"))]
    pub event_log: EventLog<Tr>,
}

/// MorpheusProcess with storage architecture
#[derive(Clone, derive_more::Debug)]
pub struct MorpheusProcess<Tr: Transaction, B: BulkStore<Tr>, S: SnapshotStore> {
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
    pub block_producer: BlockProducer<Tr>,
    pub timeout_manager: TimeoutManager,

    // Storage layers
    pub bulk_store: B,
    pub snapshot_store: S,

    // Current view cache (only current view data in memory)
    pub view_cache: ViewCache<Tr>,

    // Lightweight DAG index
    pub lightweight_dag: LightweightDAGIndex,

    // Lightweight state (references only)
    pub finalized_blocks: im::HashSet<BlockRef>,
    pub unfinalized_qcs: im::HashMap<BlockRef, im::HashSet<QCRef>>,

    // Core protocol state
    pub max_1qc_ref: QCRef,
    pub tips_refs: im::Vector<QCRef>,
    pub cached_max_1qc: Option<FinishedQC>, // Cache for ProcessState trait
    pub cached_tips: Vec<FinishedQC>,       // Cache for ProcessState trait

    // Genesis data
    pub genesis_ref: BlockRef,
    pub genesis_qc_ref: QCRef,
    pub genesis_qc: Arc<ThreshSigned<VoteData>>, // Keep in memory

    // Message deduplication
    #[debug(skip)]
    pub seen_messages: BloomFilter,
    #[debug(skip)]
    pub seen_message_hashes: BTreeSet<[u8; 32]>,

    // Event sourcing
    pub event_log: EventLog<Tr>,

    // Action processor for pure logic
    pub processor: ActionProcessor<Tr>,

    // Storage invariant checking
    pub invariant_checker: Option<InvariantChecker>,
}

impl<Tr: Transaction, B: BulkStore<Tr>, S: SnapshotStore> MorpheusProcess<Tr, B, S> {
    pub fn new(
        db: &redb::Database,
        keybook: KeyBook,
        id: Identity,
        n: u32,
        f: u32,
        bulk_store: B,
        snapshot_store: S,
        invariant_check_config: Option<InvariantCheckConfig>,
    ) -> Result<Self, String> {
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

        // Create genesis references
        let genesis_ref = BlockRef {
            key: GEN_BLOCK_KEY,
            hash: GEN_BLOCK_KEY.hash,
        };

        let genesis_qc_ref = QCRef {
            vote_data: genesis_qc.data.clone(),
            hash: None,
        };

        let invariant_checker = invariant_check_config.map(InvariantChecker::new);

        let mut process = MorpheusProcess {
            kb: keybook,
            chainid: [0; 32],
            id,
            n,
            f,
            view_manager: ViewManager::new(n, delta),
            vote_manager: VoteManager::new(),
            block_producer: BlockProducer::new(),
            timeout_manager: TimeoutManager::new(delta),
            bulk_store,
            snapshot_store,
            view_cache: ViewCache::new(ViewNum(0)),
            lightweight_dag: LightweightDAGIndex::new(),
            finalized_blocks: im::HashSet::new(),
            unfinalized_qcs: im::HashMap::new(),
            max_1qc_ref: genesis_qc_ref.clone(),
            tips_refs: im::vector![genesis_qc_ref.clone()],
            genesis_ref: genesis_ref.clone(),
            genesis_qc_ref: genesis_qc_ref.clone(),
            genesis_qc: genesis_qc.clone(),
            seen_messages: BloomFilter::with_num_bits(8 * 1024 * 16)
                .seed(&0x8F3A57D2C19E4B7F0123456789ABCDEF)
                .expected_items(100_000),
            seen_message_hashes: BTreeSet::new(),
            event_log: EventLog::new(db),
            processor,
            invariant_checker,
            cached_max_1qc: Some(genesis_qc.clone()),
            cached_tips: vec![genesis_qc.clone()],
        };

        // Store genesis in bulk storage
        process.bulk_store.append_block(genesis_block.clone())?;
        process.bulk_store.append_qc(genesis_qc.clone())?;

        // Add genesis to lightweight DAG
        process
            .lightweight_dag
            .insert_block_ref(genesis_ref.clone());

        // Process genesis block and QC
        process.handle_action(
            db,
            Action::ProcessMessage {
                sender: Identity(u32::MAX),
                payload: Message::Block(genesis_block.clone()),
            },
        )?;
        process.handle_action(
            db,
            Action::ProcessMessage {
                sender: Identity(u32::MAX),
                payload: Message::QC(genesis_qc.clone()),
            },
        )?;

        Ok(process)
    }

    /// Create from a snapshot
    pub fn from_snapshot(
        snapshot: ProcessSnapshot<Tr>,
        bulk_store: B,
        snapshot_store: S,
        invariant_check_config: Option<InvariantCheckConfig>,
    ) -> Self {
        let delta = 10;
        let processor = ActionProcessor::new(
            snapshot.id.clone(),
            snapshot.n,
            snapshot.f,
            delta,
            snapshot.kb.clone(),
        );

        let invariant_checker = invariant_check_config.map(InvariantChecker::new);

        // Initialize caches from snapshot data
        let cached_max_1qc = Some(snapshot.genesis_qc.clone()); // TODO: Load actual max 1qc
        let cached_tips = vec![snapshot.genesis_qc.clone()]; // TODO: Load actual tips

        MorpheusProcess {
            kb: snapshot.kb,
            chainid: snapshot.chainid,
            id: snapshot.id,
            n: snapshot.n,
            f: snapshot.f,
            view_manager: snapshot.view_manager,
            vote_manager: snapshot.vote_manager,
            block_producer: snapshot.block_producer,
            timeout_manager: snapshot.timeout_manager,
            bulk_store,
            snapshot_store,
            view_cache: snapshot.view_cache,
            lightweight_dag: snapshot.lightweight_dag,
            finalized_blocks: snapshot.consensus_state.finalized_blocks,
            unfinalized_qcs: snapshot.consensus_state.unfinalized_qcs,
            max_1qc_ref: snapshot.consensus_state.max_1qc,
            tips_refs: im::Vector::from(snapshot.consensus_state.tips),
            genesis_ref: snapshot.genesis_ref,
            genesis_qc_ref: snapshot.genesis_qc_ref,
            genesis_qc: snapshot.genesis_qc,
            seen_messages: BloomFilter::with_num_bits(8 * 1024 * 16)
                .seed(&0x8F3A57D2C19E4B7F0123456789ABCDEF)
                .expected_items(100_000),
            seen_message_hashes: snapshot.seen_message_hashes,
            event_log: snapshot.event_log,
            processor,
            invariant_checker,
            cached_max_1qc,
            cached_tips,
        }
    }

    /// Convert to a serializable snapshot
    pub fn to_snapshot(&self) -> ProcessSnapshot<Tr> {
        ProcessSnapshot {
            kb: self.kb.clone(),
            chainid: self.chainid,
            id: self.id.clone(),
            n: self.n,
            f: self.f,
            view_manager: self.view_manager.clone(),
            vote_manager: self.vote_manager.clone(),
            block_producer: self.block_producer.clone(),
            timeout_manager: self.timeout_manager.clone(),
            consensus_state: ConsensusState {
                current_view: self.view_manager.current_view(),
                current_phase: self.view_manager.phase(self.view_manager.current_view()),
                view_entry_time: self.view_manager.view_entry_time,
                tips: self.tips_refs.iter().cloned().collect(),
                max_1qc: self.max_1qc_ref.clone(),
                finalized_blocks: self.finalized_blocks.clone(),
                unfinalized_qcs: self.unfinalized_qcs.clone(),
                leader_blocks_by_view: im::HashMap::new(), // TODO: Implement if needed
                unfinalized_leader_by_view: im::HashMap::new(), // TODO: Implement if needed
            },
            view_cache: self.view_cache.clone(),
            lightweight_dag: self.lightweight_dag.clone(),
            genesis_ref: self.genesis_ref.clone(),
            genesis_qc_ref: self.genesis_qc_ref.clone(),
            genesis_qc: self.genesis_qc.clone(),
            seen_message_hashes: self.seen_message_hashes.clone(),
            event_log: self.event_log.clone(),
        }
    }

    /// Save a snapshot of current state
    pub fn save_snapshot(&mut self) -> Result<(), String> {
        let consensus_state = ConsensusState {
            current_view: self.view_manager.current_view(),
            current_phase: self.view_manager.phase(self.view_manager.current_view()),
            view_entry_time: self.view_manager.view_entry_time,
            tips: self.tips_refs.iter().cloned().collect(),
            max_1qc: self.max_1qc_ref.clone(),
            finalized_blocks: self.finalized_blocks.clone(),
            unfinalized_qcs: self.unfinalized_qcs.clone(),
            leader_blocks_by_view: im::HashMap::new(), // TODO: Implement
            unfinalized_leader_by_view: im::HashMap::new(), // TODO: Implement
        };

        self.snapshot_store.save_snapshot(&consensus_state)?;
        Ok(())
    }

    /// Process a message using the event sourcing pattern
    pub fn process_message(
        &mut self,
        db: &redb::Database,
        message: Message<Tr>,
        sender: Identity,
    ) -> Result<Vec<(Message<Tr>, Option<Identity>)>, String> {
        // Skip duplicate detection during replay
        if !self.event_log.replaying && self.seen_messages.contains(&message) {
            let bytes = postcard::to_stdvec(&message).unwrap();
            let hash = blake3::hash(&bytes);
            if self.seen_message_hashes.contains(hash.as_bytes()) {
                tracing::error!(
                    target: "duplicate_message",
                    sender = ?sender,
                    full_message = format::format_message(&message, false),
                    "Ignoring duplicate message: why did we receive it?"
                );
                return Ok(Vec::new());
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
    ) -> Result<Vec<(Message<Tr>, Option<Identity>)>, String> {
        self.handle_action(db, Action::SetReadyTransactions(transactions))
    }

    /// Set current time
    pub fn set_now(
        &mut self,
        db: &redb::Database,
        now: u128,
    ) -> Result<Vec<(Message<Tr>, Option<Identity>)>, String> {
        self.handle_action(db, Action::SetTime(now))
    }

    /// Check timeouts
    pub fn check_timeouts(
        &mut self,
        db: &redb::Database,
    ) -> Result<Vec<(Message<Tr>, Option<Identity>)>, String> {
        self.handle_action(db, Action::CheckTimeouts)
    }

    /// Try to produce blocks
    pub fn try_produce_blocks(
        &mut self,
        db: &redb::Database,
    ) -> Result<Vec<(Message<Tr>, Option<Identity>)>, String> {
        self.handle_action(db, Action::CheckProduceBlocks)
    }

    /// Handle an action by processing it and applying effects
    pub fn handle_action(
        &mut self,
        db: &redb::Database,
        action: Action<Tr>,
    ) -> Result<Vec<(Message<Tr>, Option<Identity>)>, String> {
        // Process the action to get effects
        let effects = self.processor.process_action(&action, self);

        // Apply the effects to update state
        let messages = self.apply_effects(&effects)?;

        // Record the action and effects to the event log
        let entry = LogEntry { action, effects };
        self.event_log.record_entry(db, entry);

        Ok(messages)
    }

    /// Apply effects to update the process state
    fn apply_effects(
        &mut self,
        effects: &[Effect<Tr>],
    ) -> Result<Vec<(Message<Tr>, Option<Identity>)>, String> {
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

                    // Transition view cache to new view
                    self.view_cache.transition_to_view(*new_view);
                    self.view_cache.load_from_bulk(&self.bulk_store)?;

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
                    self.record_block(block)?;
                }
                Effect::QcRecorded {
                    qc,
                    finalized_blocks: _,
                } => {
                    self.record_qc(qc.clone())?;
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
                    // Vote recording is handled internally
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

        // Check storage invariants if configured
        self.check_and_log_invariants();

        Ok(messages)
    }

    /// Check storage invariants if configured and log any violations
    fn check_and_log_invariants(&self) {
        if let Some(ref checker) = self.invariant_checker {
            // Build consensus state for checking
            let consensus_state = ConsensusState {
                current_view: self.view_manager.current_view(),
                current_phase: self.view_manager.phase(self.view_manager.current_view()),
                view_entry_time: self.view_manager.view_entry_time,
                tips: self.tips_refs.iter().cloned().collect(),
                max_1qc: self.max_1qc_ref.clone(),
                finalized_blocks: self.finalized_blocks.clone(),
                unfinalized_qcs: self.unfinalized_qcs.clone(),
                leader_blocks_by_view: im::HashMap::new(), // TODO: Implement if needed
                unfinalized_leader_by_view: im::HashMap::new(), // TODO: Implement if needed
            };

            let violations = checker.check_invariants(
                &self.bulk_store,
                &self.snapshot_store,
                &self.view_cache,
                &consensus_state,
            );

            log_invariant_violations(&violations, &self.id);
        }
    }

    fn record_block(&mut self, block: &Arc<Signed<Block<Tr>>>) -> Result<(), String> {
        // Store block in bulk storage
        let block_ref = self.bulk_store.append_block(block.clone())?;

        // Update lightweight DAG
        self.lightweight_dag.insert_block_ref(block_ref.clone());
        self.lightweight_dag
            .update_relationships(&block.data, &self.bulk_store)?;

        // Add to view cache if in current view
        if block.data.key.view == self.view_manager.current_view() {
            self.view_cache
                .insert_block(block.clone(), block_ref.clone());
        }

        // Update vote manager
        self.vote_manager
            .mark_pending_votes_dirty(block.data.key.view);

        // Track in pending votes
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

        Ok(())
    }

    fn record_qc(&mut self, qc: FinishedQC) -> Result<(), String> {
        // Store QC in bulk storage
        let qc_ref = self.bulk_store.append_qc(qc.clone())?;

        // Update tips
        if qc.data.z == 2 {
            // Remove finalized blocks from tips
            self.tips_refs
                .retain(|tip| tip.vote_data.for_which != qc.data.for_which);

            // Mark block as finalized
            let block_ref = BlockRef {
                key: qc.data.for_which.clone(),
                hash: qc.data.for_which.hash.clone(),
            };
            self.finalized_blocks.insert(block_ref.clone());

            // Remove from unfinalized
            self.unfinalized_qcs.remove(&block_ref);
        } else if qc.data.z == 1 {
            // Add to tips if it's a 1-QC
            self.tips_refs.push_back(qc_ref.clone());

            // Track unfinalized
            let block_ref = BlockRef {
                key: qc.data.for_which.clone(),
                hash: qc.data.for_which.hash.clone(),
            };
            self.unfinalized_qcs
                .entry(block_ref)
                .or_default()
                .insert(qc_ref.clone());
        }

        // Update max 1-QC if needed
        if qc.data.z == 1
            && qc.data.compare_qc(&self.get_max_1qc()?.data) == std::cmp::Ordering::Greater
        {
            self.max_1qc_ref = qc_ref.clone();
            self.cached_max_1qc = Some(qc.clone()); // Update cache
        }

        // Add to view cache if in current view
        if qc.data.for_which.view == self.view_manager.current_view() {
            self.view_cache.insert_qc(qc.clone(), qc_ref);
        }

        // Track 2-votes
        if qc.data.z == 1 {
            self.vote_manager
                .mark_pending_votes_dirty(qc.data.for_which.view);

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

        // Update cached tips after any changes
        self.cached_tips = self.get_tips()?;

        Ok(())
    }

    /// Get max 1-QC from storage
    fn get_max_1qc(&self) -> Result<FinishedQC, String> {
        // First check cache - note: can't update cache in immutable method
        if let Some(ref qc) = self.cached_max_1qc {
            return Ok(qc.clone());
        }

        // Load from bulk storage
        self.bulk_store
            .get_qc(&self.max_1qc_ref)?
            .ok_or_else(|| "Max 1-QC not found in storage".to_string())
    }

    /// Get tips from storage
    fn get_tips(&self) -> Result<Vec<FinishedQC>, String> {
        // First check cache - note: can't update cache in immutable method
        if !self.cached_tips.is_empty() {
            return Ok(self.cached_tips.clone());
        }

        let mut tips = Vec::new();
        for tip_ref in &self.tips_refs {
            // Check cache first
            if let Some(qc) = self.view_cache.get_qc(&tip_ref.vote_data) {
                tips.push(qc.clone());
            } else {
                // Load from bulk storage
                let qc = self
                    .bulk_store
                    .get_qc(tip_ref)?
                    .ok_or_else(|| format!("Tip QC not found: {:?}", tip_ref))?;
                tips.push(qc);
            }
        }
        Ok(tips)
    }
}

// Implement ProcessState trait for MorpheusProcess
impl<Tr: Transaction, B: BulkStore<Tr>, S: SnapshotStore> ProcessState<Tr>
    for MorpheusProcess<Tr, B, S>
{
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
        // Return cached value if available, otherwise panic with helpful message
        self.cached_max_1qc.as_ref().unwrap_or_else(|| {
            panic!("max_1qc cache not initialized - this is a bug in the storage architecture")
        })
    }

    fn has_qc(&self, qc: &FinishedQC) -> bool {
        // Check cache first
        if self.view_cache.get_qc(&qc.data).is_some() {
            return true;
        }

        // Check if we have a reference to this QC
        self.bulk_store
            .get_qc(&QCRef {
                vote_data: qc.data.clone(),
                hash: None,
            })
            .unwrap_or(None)
            .is_some()
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
        // This is challenging with the storage architecture
        // We would need to load all unfinalized QCs from storage
        // For now, return None which means no unfinalized blocks
        None
    }

    fn has_complained(&self, qc: &FinishedQC) -> bool {
        self.view_manager.complained_qcs.contains(qc)
    }

    fn has_unfinalized(&self) -> bool {
        !self.unfinalized_qcs.is_empty()
    }

    fn can_produce_tr_block(&self) -> bool {
        let has_transactions = self.block_producer.has_ready_transactions();
        let slot = self.block_producer.current_tr_slot();

        if !slot.is_zero() {
            // Check if we have the previous slot QC
            // This is expensive with storage architecture
            false // TODO: Implement properly
        } else {
            has_transactions
        }
    }

    fn can_produce_lead_block(&self) -> bool {
        let view = self.current_view();
        let _slot = self.block_producer.current_lead_slot();
        let has_produced = self.block_producer.has_produced_lead_in_view(view);

        if has_produced {
            false // TODO: Check for previous QC
        } else {
            self.view_manager.has_enough_start_views(view, self.f)
        }
    }

    fn tips_count(&self) -> usize {
        self.tips_refs.len()
    }

    // Additional methods
    fn genesis_qc(&self) -> &FinishedQC {
        &self.genesis_qc
    }

    fn tips(&self) -> &Vec<FinishedQC> {
        &self.cached_tips
    }

    fn current_tr_slot(&self) -> SlotNum {
        self.block_producer.current_tr_slot()
    }

    fn current_lead_slot(&self) -> SlotNum {
        self.block_producer.current_lead_slot()
    }

    fn latest_tr_qc(&self) -> Option<&FinishedQC> {
        panic!("ProcessState::latest_tr_qc() not implemented for storage architecture")
    }

    fn latest_leader_qc(&self) -> Option<&FinishedQC> {
        panic!("ProcessState::latest_leader_qc() not implemented for storage architecture")
    }

    fn latest_leader_1qc(&self) -> Option<&FinishedQC> {
        panic!("ProcessState::latest_leader_1qc() not implemented for storage architecture")
    }

    fn take_ready_transactions(&self) -> Vec<Tr> {
        self.block_producer.ready_transactions.clone()
    }

    fn has_produced_lead_in_view(&self, view: ViewNum) -> bool {
        self.block_producer.has_produced_lead_in_view(view)
    }

    fn get_start_views(&self, view: ViewNum) -> Option<&Vec<Arc<Signed<StartView>>>> {
        self.view_manager.get_start_views(view)
    }

    // Voting eligibility implementations
    fn is_eligible_for_tr_1_vote(&self, _block_key: &BlockKey) -> bool {
        // Check if block is single tip and has valid 1-QC
        // This requires loading data from storage which is expensive
        false // TODO: Implement
    }

    fn is_eligible_for_tr_2_vote(&self, block_key: &BlockKey) -> bool {
        // Check if QC is single tip and no higher blocks exist
        let has_single_tip = self.tips_refs.len() == 1
            && self.tips_refs.get(0).is_some_and(|tip| {
                tip.vote_data.z == 1 && tip.vote_data.for_which.eq(block_key)
            });

        let no_higher_blocks = self.lightweight_dag.max_height <= block_key.height;

        has_single_tip && no_higher_blocks
    }

    fn block_is_single_tip(&self, block_key: &BlockKey) -> bool {
        if self.tips_refs.len() != 1 {
            return false;
        }

        // Check if the single tip points to this block
        self.tips_refs.get(0).is_some_and(|tip| {
            self.lightweight_dag
                .block_pointed_by
                .get(&tip.vote_data.for_which)
                .is_some_and(|parents| {
                    parents.len() == 1 && parents.contains(block_key)
                })
        })
    }

    fn contains_lead_in_view(&self, _view: ViewNum) -> bool {
        // TODO: Track this in ConsensusState
        false
    }

    fn has_unfinalized_lead_in_view(&self, _view: ViewNum) -> bool {
        // TODO: Track this in ConsensusState
        false
    }

    fn get_block(&self, key: &BlockKey) -> Option<&Arc<Signed<Block<Tr>>>> {
        self.view_cache.get_block(key)
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
        // Only return blocks in view cache
        self.view_cache
            .blocks()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    fn get_all_qcs(&self) -> Vec<FinishedQC> {
        // Only return QCs in view cache
        self.view_cache
            .qcs()
            .iter()
            .map(|(_, v)| v.clone())
            .collect()
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
