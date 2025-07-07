//! Storage layer architecture for the Morpheus protocol
//!
//! This module provides a three-tier storage architecture:
//! 1. Bulk Storage - Append-only storage for immutable consensus artifacts
//! 2. Snapshot Storage - Lightweight state snapshots that reference bulk storage
//! 3. Event Log - Already implemented separately for deterministic replay

use crate::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub(crate) mod bulk;
pub use bulk::*;

pub(crate) mod dag_index;
pub use dag_index::*;

pub(crate) mod event_log;
pub use event_log::*;

pub(crate) mod memory;
pub use memory::*;

pub(crate) mod qc_index;
pub use qc_index::*;

pub(crate) mod serialization;
pub use serialization::*;

pub(crate) mod snapshot;
pub use snapshot::*;

pub(crate) mod state_tracking;
pub use state_tracking::*;

pub(crate) mod view_index;
pub use view_index::*;

/// Reference to a block in bulk storage
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct BlockRef {
    /// Block key (unique identifier)
    pub key: BlockKey,
    /// Optional content hash for verification
    pub hash: Option<BlockHash>,
}

/// Reference to a QC in bulk storage
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct QCRef {
    /// The vote data this QC is for
    pub vote_data: VoteData,
    /// Optional hash for verification
    pub hash: Option<[u8; 32]>,
}

/// Reference to a vote in bulk storage
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct VoteRef {
    /// Voter identity
    pub voter: Identity,
    /// Vote data
    pub vote_data: VoteData,
    /// Optional hash for verification
    pub hash: Option<[u8; 32]>,
}

/// Trait for bulk storage of consensus artifacts
pub trait BulkStore<Tr: Transaction>: Send + Sync {
    /// Append a block to storage
    fn append_block(&mut self, block: Arc<Signed<Block<Tr>>>) -> Result<BlockRef, String>;

    /// Append a QC to storage
    fn append_qc(&mut self, qc: FinishedQC) -> Result<QCRef, String>;

    /// Append a vote to storage
    fn append_vote(&mut self, vote: Arc<ThreshPartial<VoteData>>) -> Result<VoteRef, String>;

    /// Get a block by reference
    fn get_block(&self, block_ref: &BlockRef) -> Result<Option<Arc<Signed<Block<Tr>>>>, String>;

    /// Get a QC by reference
    fn get_qc(&self, qc_ref: &QCRef) -> Result<Option<FinishedQC>, String>;

    /// Get a vote by reference
    fn get_vote(&self, vote_ref: &VoteRef) -> Result<Option<Arc<ThreshPartial<VoteData>>>, String>;

    /// Get all blocks in a view
    fn get_blocks_in_view(&self, view: ViewNum) -> Result<Vec<BlockRef>, String>;

    /// Get all QCs for blocks in a view
    fn get_qcs_in_view(&self, view: ViewNum) -> Result<Vec<QCRef>, String>;
}

/// Lightweight consensus state that references bulk storage
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConsensusState {
    /// Current view
    pub current_view: ViewNum,

    /// Current phase
    pub current_phase: Phase,

    /// View entry time
    pub view_entry_time: u128,

    /// Tips of the DAG (as QC references)
    pub tips: Vec<QCRef>,

    /// Maximum 1-QC seen
    pub max_1qc: QCRef,

    /// Finalized blocks (as references)
    pub finalized_blocks: im::HashSet<BlockRef>,

    /// Unfinalized QCs by block
    pub unfinalized_qcs: im::HashMap<BlockRef, im::HashSet<QCRef>>,

    /// Leader blocks by view
    pub leader_blocks_by_view: im::HashMap<ViewNum, im::HashSet<BlockRef>>,

    /// Unfinalized leader blocks by view
    pub unfinalized_leader_by_view: im::HashMap<ViewNum, im::HashSet<BlockRef>>,
}

/// State root is a hash of the consensus state
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct StateRoot(pub [u8; 32]);

/// Trait for snapshot storage
pub trait SnapshotStore: Send + Sync {
    /// Save a snapshot of the consensus state
    fn save_snapshot(&mut self, state: &ConsensusState) -> Result<StateRoot, String>;

    /// Load a snapshot by its root
    fn load_snapshot(&self, root: &StateRoot) -> Result<Option<ConsensusState>, String>;

    /// Get the latest snapshot
    fn get_latest_snapshot(&self) -> Result<Option<(StateRoot, ConsensusState)>, String>;

    /// List all snapshot roots in order (oldest to newest)
    fn list_snapshots(&self) -> Result<Vec<StateRoot>, String>;

    /// Prune old snapshots, keeping at least `keep_count` most recent
    fn prune_snapshots(&mut self, keep_count: usize) -> Result<usize, String>;
}

/// Runtime configuration for storage invariant checking
#[derive(Debug, Clone, Default)]
pub struct InvariantCheckConfig {
    /// Whether to check cache consistency with bulk storage
    pub check_cache_consistency: bool,
    /// Whether to check snapshot consistency with bulk storage
    pub check_snapshot_consistency: bool,
    /// Whether to check view index consistency
    pub check_view_index_consistency: bool,
    /// Whether to check DAG consistency
    pub check_dag_consistency: bool,
    /// Whether to check finalization invariants
    pub check_finalization_invariants: bool,
}

