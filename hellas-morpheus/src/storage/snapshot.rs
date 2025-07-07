//! Snapshot storage implementation using redb

use super::{ConsensusState, SnapshotStore, StateRoot};
use crate::serialization::Postcard;
use crate::*;
use redb::{Database, ReadableTable, TableDefinition};
use std::sync::Arc;

/// Table definition for snapshots
const SNAPSHOTS_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("snapshots");
const SNAPSHOT_INDEX_TABLE: TableDefinition<'static, u64, Postcard<StateRoot>> =
    TableDefinition::new("snapshot_index");

/// Redb-based snapshot storage implementation
#[derive(Debug, Clone)]
pub struct RedbSnapshotStore {
    db: Arc<Database>,
}

impl RedbSnapshotStore {
    /// Create a new snapshot store with the given database
    pub fn new(db: Arc<Database>) -> Result<Self, String> {
        // Ensure tables exist
        let tx = db
            .begin_write()
            .map_err(|e| format!("Failed to begin write transaction: {:?}", e))?;

        tx.open_table(SNAPSHOTS_TABLE)
            .map_err(|e| format!("Failed to open snapshots table: {:?}", e))?;
        tx.open_table(SNAPSHOT_INDEX_TABLE)
            .map_err(|e| format!("Failed to open snapshot index table: {:?}", e))?;

        tx.commit()
            .map_err(|e| format!("Failed to commit transaction: {:?}", e))?;

        Ok(Self { db })
    }
}

impl SnapshotStore for RedbSnapshotStore {
    fn save_snapshot(&mut self, state: &ConsensusState) -> Result<StateRoot, String> {
        let tx = self
            .db
            .begin_write()
            .map_err(|e| format!("Failed to begin write transaction: {:?}", e))?;

        // Create state root by hashing the consensus state
        let state_bytes = postcard::to_stdvec(state)
            .map_err(|e| format!("Failed to serialize state: {:?}", e))?;
        let hash = blake3::hash(&state_bytes);
        let root = StateRoot(*hash.as_bytes());

        {
            let mut snapshots_table = tx
                .open_table(SNAPSHOTS_TABLE)
                .map_err(|e| format!("Failed to open snapshots table: {:?}", e))?;
            let root_array: &[u8] = &root.0;
            snapshots_table
                .insert(root_array, &state_bytes[..])
                .map_err(|e| format!("Failed to insert snapshot: {:?}", e))?;

            let mut index_table = tx
                .open_table(SNAPSHOT_INDEX_TABLE)
                .map_err(|e| format!("Failed to open index table: {:?}", e))?;
            index_table
                .insert(state.current_view.0 as u64, root.clone())
                .map_err(|e| format!("Failed to insert index: {:?}", e))?;
        }

        tx.commit()
            .map_err(|e| format!("Failed to commit transaction: {:?}", e))?;

        Ok(root)
    }

    fn load_snapshot(&self, root: &StateRoot) -> Result<Option<ConsensusState>, String> {
        let tx = self
            .db
            .begin_read()
            .map_err(|e| format!("Failed to begin read transaction: {:?}", e))?;

        let snapshots_table = tx
            .open_table(SNAPSHOTS_TABLE)
            .map_err(|e| format!("Failed to open snapshots table: {:?}", e))?;

        let root_array: &[u8] = &root.0;
        snapshots_table
            .get(root_array)
            .map_err(|e| format!("Failed to get snapshot: {:?}", e))?
            .map(|value| {
                postcard::from_bytes(value.value())
                    .map_err(|e| format!("Failed to deserialize state: {:?}", e))
            })
            .transpose()
    }

