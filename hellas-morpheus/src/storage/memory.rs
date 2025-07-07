//! Memory-based cache for current view data

use crate::storage::{BlockRef, BulkStore, QCRef, VoteRef};
use crate::*;
use im::HashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Memory cache for current view data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewCache<Tr: Transaction> {
    /// Current view
    pub current_view: ViewNum,

    /// Blocks in current view (cached from bulk storage)
    #[serde(bound(serialize = "Tr: Transaction", deserialize = "Tr: Transaction"))]
    pub blocks: HashMap<BlockKey, Arc<Signed<Block<Tr>>>>,

    /// QCs in current view (cached from bulk storage)
    pub qcs: HashMap<VoteData, FinishedQC>,

    /// Votes in current view (cached from bulk storage)
    #[serde(bound(serialize = "Tr: Transaction", deserialize = "Tr: Transaction"))]
    pub votes: HashMap<VoteData, HashMap<Identity, Arc<ThreshPartial<VoteData>>>>,

    /// References to blocks in bulk storage
    pub block_refs: HashMap<BlockKey, BlockRef>,

    /// References to QCs in bulk storage
    pub qc_refs: HashMap<VoteData, QCRef>,

    /// References to votes in bulk storage
    pub vote_refs: HashMap<(Identity, VoteData), VoteRef>,
}

impl<Tr: Transaction> ViewCache<Tr> {
    /// Create a new empty view cache
    pub fn new(view: ViewNum) -> Self {
        Self {
            current_view: view,
            blocks: HashMap::new(),
            qcs: HashMap::new(),
            votes: HashMap::new(),
            block_refs: HashMap::new(),
            qc_refs: HashMap::new(),
            vote_refs: HashMap::new(),
        }
    }

    /// Load view data from bulk storage
    pub fn load_from_bulk<S: BulkStore<Tr>>(&mut self, bulk_store: &S) -> Result<(), String> {
        // Load blocks for current view
        let block_refs = bulk_store.get_blocks_in_view(self.current_view)?;
        for block_ref in block_refs {
            if let Some(block) = bulk_store.get_block(&block_ref)? {
                self.blocks.insert(block.data.key.clone(), block.clone());
                self.block_refs.insert(block.data.key.clone(), block_ref);
            }
        }

        // Load QCs for current view
        let qc_refs = bulk_store.get_qcs_in_view(self.current_view)?;
        for qc_ref in qc_refs {
            if let Some(qc) = bulk_store.get_qc(&qc_ref)? {
                self.qcs.insert(qc.data.clone(), qc.clone());
                self.qc_refs.insert(qc.data.clone(), qc_ref);
            }
        }

        Ok(())
    }

    /// Get a block from cache
    pub fn get_block(&self, key: &BlockKey) -> Option<&Arc<Signed<Block<Tr>>>> {
        self.blocks.get(key)
    }

    /// Get a QC from cache
    pub fn get_qc(&self, vote_data: &VoteData) -> Option<&FinishedQC> {
        self.qcs.get(vote_data)
    }

    /// Get a vote from cache
    pub fn get_vote(
        &self,
        voter: &Identity,
        vote_data: &VoteData,
    ) -> Option<&Arc<ThreshPartial<VoteData>>> {
        self.votes.get(vote_data).and_then(|votes| votes.get(voter))
    }

    /// Add a block to cache
    pub fn insert_block(&mut self, block: Arc<Signed<Block<Tr>>>, block_ref: BlockRef) {
        let key = block.data.key.clone();
        self.blocks.insert(key.clone(), block);
        self.block_refs.insert(key, block_ref);
    }

    /// Add a QC to cache
    pub fn insert_qc(&mut self, qc: FinishedQC, qc_ref: QCRef) {
        let vote_data = qc.data.clone();
        self.qcs.insert(vote_data.clone(), qc);
        self.qc_refs.insert(vote_data, qc_ref);
    }

    /// Add a vote to cache
    pub fn insert_vote(&mut self, vote: Arc<ThreshPartial<VoteData>>, vote_ref: VoteRef) {
        let _key = (vote.author.clone(), vote.data.clone());
        self.votes
            .entry(vote.data.clone())
            .or_default()
            .insert(vote.author.clone(), vote);
        self.vote_refs.insert(
            (vote_ref.voter.clone(), vote_ref.vote_data.clone()),
            vote_ref,
        );
    }

    /// Clear cache when transitioning to a new view
    pub fn transition_to_view(&mut self, new_view: ViewNum) {
        self.current_view = new_view;
        self.blocks.clear();
        self.qcs.clear();
        self.votes.clear();
        self.block_refs.clear();
        self.qc_refs.clear();
        self.vote_refs.clear();
    }

    /// Get all blocks in cache
    pub fn blocks(&self) -> &HashMap<BlockKey, Arc<Signed<Block<Tr>>>> {
        &self.blocks
    }

    /// Get all QCs in cache
    pub fn qcs(&self) -> &HashMap<VoteData, FinishedQC> {
        &self.qcs
    }

    /// Get all block references
    pub fn block_refs(&self) -> &HashMap<BlockKey, BlockRef> {
        &self.block_refs
    }

    /// Get all QC references
    pub fn qc_refs(&self) -> &HashMap<VoteData, QCRef> {
        &self.qc_refs
    }
}

/// Lightweight DAG index using references
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightweightDAGIndex {
    /// Block references by key
    pub block_refs: im::HashMap<BlockKey, BlockRef>,

    /// Parent-child relationships
    pub block_points_to: im::HashMap<BlockKey, im::HashSet<BlockKey>>,
    pub block_pointed_by: im::HashMap<BlockKey, im::HashSet<BlockKey>>,

    /// Tips of the DAG
    pub tips: im::Vector<QCRef>,

    /// Maximum height seen
    pub max_height: u64,
}

impl Default for LightweightDAGIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl LightweightDAGIndex {
    pub fn new() -> Self {
        Self {
            block_refs: im::HashMap::new(),
            block_points_to: im::HashMap::new(),
            block_pointed_by: im::HashMap::new(),
            tips: im::Vector::new(),
            max_height: 0,
        }
    }

    /// Insert a block reference
    pub fn insert_block_ref(&mut self, block_ref: BlockRef) {
        self.block_refs
            .insert(block_ref.key.clone(), block_ref.clone());
        self.max_height = self.max_height.max(block_ref.key.height);
    }

    /// Update relationships based on a block (requires loading from bulk storage)
    pub fn update_relationships<Tr: Transaction, S: BulkStore<Tr>>(
        &mut self,
        block: &Block<Tr>,
        bulk_store: &S,
    ) -> Result<(), String> {
        let block_key = block.key.clone();

        // Update parent-child relationships
        for prev_qc in &block.prev {
            let parent_key = &prev_qc.data.for_which;

            self.block_points_to
                .entry(block_key.clone())
                .or_default()
                .insert(parent_key.clone());

            self.block_pointed_by
                .entry(parent_key.clone())
                .or_default()
                .insert(block_key.clone());
        }

        // Also add one-QC parent
        let one_parent = &block.one.data.for_which;
        self.block_points_to
            .entry(block_key.clone())
            .or_default()
            .insert(one_parent.clone());

        self.block_pointed_by
            .entry(one_parent.clone())
            .or_default()
            .insert(block_key.clone());

        Ok(())
    }
}
