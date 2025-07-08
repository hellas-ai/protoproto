//! Unified process state - the single source of truth
//! 
//! This module defines the comprehensive ProcessState that consolidates
//! all protocol state into one coherent structure, eliminating the
//! fragmentation issues of the previous architecture.

use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// The complete state of a Morpheus process
/// 
/// This is the single source of truth for all protocol state.
/// All logic functions operate on immutable references to this state,
/// and all mutations go through a controlled apply method.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessState<Tr: Transaction> {
    // === Core Protocol State ===
    
    /// Current view number
    pub current_view: ViewNum,
    
    /// Current phase within the view
    pub current_phase: Phase,
    
    /// Time when entered current view
    pub view_entry_time: u128,
    
    /// Current logical time
    pub current_time: u128,
    
    // === Block Production State ===
    
    /// Current slots for block production
    pub slot_lead: SlotNum,
    pub slot_tr: SlotNum,
    
    /// Ready transactions for next block
    #[serde(bound(serialize = "Tr: Transaction", deserialize = "Tr: Transaction"))]
    pub ready_transactions: Vec<Tr>,
    
    /// Tracks if we produced a leader block in each view
    pub produced_lead_in_view: BTreeMap<ViewNum, bool>,
    
    // === DAG State (formerly StateIndex) ===
    
    /// All blocks by key
    #[serde(bound(serialize = "Tr: Transaction", deserialize = "Tr: Transaction"))]
    pub blocks: BTreeMap<BlockKey, Arc<Signed<Block<Tr>>>>,
    
    /// All QCs
    pub qcs: BTreeSet<FinishedQC>,
    
    /// QCs indexed by vote data
    pub qcs_by_vote: BTreeMap<VoteData, FinishedQC>,
    
    /// Current tips of the DAG
    pub tips: Vec<FinishedQC>,
    
    /// Parent-child relationships
    pub block_pointed_by: BTreeMap<BlockKey, BTreeSet<BlockKey>>,
    
    /// Maximum values tracking
    pub max_view: (ViewNum, FinishedQC),
    pub max_height: (u64, BlockKey),
    pub max_1qc: FinishedQC,
    
    /// Latest QCs for block production
    pub latest_leader_1qc: Option<FinishedQC>,
    pub latest_leader_qc: Option<FinishedQC>,
    pub latest_tr_qc: Option<FinishedQC>,
    
    /// Finalization tracking
    pub finalized: BTreeSet<BlockKey>,
    pub unfinalized_qcs: BTreeMap<BlockKey, BTreeSet<FinishedQC>>,
    pub unfinalized_2qc: BTreeSet<FinishedQC>,
    
    /// View-based tracking
    pub contains_lead_by_view: BTreeMap<ViewNum, bool>,
    pub unfinalized_lead_by_view: BTreeMap<ViewNum, BTreeSet<BlockKey>>,
    
    // === Voting State ===
    
    /// Tracks votes for quorum formation
    pub vote_tracker: BTreeMap<VoteData, BTreeMap<Identity, Arc<ThreshPartial<VoteData>>>>,
    
    /// Tracks which blocks we've voted for
    pub voted: BTreeSet<(u8, BlockType, SlotNum, Identity)>,
    
    /// Tracks 0-QCs sent
    pub zero_qcs_sent: BTreeSet<BlockKey>,
    
    /// End-view messages
    pub end_views: BTreeMap<ViewNum, BTreeMap<Identity, Arc<ThreshPartial<ViewNum>>>>,
    
    /// Pending votes by view
    pub pending_votes: BTreeMap<ViewNum, PendingVotes>,
    
    // === View Management State ===
    
    /// Phase tracking by view
    pub phase_by_view: BTreeMap<ViewNum, Phase>,
    
    /// Complained QCs
    pub complained_qcs: BTreeSet<FinishedQC>,
    
    /// Start view messages
    pub start_views: BTreeMap<ViewNum, Vec<Arc<Signed<StartView>>>>,
    
    // === Genesis References ===
    #[serde(bound(serialize = "Tr: Transaction", deserialize = "Tr: Transaction"))]
    pub genesis_block: Arc<Signed<Block<Tr>>>,
    pub genesis_qc: FinishedQC,
}

