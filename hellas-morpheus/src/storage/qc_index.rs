use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::*;

/// Manages Quorum Certificate tracking and finalization state
///
/// This component is responsible for:
/// - Tracking all QCs
/// - Managing finalization state
/// - Tracking unfinalized QCs
/// - Maintaining max QCs by type
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QCIndex {
    /// All QCs seen by this process
    pub qcs: BTreeSet<FinishedQC>,

    /// Tracks the maximum view seen and its associated VoteData
    pub max_view: (ViewNum, FinishedQC),

    /// Stores the maximum 1-QC seen by this process
    /// Used when entering a new view: "Send (v, q') signed by p_i to lead(v),
    /// where q' is a maximal amongst 1-QCs seen by p_i"
    pub max_1qc: FinishedQC,

    /// 1-QC for the leader block we produced in our previous slot
    pub latest_leader_1qc: Option<FinishedQC>,

    /// z-QC for the leader block we produced in our previous slot
    pub latest_leader_qc: Option<FinishedQC>,

    /// z-QC for the transaction block we produced in our previous slot
    pub latest_tr_qc: Option<FinishedQC>,

    /// Tracks unfinalized blocks with 2-QC
    /// Used to identify blocks that have 2-QC but are not yet finalized
    pub unfinalized_2qc: BTreeSet<FinishedQC>,

    /// Maps block keys to their finalization status
    /// Used to track which blocks have been finalized
    pub finalized: BTreeSet<BlockKey>,

    /// Maps block keys to their unfinalized QCs
    /// Used to track which QCs are not yet finalized
    pub unfinalized: BTreeMap<BlockKey, BTreeSet<FinishedQC>>,
}

impl QCIndex {
    pub fn new(genesis_qc: FinishedQC) -> Self {
        Self {
            qcs: BTreeSet::from([genesis_qc.clone()]),
            max_view: (genesis_qc.data.for_which.view, genesis_qc.clone()),
            max_1qc: genesis_qc,
            latest_leader_1qc: None,
            latest_leader_qc: None,
            latest_tr_qc: None,
            unfinalized_2qc: BTreeSet::new(),
            finalized: BTreeSet::from([GEN_BLOCK_KEY]),
            unfinalized: BTreeMap::new(),
        }
    }

    /// Records a new QC and returns true if it was newly inserted
    pub fn insert_qc(&mut self, qc: FinishedQC) -> bool {
        if !self.qcs.insert(qc.clone()) {
            return false;
        }

        // Update max_view if necessary
        if qc.data.for_which.view > self.max_view.0 {
            tracing::debug!(target: "new_max_view", old_max_view = ?self.max_view, new_block = ?qc.data.for_which);
            self.max_view = (qc.data.for_which.view, qc.clone());
        }

        // Update max_1qc if this is a 1-QC
        if qc.data.z == 1 && self.max_1qc.data.compare_qc(&qc.data) != std::cmp::Ordering::Greater {
            tracing::debug!(target: "new_max_1qc", old_max_1qc = ?self.max_1qc.data, new_1qc = ?qc.data);
            self.max_1qc = qc.clone();
        }

        // All new QCs are unfinalized until proven otherwise
        self.unfinalized
            .entry(qc.data.for_which.clone())
            .or_default()
            .insert(qc.clone());

        // Track unfinalized 2-QCs
        if qc.data.z == 2 {
            self.unfinalized_2qc.insert(qc);
        }

        true
    }

    /// Updates latest QCs for blocks produced by a specific process
    pub fn update_latest_qcs(
        &mut self,
        qc: &FinishedQC,
        process_id: &Identity,
        slot_i_lead: SlotNum,
        slot_i_tr: SlotNum,
    ) {
        if let Some(author) = &qc.data.for_which.author {
            if author == process_id {
                match qc.data.for_which.type_ {
                    BlockType::Lead if qc.data.for_which.slot.is_pred(slot_i_lead) => {
                        self.latest_leader_qc = Some(qc.clone());
                        if qc.data.z == 1 {
                            self.latest_leader_1qc = Some(qc.clone());
                        }
                    }
                    BlockType::Tr if qc.data.for_which.slot.is_pred(slot_i_tr) => {
                        self.latest_tr_qc = Some(qc.clone());
                    }
                    _ => {}
                }
            }
        }
    }

    /// Finalizes blocks based on a new QC that observes them
    ///
    /// Returns the set of blocks that were finalized
    pub fn finalize_blocks_observed_by(
        &mut self,
        qc: &FinishedQC,
        observes_fn: impl Fn(&VoteData, &VoteData) -> bool,
    ) -> BTreeSet<FinishedQC> {
        let mut finalized_here = BTreeSet::new();

        // Find all unfinalized 2-QCs that this QC observes
        for unfinalized_2qc in &self.unfinalized_2qc {
            if observes_fn(&qc.data, &unfinalized_2qc.data) {
                finalized_here.insert(unfinalized_2qc.clone());
            }
        }

        // Remove finalized QCs from unfinalized_2qc
        self.unfinalized_2qc
            .retain(|unfinalized_2qc| !finalized_here.contains(unfinalized_2qc));

        // Mark blocks as finalized
        for finalized_qc in &finalized_here {
            tracing::debug!(target: "finalized_block", cause_qc = ?finalized_qc, key = ?finalized_qc.data.for_which);
            self.unfinalized.remove(&finalized_qc.data.for_which);
            self.finalized.insert(finalized_qc.data.for_which.clone());
        }

        finalized_here
    }

    /// Checks if a block is finalized
    pub fn is_finalized(&self, block_key: &BlockKey) -> bool {
        self.finalized.contains(block_key)
    }

    /// Gets all unfinalized QCs for a block
    pub fn get_unfinalized_qcs(&self, block_key: &BlockKey) -> Option<&BTreeSet<FinishedQC>> {
        self.unfinalized.get(block_key)
    }

    /// Gets all QCs for a specific block
    pub fn get_qcs_for_block(&self, block_key: &BlockKey) -> Vec<FinishedQC> {
        self.qcs
            .iter()
            .filter(|qc| &qc.data.for_which == block_key)
            .cloned()
            .collect()
    }

    /// Checks if we have a specific z-QC for a block
    pub fn has_z_qc(&self, block_key: &BlockKey, z: u8) -> bool {
        self.qcs
            .iter()
            .any(|qc| &qc.data.for_which == block_key && qc.data.z == z)
    }

    /// Check if we have a QC
    pub fn contains_qc(&self, qc: &FinishedQC) -> bool {
        let _key = (qc.data.z, qc.data.for_which.clone());
        self.qcs.contains(qc)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_qc_index_creation() {
        // Test creation and basic operations
        // TODO: Add comprehensive tests
    }
}
