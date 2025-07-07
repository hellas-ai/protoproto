use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::*;

/// Manages the DAG structure of blocks and their relationships
///
/// This component is responsible for:
/// - Tracking block relationships (who points to whom)
/// - Maintaining the tips of the DAG
/// - Implementing the observes relation
/// - Determining block heights
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DAGIndex<Tr: Transaction> {
    /// Maps block keys to signed blocks
    #[serde(bound(serialize = "Tr: Transaction", deserialize = "Tr: Transaction"))]
    pub blocks: BTreeMap<BlockKey, Arc<Signed<Block<Tr>>>>,

    /// Tracks which blocks point to which other blocks
    /// Key: pointed-to block, Value: set of blocks that point to it
    pub block_pointed_by: BTreeMap<BlockKey, BTreeSet<BlockKey>>,

    /// Stores the current tips of the block DAG
    /// "The tips of Q_i are those q ∈ Q_i such that there does not exist q' ∈ Q_i with q' ≻ q"
    pub tips: Vec<FinishedQC>,

    /// Tracks the maximum height block seen and its key
    pub max_height: (u64, BlockKey),
}

impl<Tr: Transaction> DAGIndex<Tr> {
    pub fn new(genesis_qc: FinishedQC, genesis_block: Arc<Signed<Block<Tr>>>) -> Self {
        let mut blocks = BTreeMap::new();
        blocks.insert(GEN_BLOCK_KEY, genesis_block);

        Self {
            blocks,
            block_pointed_by: BTreeMap::new(),
            tips: vec![genesis_qc],
            max_height: (0, GEN_BLOCK_KEY),
        }
    }

    /// Check if a block exists in the DAG
    pub fn contains_block(&self, key: &BlockKey) -> bool {
        self.blocks.contains_key(key)
    }

    /// Records a new block in the DAG
    ///
    /// Updates:
    /// - The blocks map
    /// - The block_pointed_by relationships
    /// - The max_height if necessary
    pub fn insert_block(&mut self, block: &Arc<Signed<Block<Tr>>>) -> bool {
        let block_key = block.data.key.clone();

        if self.blocks.contains_key(&block_key) {
            tracing::warn!(target: "duplicate_block", key = ?block_key);
            return false;
        }

        // Update max_height if necessary
        if block_key.height > self.max_height.0 {
            tracing::debug!(target: "new_max_height", prev_height = ?self.max_height, key = ?block_key);
            self.max_height = (block_key.height, block_key.clone());
        }

        // Insert the block
        self.blocks.insert(block_key.clone(), block.clone());

        // Update block_pointed_by relationships
        for qc in &block.data.prev {
            self.block_pointed_by
                .entry(qc.data.for_which.clone())
                .or_default()
                .insert(block_key.clone());
        }

        true
    }

    /// Updates the tips based on a new QC
    ///
    /// When a new QC is added, it may:
    /// - Replace existing tips that it observes
    /// - Become a new tip if it's not observed by existing tips
    pub fn update_tips_for_qc(&mut self, qc: FinishedQC) {
        let mut tips_to_remove = BTreeSet::new();

        // Check if the new QC observes any existing tips
        for tip in &self.tips {
            if self.observes(&qc.data, &tip.data) {
                tips_to_remove.insert(tip.clone());
                tracing::debug!(target: "yeet_tip", new_tip = ?qc.data, old_tip = ?tip.data);
            }
        }

        if !tips_to_remove.is_empty() {
            // Remove observed tips and add the new QC
            self.tips.retain(|tip| !tips_to_remove.contains(tip));
            self.tips.push(qc.clone());
            tracing::debug!(target: "new_tip", reason = "extends existing tip", qc = ?qc.data);
        } else {
            // Check if any existing tip observes the new QC
            let observed_by_existing = self
                .tips
                .iter()
                .any(|tip| self.observes(&tip.data, &qc.data));

            if !observed_by_existing {
                // This QC is a new branch
                self.tips.push(qc.clone());
                tracing::debug!(target: "new_tip", reason = "new branch", qc = ?qc.data);
            }
        }
    }

    /// Checks if a block is the single tip of the DAG
    ///
    /// A block is a single tip if:
    /// - There is exactly one tip
    /// - That tip points to this block
    /// - This block is the only parent of the tip
    pub fn is_single_tip(&self, block_key: &BlockKey) -> bool {
        if self.tips.len() != 1 {
            return false;
        }

        match self.tips.first() {
            Some(tip) => self
                .block_pointed_by
                .get(&tip.data.for_which)
                .is_some_and(|parents| {
                    parents.len() == 1 && parents.first().unwrap() == block_key
                }),
            None => false,
        }
    }

    /// Gets all blocks pointed to by a given block
    pub fn get_pointed_blocks(&self, block_key: &BlockKey) -> Vec<BlockKey> {
        self.blocks
            .get(block_key)
            .map(|block| {
                block
                    .data
                    .prev
                    .iter()
                    .map(|qc| qc.data.for_which.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Gets all blocks that point to a given block
    pub fn get_pointing_blocks(&self, block_key: &BlockKey) -> Vec<BlockKey> {
        self.block_pointed_by
            .get(block_key)
            .map(|blocks| blocks.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Performs a BFS to find all blocks observed by a given block
    pub fn get_observed_blocks(&self, root: &BlockKey) -> BTreeSet<BlockKey> {
        let mut observed = BTreeSet::new();
        let mut to_visit = VecDeque::new();

        to_visit.push_back(root.clone());

        while let Some(current) = to_visit.pop_front() {
            if observed.insert(current.clone()) {
                // Add all blocks this one points to
                for pointed in self.get_pointed_blocks(&current) {
                    to_visit.push_back(pointed);
                }
            }
        }

        observed
    }

    /// Determines if one QC observes another according to the observes relation ⪰
    ///
    /// Implements the observes relation from the pseudocode:
    /// "We define the 'observes' relation ⪰ on Q_i to be the minimal preordering satisfying (transitivity and):
    /// • If q,q' ∈ Q_i, q.type = q'.type, q.auth = q'.auth and q.slot > q'.slot, then q ⪰ q'.
    /// • If q,q' ∈ Q_i, q.type = q'.type, q.auth = q'.auth, q.slot = q'.slot, and q.z ≥ q'.z, then q ⪰ q'."
    /// • If q,q' ∈ Q_i, q.b = b, q'.b = b', b ∈ M_i and b points to b', then q ⪰ q'."
    pub fn observes(&self, root: &VoteData, needle: &VoteData) -> bool {
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

    /// Determines if one QC directly observes another (without transitivity)
    pub fn directly_observes(&self, looks: &VoteData, seen: &VoteData) -> bool {
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
            if block
                .data
                .prev
                .iter()
                .any(|prev| prev.data.for_which == seen.for_which)
            {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_dag_index_creation() {
        // Test creation and basic operations
        // TODO: Add comprehensive tests
    }
}