impl InvariantCheckConfig {
    /// Create a paranoid configuration that checks everything
    pub fn paranoid() -> Self {
        Self {
            check_cache_consistency: true,
            check_snapshot_consistency: true,
            check_view_index_consistency: true,
            check_dag_consistency: true,
            check_finalization_invariants: true,
        }
    }

    /// Create a debug configuration with basic checks
    pub fn debug() -> Self {
        Self {
            check_cache_consistency: true,
            check_snapshot_consistency: false,
            check_view_index_consistency: true,
            check_dag_consistency: true,
            check_finalization_invariants: false,
        }
    }
}

/// Storage invariants that should be maintained
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageInvariant {
    /// All blocks referenced in view cache should exist in bulk storage
    BlockInCacheButNotInBulk { key: BlockKey },

    /// All QCs referenced in view cache should exist in bulk storage
    QcInCacheButNotInBulk { vote_data: VoteData },

    /// All votes referenced in view cache should exist in bulk storage
    VoteInCacheButNotInBulk {
        voter: Identity,
        vote_data: VoteData,
    },

    /// All blocks in snapshot state should exist in bulk storage
    BlockInSnapshotButNotInBulk { key: BlockKey },

    /// All QCs in snapshot state should exist in bulk storage
    QcInSnapshotButNotInBulk { vote_data: VoteData },

    /// View index should be consistent with actual stored items
    ViewIndexInconsistent {
        view: ViewNum,
        expected_blocks: usize,
        actual_blocks: usize,
    },

    /// DAG parent-child relationships should be consistent
    DagRelationshipInconsistent { parent: BlockKey, child: BlockKey },

    /// Tips should be maximal (not observed by any other QC)
    TipNotMaximal {
        tip: VoteData,
        observed_by: VoteData,
    },

    /// All finalized blocks should have 2-QCs
    FinalizedBlockWithout2QC { key: BlockKey },

    /// Snapshot state should be internally consistent
    SnapshotStateInconsistent { description: String },
}

/// Storage invariant checker
#[derive(Debug, Clone)]
pub struct InvariantChecker {
    pub config: InvariantCheckConfig,
}

impl InvariantChecker {
    pub fn new(config: InvariantCheckConfig) -> Self {
        Self { config }
    }

    /// Check all configured invariants
    pub fn check_invariants<Tr: Transaction, B: BulkStore<Tr>, S: SnapshotStore>(
        &self,
        bulk_store: &B,
        snapshot_store: &S,
        view_cache: &ViewCache<Tr>,
        consensus_state: &ConsensusState,
    ) -> Vec<StorageInvariant> {
        let mut violations = Vec::new();

        if self.config.check_cache_consistency {
            self.check_cache_consistency(bulk_store, view_cache, &mut violations);
        }

        if self.config.check_snapshot_consistency {
            self.check_snapshot_consistency(
                bulk_store,
                snapshot_store,
                consensus_state,
                &mut violations,
            );
        }

        if self.config.check_view_index_consistency {
            self.check_view_index_consistency(
                bulk_store,
                consensus_state.current_view,
                &mut violations,
            );
        }

        if self.config.check_dag_consistency {
            self.check_dag_consistency(bulk_store, view_cache, &mut violations);
        }

        if self.config.check_finalization_invariants {
            self.check_finalization_invariants(consensus_state, &mut violations);
        }

        violations
    }

    fn check_cache_consistency<Tr: Transaction, B: BulkStore<Tr>>(
        &self,
        bulk_store: &B,
        cache: &ViewCache<Tr>,
        violations: &mut Vec<StorageInvariant>,
    ) {
        // Check all cached blocks exist in bulk storage
        for (key, _) in &cache.blocks {
            let block_ref = BlockRef {
                key: key.clone(),
                hash: key.hash.clone(),
            };

            match bulk_store.get_block(&block_ref) {
                Ok(None) => {
                    violations
                        .push(StorageInvariant::BlockInCacheButNotInBulk { key: key.clone() });
                }
                Err(e) => {
                    tracing::warn!("Error checking block in bulk storage: {}", e);
                }
                _ => {}
            }
        }

        // Check all cached QCs exist in bulk storage
        for qc in cache.qcs.values() {
            let qc_ref = QCRef {
                vote_data: qc.data.clone(),
                hash: None,
            };

            match bulk_store.get_qc(&qc_ref) {
                Ok(None) => {
                    violations.push(StorageInvariant::QcInCacheButNotInBulk {
                        vote_data: qc.data.clone(),
                    });
                }
                Err(e) => {
                    tracing::warn!("Error checking QC in bulk storage: {}", e);
                }
                _ => {}
            }
        }

        // Check all cached votes exist in bulk storage
        for (vote_data, votes) in &cache.votes {
            for (voter, _vote) in votes {
                let vote_ref = VoteRef {
                    voter: voter.clone(),
                    vote_data: vote_data.clone(),
                    hash: None,
                };

                match bulk_store.get_vote(&vote_ref) {
                    Ok(None) => {
                        violations.push(StorageInvariant::VoteInCacheButNotInBulk {
                            voter: voter.clone(),
                            vote_data: vote_data.clone(),
                        });
                    }
                    Err(e) => {
                        tracing::warn!("Error checking vote in bulk storage: {}", e);
                    }
                    _ => {}
                }
            }
        }
    }