/// Voting state that needs re-evaluation
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PendingVotes {
    pub tr_1: BTreeMap<BlockKey, bool>,
    pub tr_2: BTreeMap<BlockKey, bool>,
    pub lead_1: BTreeMap<BlockKey, bool>,
    pub lead_2: BTreeMap<BlockKey, bool>,
    pub dirty: bool,
}

impl<Tr: Transaction> ProcessState<Tr> {
    /// Create a new process state with genesis
    pub fn new(
        genesis_block: Arc<Signed<Block<Tr>>>,
        genesis_qc: FinishedQC,
    ) -> Self {
        let mut blocks = BTreeMap::new();
        blocks.insert(GEN_BLOCK_KEY, genesis_block.clone());
        
        let mut qcs = BTreeSet::new();
        qcs.insert(genesis_qc.clone());
        
        let mut qcs_by_vote = BTreeMap::new();
        qcs_by_vote.insert(genesis_qc.data.clone(), genesis_qc.clone());
        
        let mut phase_by_view = BTreeMap::new();
        phase_by_view.insert(ViewNum(0), Phase::High);
        
        let mut produced_lead_in_view = BTreeMap::new();
        produced_lead_in_view.insert(ViewNum(0), false);
        
        Self {
            current_view: ViewNum(0),
            current_phase: Phase::High,
            view_entry_time: 0,
            current_time: 0,
            slot_lead: SlotNum(0),
            slot_tr: SlotNum(0),
            ready_transactions: Vec::new(),
            produced_lead_in_view,
            blocks,
            qcs,
            qcs_by_vote,
            tips: vec![genesis_qc.clone()],
            block_pointed_by: BTreeMap::new(),
            max_view: (ViewNum(-1), genesis_qc.clone()),
            max_height: (0, GEN_BLOCK_KEY),
            max_1qc: genesis_qc.clone(),
            latest_leader_1qc: None,
            latest_leader_qc: None,
            latest_tr_qc: None,
            finalized: BTreeSet::from([GEN_BLOCK_KEY]),
            unfinalized_qcs: BTreeMap::new(),
            unfinalized_2qc: BTreeSet::new(),
            contains_lead_by_view: BTreeMap::new(),
            unfinalized_lead_by_view: BTreeMap::new(),
            vote_tracker: BTreeMap::new(),
            voted: BTreeSet::new(),
            zero_qcs_sent: BTreeSet::new(),
            end_views: BTreeMap::new(),
            pending_votes: BTreeMap::new(),
            phase_by_view,
            complained_qcs: BTreeSet::new(),
            start_views: BTreeMap::new(),
            genesis_block,
            genesis_qc,
        }
    }
    
