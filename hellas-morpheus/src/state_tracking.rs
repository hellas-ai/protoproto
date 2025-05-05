use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use serde::{Deserialize, Serialize};

use crate::*;

/// Tracks votes pending for reevaluation in each view
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct PendingVotes {
    /// Transaction block 1-votes pending evaluation
    pub tr_1: BTreeMap<BlockKey, bool>,
    
    /// Transaction block 2-votes pending evaluation
    pub tr_2: BTreeMap<BlockKey, bool>,
    
    /// Leader block 1-votes pending evaluation
    pub lead_1: BTreeMap<BlockKey, bool>,
    
    /// Leader block 2-votes pending evaluation
    pub lead_2: BTreeMap<BlockKey, bool>,
    
    /// Flag indicating votes need reevaluation
    pub dirty: bool,
}

/// Manages the block DAG structure and QC tracking
/// 
/// StateIndex is responsible for tracking all blocks, QCs, and their relationships
/// in the DAG. It maintains multiple indices for efficient lookup and querying.
#[derive(Clone, Serialize, Deserialize)]
pub struct StateIndex {
    /// Stores QCs indexed by their VoteData
    /// Part of Q_i in pseudocode - "stores at most one z-QC for each block"
    #[serde(with = "serde_json_any_key::any_key_map")]
    pub qcs: BTreeMap<VoteData, Arc<ThreshSigned<VoteData>>>,

    /// Stores all 1-QCs seen by this process
    pub all_1qc: BTreeSet<Arc<ThreshSigned<VoteData>>>,

    /// Stores the current tips of the block DAG
    /// "The tips of Q_i are those q ∈ Q_i such that there does not exist q' ∈ Q_i with q' ≻ q"
    pub tips: Vec<VoteData>,

    /// Maps block keys to blocks (part of M_i in pseudocode)
    /// Implements part of "the set of all received messages"
    #[serde(with = "serde_json_any_key::any_key_map")]
    pub blocks: BTreeMap<BlockKey, Arc<Signed<Block>>>,

    // === Performance optimization indexes ===
    /// Tracks which blocks point to which other blocks
    /// Used to efficiently determine the DAG structure
    #[serde(with = "serde_json_any_key::any_key_map")]
    pub block_pointed_by: BTreeMap<BlockKey, BTreeSet<BlockKey>>,

    /// Tracks the maximum view seen and its associated VoteData
    pub max_view: (ViewNum, VoteData),

    /// Tracks the maximum height block seen and its key
    /// Used for identifying the tallest block in the DAG
    pub max_height: (usize, BlockKey),

    /// Stores the maximum 1-QC seen by this process
    /// Used when entering a new view: "Send (v, q') signed by p_i to lead(v),
    /// where q' is a maximal amongst 1-QCs seen by p_i"
    pub max_1qc: Arc<ThreshSigned<VoteData>>,

    /// Tracks unfinalized blocks with 2-QC
    /// Used to identify blocks that have 2-QC but are not yet finalized
    pub unfinalized_2qc: BTreeSet<VoteData>,

    /// Maps block keys to their finalization status
    /// Used to track which blocks have been finalized
    #[serde(with = "serde_json_any_key::any_key_map")]
    pub finalized: BTreeMap<BlockKey, bool>,

    /// Maps block keys to their unfinalized QCs
    /// Used to track which QCs are not yet finalized
    #[serde(with = "serde_json_any_key::any_key_map")]
    pub unfinalized: BTreeMap<BlockKey, BTreeSet<VoteData>>,

    /// Tracks whether we've seen a leader block for each view
    /// Used to implement logic that depends on leader blocks within a view
    #[serde(with = "serde_json_any_key::any_key_map")]
    pub contains_lead_by_view: BTreeMap<ViewNum, bool>,

    /// Maps views to sets of unfinalized leader blocks
    /// Tracks which leader blocks are not yet finalized by view
    #[serde(with = "serde_json_any_key::any_key_map")]
    pub unfinalized_lead_by_view: BTreeMap<ViewNum, BTreeSet<BlockKey>>,

    /// Quick index to QCs by block type, author, and slot
    /// Enables O(1) lookup of QCs
    #[serde(with = "serde_json_any_key::any_key_map")]
    pub qc_by_slot: BTreeMap<(BlockType, Identity, SlotNum), Arc<ThreshSigned<VoteData>>>,

