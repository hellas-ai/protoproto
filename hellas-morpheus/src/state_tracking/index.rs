use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use ark_serialize::CanonicalSerialize;
use serde::{Deserialize, Serialize};
use crate::*;

/// Stores all structural indices of the protocol state
#[derive(Clone, Serialize, Deserialize)]
pub struct StateIndex {
    /// Stores QCs indexed by their VoteData
    #[serde(with = "serde_json_any_key::any_key_map")]
    pub qcs: BTreeMap<VoteData, Arc<ThreshSigned<VoteData>>>,
    /// All 1-QCs seen
    pub all_1qc: BTreeSet<Arc<ThreshSigned<VoteData>>>,
    /// Current tips of the block DAG
    pub tips: Vec<VoteData>,
    /// Maps block keys to blocks
    #[serde(with = "serde_json_any_key::any_key_map")]
    pub blocks: BTreeMap<BlockKey, Arc<Signed<Block>>>,
    /// Which blocks point to which other blocks
    #[serde(with = "serde_json_any_key::any_key_map")]
    pub block_pointed_by: BTreeMap<BlockKey, BTreeSet<BlockKey>>,
    /// Maximum view seen and its associated VoteData
    pub max_view: (ViewNum, VoteData),
    /// Maximum height block seen and its key
    pub max_height: (usize, BlockKey),
    /// Maximum 1-QC seen
    pub max_1qc: Arc<ThreshSigned<VoteData>>,
    /// Unfinalized blocks with 2-QC
    pub unfinalized_2qc: BTreeSet<VoteData>,
    /// Finalization status per block
    #[serde(with = "serde_json_any_key::any_key_map")]
    pub finalized: BTreeMap<BlockKey, bool>,
    /// Unfinalized QCs per block
    #[serde(with = "serde_json_any_key::any_key_map")]
    pub unfinalized: BTreeMap<BlockKey, BTreeSet<VoteData>>,
    /// Leader presence per view
    #[serde(with = "serde_json_any_key::any_key_map")]
    pub contains_lead_by_view: BTreeMap<ViewNum, bool>,
    /// Unfinalized leader blocks per view
    #[serde(with = "serde_json_any_key::any_key_map")]
    pub unfinalized_lead_by_view: BTreeMap<ViewNum, BTreeSet<BlockKey>>,
    /// Quick index to QCs by slot
    #[serde(with = "serde_json_any_key::any_key_map")]
    pub qc_by_slot: BTreeMap<(BlockType, Identity, SlotNum), Arc<ThreshSigned<VoteData>>>,
    /// QCs by view
    #[serde(with = "serde_json_any_key::any_key_map")]
    pub qc_by_view: BTreeMap<(BlockType, Identity, ViewNum), Vec<Arc<ThreshSigned<VoteData>>>>,
    /// Blocks by type, view, and author
    #[serde(with = "serde_json_any_key::any_key_map")]
    pub block_index: BTreeMap<(BlockType, ViewNum, Identity), Vec<Arc<Signed<Block>>>>,
}

impl StateIndex {
    /// Create a new index with the genesis block and QC
    pub fn new(genesis_qc: Arc<ThreshSigned<VoteData>>, genesis_block: Arc<Signed<Block>>) -> Self {
        let mut qcs = BTreeMap::new();
        qcs.insert(genesis_qc.data.clone(), genesis_qc.clone());
        let mut blocks = BTreeMap::new();
        blocks.insert(GEN_BLOCK_KEY, genesis_block.clone());
        let mut finalized = BTreeMap::new();
        finalized.insert(GEN_BLOCK_KEY, true);
        Self {
            qcs,
            all_1qc: BTreeSet::new(),
            tips: vec![genesis_qc.data.clone()],
            blocks,
            block_pointed_by: BTreeMap::new(),
            max_view: (ViewNum(-1), genesis_qc.data.clone()),
            max_height: (0, GEN_BLOCK_KEY),
            max_1qc: genesis_qc.clone(),
            unfinalized_2qc: BTreeSet::new(),
            finalized,
            unfinalized: BTreeMap::new(),
            contains_lead_by_view: BTreeMap::new(),
            unfinalized_lead_by_view: BTreeMap::new(),
            qc_by_slot: BTreeMap::from([(
                (BlockType::Genesis, Identity(u64::MAX), SlotNum(0)), genesis_qc.clone()
            )]),
            qc_by_view: BTreeMap::from([(
                (BlockType::Genesis, Identity(u64::MAX), ViewNum(-1)), vec![genesis_qc.clone()]
            )]),
            block_index: {
                let mut idx = BTreeMap::new();
                idx.insert((BlockType::Genesis, ViewNum(-1), Identity(u64::MAX)), vec![genesis_block.clone()]);
                idx
            },
        }
    }
}