    fn get_latest_snapshot(&self) -> Result<Option<(StateRoot, ConsensusState)>, String> {
        let tx = self
            .db
            .begin_read()
            .map_err(|e| format!("Failed to begin read transaction: {:?}", e))?;

        let index_table = tx
            .open_table(SNAPSHOT_INDEX_TABLE)
            .map_err(|e| format!("Failed to open index table: {:?}", e))?;

        // Find the latest view
        let latest_root_data = index_table
            .iter()
            .map_err(|e| format!("Failed to iterate index table: {:?}", e))?
            .next_back()
            .and_then(|item| item.ok())
            .map(|(_, postcard_root)| postcard_root.value().0);

        if let Some(latest_root) = latest_root_data {
            let snapshots_table = tx
                .open_table(SNAPSHOTS_TABLE)
                .map_err(|e| format!("Failed to open snapshots table: {:?}", e))?;

            let root_array: &[u8] = &latest_root;
            let state = snapshots_table
                .get(root_array)
                .map_err(|e| format!("Failed to get snapshot: {:?}", e))?
                .map(|value| {
                    postcard::from_bytes(value.value())
                        .map_err(|e| format!("Failed to deserialize state: {:?}", e))
                })
                .transpose()?
                .ok_or_else(|| "Snapshot not found despite index entry".to_string())?;

            Ok(Some((StateRoot(latest_root), state)))
        } else {
            Ok(None)
        }
    }

    fn list_snapshots(&self) -> Result<Vec<StateRoot>, String> {
        let tx = self
            .db
            .begin_read()
            .map_err(|e| format!("Failed to begin read transaction: {:?}", e))?;

        let index_table = tx
            .open_table(SNAPSHOT_INDEX_TABLE)
            .map_err(|e| format!("Failed to open index table: {:?}", e))?;

        let mut roots = Vec::new();
        for entry in index_table
            .iter()
            .map_err(|e| format!("Failed to iterate index table: {:?}", e))?
        {
            let (_, postcard_root) = entry.map_err(|e| format!("Failed to read entry: {:?}", e))?;
            roots.push(postcard_root.value().0);
        }

        Ok(roots.into_iter().map(StateRoot).collect())
    }

    fn prune_snapshots(&mut self, keep_count: usize) -> Result<usize, String> {
        let tx = self
            .db
            .begin_write()
            .map_err(|e| format!("Failed to begin write transaction: {:?}", e))?;

        let mut to_remove = Vec::new();

        {
            let index_table = tx
                .open_table(SNAPSHOT_INDEX_TABLE)
                .map_err(|e| format!("Failed to open index table: {:?}", e))?;

            // Collect all entries
            let mut entries: Vec<_> = index_table
                .iter()
                .map_err(|e| format!("Failed to iterate index table: {:?}", e))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("Failed to collect entries: {:?}", e))?;

            // Sort by view number (descending)
            entries.sort_by(|a, b| b.0.value().cmp(&a.0.value()));

            // Mark older entries for removal
            if entries.len() > keep_count {
                for (view, postcard_root) in entries.into_iter().skip(keep_count) {
                    to_remove.push((view.value(), postcard_root.value().0));
                }
            }
        }

        let removed_count = to_remove.len();

        // Remove marked entries
        if !to_remove.is_empty() {
            let mut snapshots_table = tx
                .open_table(SNAPSHOTS_TABLE)
                .map_err(|e| format!("Failed to open snapshots table: {:?}", e))?;
            let mut index_table = tx
                .open_table(SNAPSHOT_INDEX_TABLE)
                .map_err(|e| format!("Failed to open index table: {:?}", e))?;

            for (view, root_data) in to_remove {
                // Remove from index
                index_table
                    .remove(view)
                    .map_err(|e| format!("Failed to remove from index: {:?}", e))?;

                // Remove from snapshots
                let root_array: &[u8] = &root_data;
                snapshots_table
                    .remove(root_array)
                    .map_err(|e| format!("Failed to remove snapshot: {:?}", e))?;
            }
        }

        tx.commit()
            .map_err(|e| format!("Failed to commit transaction: {:?}", e))?;

        Ok(removed_count)
    }
}