    /// Maps (type, author, view) to QCs for efficient retrieval
    /// Used to find QCs for a specific block type, author, and view
    #[serde(with = "serde_json_any_key::any_key_map")]
    pub qc_by_view: BTreeMap<(BlockType, Identity, ViewNum), Vec<Arc<ThreshSigned<VoteData>>>>,

    /// Maps (type, view, author) to blocks for efficient retrieval
    /// Used to find blocks of a specific type, view, and author
    #[serde(with = "serde_json_any_key::any_key_map")]
    pub block_index: BTreeMap<(BlockType, ViewNum, Identity), Vec<Arc<Signed<Block>>>>,
}

impl StateIndex {
    /// Creates a new StateIndex with the genesis block and QC
    pub fn new(genesis_qc: Arc<ThreshSigned<VoteData>>, genesis_block: Arc<Signed<Block>>) -> Self {
        Self {
            qcs: {
                let mut map = BTreeMap::new();
                map.insert(genesis_qc.data.clone(), genesis_qc.clone());
                map
            },
            max_view: (ViewNum(-1), genesis_qc.data.clone()),
            max_height: (0, GEN_BLOCK_KEY),
            max_1qc: genesis_qc.clone(),
            all_1qc: BTreeSet::new(),
            tips: vec![genesis_qc.data.clone()],
            blocks: {
                let mut map = BTreeMap::new();
                map.insert(GEN_BLOCK_KEY, genesis_block.clone());
                map
            },
            block_pointed_by: BTreeMap::new(),
            unfinalized_2qc: BTreeSet::new(),
            finalized: {
                let mut map = BTreeMap::new();
                map.insert(GEN_BLOCK_KEY, true);
                map
            },
            unfinalized: BTreeMap::new(),
            contains_lead_by_view: BTreeMap::new(),
            unfinalized_lead_by_view: BTreeMap::new(),

            qc_by_slot: BTreeMap::from([(
                (BlockType::Genesis, Identity(u64::MAX), SlotNum(0)),
                genesis_qc.clone(),
            )]),
            qc_by_view: BTreeMap::from([(
                (BlockType::Genesis, Identity(u64::MAX), ViewNum(-1)),
                vec![genesis_qc.clone()],
            )]),
            block_index: {
                let mut map = BTreeMap::new();
                map.insert(
                    (BlockType::Genesis, ViewNum(-1), Identity(u64::MAX)),
                    vec![genesis_block.clone()],
                );
                map
            },
        }
    }
}

/// State tracking functionality for the MorpheusProcess
impl MorpheusProcess {
    /// Records a new quorum certificate in this process's state
    ///
    /// This implements the automatic updating of Q_i from the pseudocode:
    /// "For z ∈ {0,1,2}, if p_i receives a z-quorum or a z-QC for b,
    /// and if Q_i does not contain a z-QC for b, then p_i automatically
    /// enumerates a z-QC for b into Q_i"
    pub fn record_qc(&mut self, qc: Arc<ThreshSigned<VoteData>>) {
        // Skip if we already have this QC
        if self.index.qcs.contains_key(&qc.data) {
            return;
        }

        // Update indexes: qc_by_slot and qc_by_view
        if let Some(author) = &qc.data.for_which.author {
            self.index.qc_by_slot.insert(
                (
                    qc.data.for_which.type_,
                    author.clone(),
                    qc.data.for_which.slot,
                ),
                qc.clone(),
            );

            self.index
                .qc_by_view
                .entry((
                    qc.data.for_which.type_,
                    author.clone(),
                    qc.data.for_which.view,
                ))
                .or_insert_with(Vec::new)
                .push(qc.clone());
        }

        // Track unfinalized QCs
        self.index
            .unfinalized
            .entry(qc.data.for_which.clone())
            .or_default()
            .insert(qc.data.clone());

        // Special handling for 1-QCs
        if qc.data.z == 1 {
            self.index.all_1qc.insert(qc.clone());

            // Update max_1qc if this QC is greater
            if self.index.max_1qc.data.compare_qc(&qc.data) != std::cmp::Ordering::Greater {
                tracing_setup::protocol_transition(
                    &self.id,
                    "updating max 1-QC",
                    &self.index.max_1qc.data,
                    &qc.data,
                    Some("new qc is greater than current max 1-QC"),
                );
                self.index.max_1qc = qc.clone();
            }
        }

        // Update max_view if needed
        if qc.data.for_which.view > self.index.max_view.0 {
            self.index.max_view = (qc.data.for_which.view, qc.data.clone());
        }

        // Update DAG tips - remove tips that are observed by this QC
        let mut tips_to_remove = BTreeSet::new();
        for tip in &self.index.tips {
            if self.observes(qc.data.clone(), tip) {
                tips_to_remove.insert(tip.clone());
                tracing::info!(target: "remove_tip", new_tip = ?qc.data, old_tip = ?tip);
            }
        }
        
        if !tips_to_remove.is_empty() {
            // This QC is a new tip because it observes some existing tips
            self.index.tips.retain(|tip| !tips_to_remove.contains(tip));
            self.index.tips.push(qc.data.clone());
            tracing::info!(target: "new_tip", qc = ?qc.data);
        } else {
            // This QC might still be a new tip if none of the existing tips observe it
            if !self
                .index
                .tips
                .iter()
                .cloned()
                .any(|tip| self.observes(tip, &qc.data))
            {
                self.index.tips.push(qc.data.clone());
                tracing::info!(target: "new_tip", qc = ?qc.data);
            }
        }

        // Add the QC to our state
        self.index.qcs.insert(qc.data.clone(), qc.clone());

        // Process finalization - find all blocks that this QC finalizes
        let finalized_blocks = self.process_finalization(&qc);

        // Update pending votes for affected views
        for block_key in finalized_blocks {
            // Mark the view's pending votes as dirty so they'll be reevaluated
            self.pending_votes
                .entry(block_key.view)
                .or_default()
                .dirty = true;
        }

        // Handle pending votes for 2-votes after a 1-QC is formed
        if qc.data.z == 1 {
            let pending = self
                .pending_votes
                .entry(qc.data.for_which.view)
                .or_default();
            pending.dirty = true;
            match qc.data.for_which.type_ {
                BlockType::Lead => pending.lead_2.insert(qc.data.for_which.clone(), true),
                BlockType::Tr => pending.tr_2.insert(qc.data.for_which.clone(), true),
                BlockType::Genesis => unreachable!(),
            };
        }
    }