    /// Apply an effect to mutate the state
    /// This is the ONLY way state should be mutated
    pub fn apply<N, F>(&mut self, effect: &Effect<Tr>, id: &Identity, n: N, f: F)
    where
        N: Into<u32>,
        F: Into<u32>,
    {
        let n = n.into();
        let f = f.into();
        
        match effect {
            Effect::TimeUpdated(time) => {
                self.current_time = *time;
            }
            
            Effect::ViewChanged { old_view: _, new_view, cause: _ } => {
                self.current_view = *new_view;
                self.view_entry_time = self.current_time;
                self.current_phase = Phase::High;
                self.phase_by_view.insert(*new_view, Phase::High);
                self.pending_votes.entry(*new_view).or_default().dirty = true;
            }
            
            Effect::PhaseChanged { view, old_phase: _, new_phase } => {
                if *view == self.current_view {
                    self.current_phase = *new_phase;
                }
                self.phase_by_view.insert(*view, *new_phase);
            }
            
            Effect::BlockRecorded { block } => {
                self.insert_block(block);
            }
            
            Effect::QcRecorded { qc, finalized_blocks: _ } => {
                self.insert_qc(qc, id, n, f);
            }
            
            Effect::VoteSent { vote_type, block_key, target: _ } => {
                if let Some(author) = &block_key.author {
                    self.voted.insert((*vote_type, block_key.type_, block_key.slot, author.clone()));
                }
            }
            
            Effect::VoteRecorded { voter, vote_data } => {
                let votes = self.vote_tracker.entry(vote_data.clone()).or_default();
                // In a real implementation, we'd store the actual vote here
                votes.insert(voter.clone(), Arc::new(ThreshPartial {
                    data: vote_data.clone(),
                    author: voter.clone(),
                    signature: hints::PartialSignature::default(),
                }));
            }
            
            Effect::TransactionsUpdated { transactions } => {
                self.ready_transactions = transactions.clone();
            }
            
            Effect::SlotAdvanced { slot_type, new_slot } => {
                match slot_type {
                    BlockType::Lead => self.slot_lead = *new_slot,
                    BlockType::Tr => self.slot_tr = *new_slot,
                    _ => {}
                }
            }
            
            Effect::LeaderBlockProducedInView { view } => {
                self.produced_lead_in_view.insert(*view, true);
            }
            
            Effect::ComplaintSent { qc, target: _ } => {
                self.complained_qcs.insert(qc.clone());
            }
            
            _ => {} // Other effects don't mutate state directly
        }
    }
    
    /// Insert a block into the state
    fn insert_block(&mut self, block: &Arc<Signed<Block<Tr>>>) {
        let key = block.data.key.clone();
        
        if self.blocks.contains_key(&key) {
            return;
        }
        
        // Update height tracking
        if key.height > self.max_height.0 {
            self.max_height = (key.height, key.clone());
        }
        
        // Insert the block
        self.blocks.insert(key.clone(), block.clone());
        
        // Update parent-child relationships
        for qc in &block.data.prev {
            self.block_pointed_by
                .entry(qc.data.for_which.clone())
                .or_default()
                .insert(key.clone());
        }
        
        // Update view-based tracking
        if key.type_ == BlockType::Lead {
            self.contains_lead_by_view.insert(key.view, true);
            self.unfinalized_lead_by_view
                .entry(key.view)
                .or_default()
                .insert(key.clone());
        }
        
        // Update pending votes
        self.pending_votes.entry(key.view).or_default().dirty = true;
        let pending = self.pending_votes.get_mut(&key.view).unwrap();
        match key.type_ {
            BlockType::Lead => { pending.lead_1.insert(key.clone(), true); }
            BlockType::Tr => { pending.tr_1.insert(key.clone(), true); }
            _ => {}
        }
    }
    
    /// Insert a QC and update all related state
    fn insert_qc<N, F>(&mut self, qc: &FinishedQC, process_id: &Identity, n: N, f: F)
    where
        N: Into<u32>,
        F: Into<u32>,
    {
        if !self.qcs.insert(qc.clone()) {
            return;
        }
        
        // Index by vote data
        self.qcs_by_vote.insert(qc.data.clone(), qc.clone());
        
        // Update max view
        if qc.data.for_which.view > self.max_view.0 {
            self.max_view = (qc.data.for_which.view, qc.clone());
        }
        
        // Update max 1-QC
        if qc.data.z == 1 && qc.data.compare_qc(&self.max_1qc.data) == std::cmp::Ordering::Greater {
            self.max_1qc = qc.clone();
        }
        
        // CRITICAL FIX: Update latest QCs for the process
        if let Some(author) = &qc.data.for_which.author {
            if author == process_id {
                match qc.data.for_which.type_ {
                    BlockType::Lead => {
                        if qc.data.for_which.slot.is_pred(self.slot_lead) {
                            self.latest_leader_qc = Some(qc.clone());
                            if qc.data.z == 1 {
                                self.latest_leader_1qc = Some(qc.clone());
                            }
                        }
                    }
                    BlockType::Tr => {
                        if qc.data.for_which.slot.is_pred(self.slot_tr) {
                            self.latest_tr_qc = Some(qc.clone());
                        }
                    }
                    _ => {}
                }
            }
        }
        
        // Track unfinalized
        self.unfinalized_qcs
            .entry(qc.data.for_which.clone())
            .or_default()
            .insert(qc.clone());
        
        if qc.data.z == 2 {
            self.unfinalized_2qc.insert(qc.clone());
        }
        
        // Update tips
        self.update_tips_for_qc(qc, n.into(), f.into());
        
        // Check for finalization
        self.check_finalization(qc);
        
        // Update pending votes
        if qc.data.z == 1 {
            let pending = self.pending_votes.entry(qc.data.for_which.view).or_default();
            pending.dirty = true;
            match qc.data.for_which.type_ {
                BlockType::Lead => { pending.lead_2.insert(qc.data.for_which.clone(), true); }
                BlockType::Tr => { pending.tr_2.insert(qc.data.for_which.clone(), true); }
                _ => {}
            }
        }
    }
    
