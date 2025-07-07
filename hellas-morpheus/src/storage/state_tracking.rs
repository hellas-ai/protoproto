use std::{
    collections::BTreeMap,
    sync::Arc,
};

use serde::{Deserialize, Serialize};

use crate::dag_index::DAGIndex;
use crate::qc_index::QCIndex;
use crate::view_index::ViewIndex;
use crate::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PendingVotes {
    pub tr_1: BTreeMap<BlockKey, bool>,
    pub tr_2: BTreeMap<BlockKey, bool>,
    pub lead_1: BTreeMap<BlockKey, bool>,
    pub lead_2: BTreeMap<BlockKey, bool>,
    pub dirty: bool,
}

/// Main state index combining all tracking components
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StateIndex<Tr: Transaction> {
    /// DAG index for block relationships
    #[serde(bound(serialize = "Tr: Transaction", deserialize = "Tr: Transaction"))]
    pub dag: DAGIndex<Tr>,

    /// QC index for quorum certificates
    pub qc_index: QCIndex,

    /// View-based index for leader blocks
    pub view_index: ViewIndex,
}

impl<Tr: Transaction> StateIndex<Tr> {
    /// Create a new state index
    pub fn new(genesis_qc: FinishedQC, genesis_block: Arc<Signed<Block<Tr>>>) -> Self {
        let dag = DAGIndex::new(genesis_qc.clone(), genesis_block);

        let mut qc_index = QCIndex::new(genesis_qc.clone());
        qc_index.insert_qc(genesis_qc);

        Self {
            dag,
            qc_index,
            view_index: ViewIndex::new(),
        }
    }

    /// Get the maximum 1-QC
    pub fn max_1qc(&self) -> &FinishedQC {
        &self.qc_index.max_1qc
    }

    /// Get the current tips
    pub fn tips(&self) -> &Vec<FinishedQC> {
        &self.dag.tips
    }

    /// Get unfinalized QCs
    pub fn unfinalized(
        &self,
    ) -> &std::collections::BTreeMap<BlockKey, std::collections::BTreeSet<FinishedQC>> {
        &self.qc_index.unfinalized
    }

    /// Get the latest transaction QC
    pub fn latest_tr_qc(&self) -> Option<&FinishedQC> {
        self.qc_index.latest_tr_qc.as_ref()
    }

    /// Get the latest leader QC
    pub fn latest_leader_qc(&self) -> Option<&FinishedQC> {
        self.qc_index.latest_leader_qc.as_ref()
    }

    /// Get the latest leader 1-QC
    pub fn latest_leader_1qc(&self) -> Option<&FinishedQC> {
        self.qc_index.latest_leader_1qc.as_ref()
    }

    /// Check if the DAG contains a block
    pub fn contains_block(&self, key: &BlockKey) -> bool {
        self.dag.contains_block(key)
    }

    /// Check if we have a QC
    pub fn has_qc(&self, qc: &FinishedQC) -> bool {
        self.qc_index.contains_qc(qc)
    }

    /// Get a block by key
    pub fn get_block(&self, key: &BlockKey) -> Option<&Arc<Signed<Block<Tr>>>> {
        self.dag.blocks.get(key)
    }

    /// Get the maximum height in the DAG
    pub fn max_height(&self) -> (usize, BlockKey) {
        (
            self.dag.max_height.0 as usize,
            self.dag.max_height.1.clone(),
        )
    }
}