    /// Records a new block in this process's state
    ///
    /// This implements part of the automatic updating of M_i from the pseudocode:
    /// "Each process p_i maintains a local variable M_i, which is automatically
    /// updated and specifies the set of all received messages."
    ///
    /// It will also record any QCs that are used as pointers in the block.
    pub fn record_block(&mut self, block: &Arc<Signed<Block>>) {
        // Skip duplicate blocks
        if self.index.blocks.contains_key(&block.data.key) {
            tracing::warn!(target: "duplicate_block", key = ?block.data.key);
            return;
        }

        // Update max_height tracking
        if block.data.key.height > self.index.max_height.0 {
            tracing::debug!(target: "new_max_height", height = block.data.key.height, key = ?block.data.key);
            self.index.max_height = (block.data.key.height, block.data.key.clone());
        }

        // Update block indexing
        if let Some(author) = &block.data.key.author {
            self.index
                .block_index
                .entry((block.data.key.type_, block.data.key.view, author.clone()))
                .or_insert(Vec::new())
                .push(block.clone());

            // Track produced leader blocks
            if block.data.key.type_ == BlockType::Lead && author == &self.id {
                self.produced_lead_in_view.insert(block.data.key.view, true);
            }
        }

        let block_key = block.data.key.clone();
        
        // Initialize finalization tracking
        self.index.finalized.insert(block_key.clone(), false);
        self.index.blocks.insert(block_key.clone(), block.clone());

        // Update pending votes tracking
        let pending = self.pending_votes.entry(block.data.key.view).or_default();
        match block.data.key.type_ {
            BlockType::Lead => {
                // Track leader blocks per view
                self.index
                    .contains_lead_by_view
                    .insert(block.data.key.view, true);
                self.index
                    .unfinalized_lead_by_view
                    .entry(block.data.key.view)
                    .or_default()
                    .insert(block.data.key.clone());
                pending.lead_1.insert(block.data.key.clone(), true);
                pending.dirty = true;
            }
            BlockType::Tr => {
                pending.tr_1.insert(block.data.key.clone(), true);
                pending.dirty = true;
            }
            BlockType::Genesis => panic!("Why are we recording the genesis block?"),
        }

        // Update DAG structure tracking (points-to relationship)
        for qc in &block.data.prev {
            self.index
                .block_pointed_by
                .entry(qc.data.for_which.clone())
                .or_default()
                .insert(block_key.clone());
        }

        // Record any QCs contained in the block
        for qc in block
            .data
            .prev
            .iter()
            .chain(Some(&block.data.one).into_iter())
        {
            self.record_qc(Arc::new(qc.clone()));
        }
    }
}