    fn check_snapshot_consistency<Tr: Transaction, B: BulkStore<Tr>, S: SnapshotStore>(
        &self,
        bulk_store: &B,
        _snapshot_store: &S,
        consensus_state: &ConsensusState,
        violations: &mut Vec<StorageInvariant>,
    ) {
        // Check all blocks referenced in consensus state exist
        for block_ref in &consensus_state.finalized_blocks {
            match bulk_store.get_block(block_ref) {
                Ok(None) => {
                    violations.push(StorageInvariant::BlockInSnapshotButNotInBulk {
                        key: block_ref.key.clone(),
                    });
                }
                Err(e) => {
                    tracing::warn!("Error checking snapshot block in bulk storage: {}", e);
                }
                _ => {}
            }
        }

        // Check tips exist
        for tip_ref in &consensus_state.tips {
            match bulk_store.get_qc(tip_ref) {
                Ok(None) => {
                    violations.push(StorageInvariant::QcInSnapshotButNotInBulk {
                        vote_data: tip_ref.vote_data.clone(),
                    });
                }
                Err(e) => {
                    tracing::warn!("Error checking tip QC in bulk storage: {}", e);
                }
                _ => {}
            }
        }

        // Check internal consistency
        if consensus_state.current_view.0 < 0 && consensus_state.current_view != ViewNum(0) {
            violations.push(StorageInvariant::SnapshotStateInconsistent {
                description: format!("Invalid current view: {:?}", consensus_state.current_view),
            });
        }
    }

    fn check_view_index_consistency<Tr: Transaction, B: BulkStore<Tr>>(
        &self,
        bulk_store: &B,
        current_view: ViewNum,
        violations: &mut Vec<StorageInvariant>,
    ) {
        // Check recent views
        for i in -5..=0 {
            let view = ViewNum(current_view.0.saturating_add(i));

            if let Ok(blocks_in_view) = bulk_store.get_blocks_in_view(view) {
                // Verify each block actually exists
                let mut actual_count = 0;
                for block_ref in &blocks_in_view {
                    if let Ok(Some(_)) = bulk_store.get_block(block_ref) {
                        actual_count += 1;
                    }
                }

                if actual_count != blocks_in_view.len() {
                    violations.push(StorageInvariant::ViewIndexInconsistent {
                        view,
                        expected_blocks: blocks_in_view.len(),
                        actual_blocks: actual_count,
                    });
                }
            }
        }
    }

    fn check_dag_consistency<Tr: Transaction, B: BulkStore<Tr>>(
        &self,
        bulk_store: &B,
        cache: &ViewCache<Tr>,
        violations: &mut Vec<StorageInvariant>,
    ) {
        // For each block, verify parent-child relationships
        for (key, block) in &cache.blocks {
            for prev_qc in &block.data.prev {
                // Check if the parent block exists
                let parent_ref = BlockRef {
                    key: prev_qc.data.for_which.clone(),
                    hash: prev_qc.data.for_which.hash.clone(),
                };

                if let Ok(None) = bulk_store.get_block(&parent_ref) {
                    violations.push(StorageInvariant::DagRelationshipInconsistent {
                        parent: prev_qc.data.for_which.clone(),
                        child: key.clone(),
                    });
                }
            }
        }
    }

    fn check_finalization_invariants(
        &self,
        consensus_state: &ConsensusState,
        _violations: &mut Vec<StorageInvariant>,
    ) {
        // Check that all finalized blocks should have implied 2-QCs
        // This is a simplified check - in reality we'd need to traverse the QC structure
        for block_ref in &consensus_state.finalized_blocks {
            // For now, just check that it's not a genesis block
            if block_ref.key != GEN_BLOCK_KEY {
                // TODO: critical!
                // In a real implementation, we'd check for the existence of a 2-QC
            }
        }

        // Check tips are maximal
        for (i, _tip1) in consensus_state.tips.iter().enumerate() {
            for (j, _tip2) in consensus_state.tips.iter().enumerate() {
                if i != j {
                    // In a real implementation, we'd check if tip1 observes tip2
                    // TODO: critical!
                }
            }
        }
    }
}

/// Helper function to log invariant violations
pub fn log_invariant_violations(violations: &[StorageInvariant], process_id: &Identity) {
    if !violations.is_empty() {
        tracing::error!(
            target: "storage_invariants",
            process = ?process_id,
            violation_count = violations.len(),
            "Storage invariant violations detected"
        );

        for violation in violations {
            tracing::error!(
                target: "storage_invariants",
                process = ?process_id,
                violation = ?violation,
                "Storage invariant violation"
            );
        }
    }
}
