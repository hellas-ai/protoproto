use std::{
    collections::{BTreeMap, BTreeSet},
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

/// Tracks all structural state
///
/// This is now a composition of specialized index components
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateIndex<Tr: Transaction> {
    /// DAG structure and block relationships
    #[serde(bound(serialize = "Tr: Transaction", deserialize = "Tr: Transaction"))]
    pub dag: DAGIndex<Tr>,

    /// QC tracking and finalization state
    pub qc_index: QCIndex,

    /// View-specific tracking
    pub view_index: ViewIndex,
}

impl<Tr: Transaction> StateIndex<Tr> {
    pub fn new(genesis_qc: FinishedQC, genesis_block: Arc<Signed<Block<Tr>>>) -> Self {
        Self {
            dag: DAGIndex::new(genesis_qc.clone(), genesis_block),
            qc_index: QCIndex::new(genesis_qc),
            view_index: ViewIndex::new(),
        }
    }

    // Keep only the essential accessor methods that are still used
    
    pub fn tips(&self) -> &Vec<FinishedQC> {
        &self.dag.tips
    }

    pub fn max_view(&self) -> (ViewNum, FinishedQC) {
        self.qc_index.max_view.clone()
    }

    pub fn max_1qc(&self) -> &FinishedQC {
        &self.qc_index.max_1qc
    }

    pub fn latest_leader_1qc(&self) -> Option<&FinishedQC> {
        self.qc_index.latest_leader_1qc.as_ref()
    }

    pub fn latest_leader_qc(&self) -> Option<&FinishedQC> {
        self.qc_index.latest_leader_qc.as_ref()
    }

    pub fn latest_tr_qc(&self) -> Option<&FinishedQC> {
        self.qc_index.latest_tr_qc.as_ref()
    }

    pub fn unfinalized(&self) -> &BTreeMap<BlockKey, BTreeSet<FinishedQC>> {
        &self.qc_index.unfinalized
    }
    
    // Additional methods needed by invariants
    pub fn finalized(&self) -> &BTreeSet<BlockKey> {
        &self.qc_index.finalized
    }
    
    pub fn blocks(&self) -> &BTreeMap<BlockKey, Arc<Signed<Block<Tr>>>> {
        &self.dag.blocks
    }
    
    pub fn max_height(&self) -> (usize, BlockKey) {
        self.dag.max_height.clone()
    }
    
    pub fn unfinalized_2qc(&self) -> &BTreeSet<FinishedQC> {
        &self.qc_index.unfinalized_2qc
    }
}