    /// Update tips when a new QC is added
    fn update_tips_for_qc(&mut self, qc: &FinishedQC, _n: u32, _f: u32) {
        let mut tips_to_remove = BTreeSet::new();
        
        // Check if the new QC observes any existing tips
        for tip in &self.tips {
            if self.observes(&qc.data, &tip.data) {
                tips_to_remove.insert(tip.clone());
            }
        }
        
        if !tips_to_remove.is_empty() {
            self.tips.retain(|tip| !tips_to_remove.contains(tip));
            self.tips.push(qc.clone());
        } else {
            // Check if any existing tip observes the new QC
            let observed_by_existing = self.tips.iter().any(|tip| self.observes(&tip.data, &qc.data));
            
            if !observed_by_existing {
                self.tips.push(qc.clone());
            }
        }
    }
    
    /// Check if blocks should be finalized
    fn check_finalization(&mut self, qc: &FinishedQC) {
        let mut finalized_here = BTreeSet::new();
        
        for unfinalized_2qc in &self.unfinalized_2qc {
            if self.observes(&qc.data, &unfinalized_2qc.data) {
                finalized_here.insert(unfinalized_2qc.clone());
            }
        }
        
        self.unfinalized_2qc.retain(|unfinalized_2qc| !finalized_here.contains(unfinalized_2qc));
        
        for finalized_qc in finalized_here {
            let block_key = &finalized_qc.data.for_which;
            self.unfinalized_lead_by_view
                .entry(block_key.view)
                .or_default()
                .remove(block_key);
            self.unfinalized_qcs.remove(block_key);
            self.finalized.insert(block_key.clone());
        }
    }
    
    /// Check observes relation
    pub fn observes(&self, root: &VoteData, needle: &VoteData) -> bool {
        use std::collections::VecDeque;
        
        let mut to_visit: VecDeque<VoteData> = vec![root.clone()].into();
        while let Some(node) = to_visit.pop_front() {
            if self.directly_observes(&node, needle) {
                return true;
            }
            if let Some(block) = self.blocks.get(&node.for_which) {
                for prev in &block.data.prev {
                    to_visit.push_back(prev.data.clone());
                }
            }
        }
        false
    }
    
    /// Direct observation check
    fn directly_observes(&self, looks: &VoteData, seen: &VoteData) -> bool {
        if looks.for_which.type_ == seen.for_which.type_
            && looks.for_which.author == seen.for_which.author
            && looks.for_which.slot > seen.for_which.slot
        {
            return true;
        }
        if looks.for_which.type_ == seen.for_which.type_
            && looks.for_which.author == seen.for_which.author
            && looks.for_which.slot == seen.for_which.slot
            && looks.z >= seen.z
        {
            return true;
        }
        if let Some(block) = self.blocks.get(&looks.for_which) {
            if block.data.prev.iter().any(|prev| prev.data.for_which == seen.for_which) {
                return true;
            }
        }
        false
    }
} 