use crate::process::ProcessSnapshot;
use crate::serialization::Postcard;
use crate::storage::{BulkStore, SnapshotStore};
use crate::*;
use redb::{Database, ReadableTable, ReadableTableMetadata, TableDefinition};
use serde::{Deserialize, Serialize};

/// Entry in the event log containing an action and its resulting effects
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub struct LogEntry<Tr: Transaction> {
    /// The external action that triggered this entry
    #[serde(bound(serialize = "Tr: Transaction", deserialize = "Tr: Transaction"))]
    pub action: Action<Tr>,

    /// The effects produced by processing the action
    #[serde(bound(serialize = "Tr: Transaction", deserialize = "Tr: Transaction"))]
    pub effects: Vec<Effect<Tr>>,
}

/// Manages event logging and replay functionality
#[derive(Clone, Serialize, Deserialize, derivative::Derivative)]
#[derivative(Debug, PartialEq)]
pub struct EventLog<Tr: Transaction> {
    /// Count of recorded log entries
    pub recorded_entries: u64,

    /// Whether we're currently replaying events
    pub replaying: bool,

    /// Table definition for storing log entries (action + effects)
    #[serde(skip)]
    #[serde(default = "default_log_table")]
    #[serde(bound(serialize = "Tr: Transaction", deserialize = "Tr: Transaction"))]
    #[derivative(Debug = "ignore", PartialEq = "ignore")]
    pub log_table: Option<TableDefinition<'static, u64, Postcard<LogEntry<Tr>>>>,

    /// Table definition for storing snapshots
    /// Note: We can't store the full type parameters here, so we'll need to handle serialization differently
    #[serde(skip)]
    #[serde(default = "default_snapshots_table_none")]
    #[derivative(Debug = "ignore", PartialEq = "ignore")]
    pub snapshots_table: Option<TableDefinition<'static, u64, Vec<u8>>>,
}

fn default_log_table<Tr: Transaction>(
) -> Option<TableDefinition<'static, u64, Postcard<LogEntry<Tr>>>> {
    Some(TableDefinition::new("event_log"))
}

fn default_snapshots_table_none() -> Option<TableDefinition<'static, u64, Vec<u8>>> {
    Some(TableDefinition::new("snapshots"))
}

pub fn default_snapshots_table<Tr: Transaction>() -> Option<TableDefinition<'static, u64, Vec<u8>>>
{
    Some(TableDefinition::new("snapshots"))
}

impl<Tr: Transaction> EventLog<Tr> {
    pub fn new(db: &Database) -> Self {
        let log_table = default_log_table::<Tr>().unwrap();

        // Ensure the log table exists
        let tx = db.begin_write().unwrap();
        {
            tx.open_table(log_table).unwrap();
        }
        tx.commit().unwrap();

        Self {
            recorded_entries: 0,
            replaying: false,
            log_table: Some(log_table),
            snapshots_table: default_snapshots_table_none(),
        }
    }

    /// Start replay mode
    pub fn start_replay(&mut self) {
        self.replaying = true;
    }

    /// End replay mode
    pub fn end_replay(&mut self) {
        self.replaying = false;
    }

    /// Record a log entry (action + effects) to the log
    pub fn record_entry(&mut self, db: &Database, entry: LogEntry<Tr>) {
        if self.replaying {
            self.recorded_entries += 1;
            return;
        }

        let tx = db.begin_write().unwrap();
        {
            let mut tbl = tx.open_table(self.log_table.unwrap()).unwrap();
            let processed_entries = tbl.len().unwrap();
            tbl.insert(processed_entries, &entry).unwrap();
            self.recorded_entries = processed_entries + 1;
        }
        tx.commit().unwrap();
    }

    /// Get the total number of recorded log entries
    pub fn entry_count(&self, db: &Database) -> u64 {
        let tx = db.begin_read().unwrap();
        let tbl = tx.open_table(self.log_table.unwrap()).unwrap();
        tbl.len().unwrap()
    }

    /// Replay log entries from a given index
    pub fn replay_entries_from<F>(
        &mut self,
        db: &Database,
        start_index: u64,
        mut process_fn: F,
    ) -> Result<(), String>
    where
        F: FnMut(u64, LogEntry<Tr>) -> Result<(), String>,
    {
        self.start_replay();

        let tx = db.begin_read().unwrap();
        let tbl = tx.open_table(self.log_table.unwrap()).unwrap();

        let range = tbl.range(start_index..).map_err(|e| {
            self.end_replay();
            format!("Error creating range: {:?}", e)
        })?;

        for result in range {
            match result {
                Ok((index, entry)) => {
                    process_fn(index.value(), entry.value())?;
                }
                Err(e) => {
                    self.end_replay();
                    return Err(format!("Error reading log entry: {:?}", e));
                }
            }
        }

        self.end_replay();
        Ok(())
    }

    /// Save a snapshot (using serialization)
    pub fn save_snapshot<B: BulkStore<Tr>, S: SnapshotStore>(
        &self,
        db: &Database,
        process: &MorpheusProcess<Tr, B, S>,
    ) -> Result<u64, String> {
        let tx = db.begin_write().unwrap();
        let snapshot_index = self.recorded_entries;

        {
            let mut tbl = tx
                .open_table(self.snapshots_table.unwrap())
                .map_err(|e| format!("Failed to open snapshots table: {:?}", e))?;

            // Convert to serializable snapshot
            let snapshot = process.to_snapshot();
            let serialized = postcard::to_stdvec(&snapshot)
                .map_err(|e| format!("Failed to serialize snapshot: {:?}", e))?;

            tbl.insert(snapshot_index, &serialized)
                .map_err(|e| format!("Failed to insert snapshot: {:?}", e))?;
        }

        tx.commit()
            .map_err(|e| format!("Failed to commit snapshot: {:?}", e))?;

        Ok(snapshot_index)
    }

    /// Load the latest snapshot before a given event index
    pub fn load_snapshot_before<B: BulkStore<Tr>, S: SnapshotStore>(
        &self,
        db: &Database,
        before_index: u64,
        bulk_store: B,
        snapshot_store: S,
        invariant_check_config: Option<crate::storage::InvariantCheckConfig>,
    ) -> Result<Option<(u64, MorpheusProcess<Tr, B, S>)>, String> {
        let tx = db.begin_read().unwrap();
        let tbl = tx
            .open_table(self.snapshots_table.unwrap())
            .map_err(|e| format!("Failed to open snapshots table: {:?}", e))?;

        // Find the latest snapshot before the given index
        let mut latest_snapshot = None;

        let iter = tbl
            .iter()
            .map_err(|e| format!("Failed to create iterator: {:?}", e))?;

        for result in iter {
            match result {
                Ok((index, snapshot_bytes)) => {
                    if index.value() < before_index {
                        // Deserialize the snapshot
                        let snapshot: ProcessSnapshot<Tr> =
                            postcard::from_bytes(&snapshot_bytes.value())
                                .map_err(|e| format!("Failed to deserialize snapshot: {:?}", e))?;
                        latest_snapshot = Some((index.value(), snapshot));
                    } else {
                        break;
                    }
                }
                Err(e) => return Err(format!("Error reading snapshot: {:?}", e)),
            }
        }

        // Convert snapshot back to process if we found one
        if let Some((index, snapshot)) = latest_snapshot {
            let process = MorpheusProcess::from_snapshot(
                snapshot,
                bulk_store,
                snapshot_store,
                invariant_check_config,
            );
            Ok(Some((index, process)))
        } else {
            Ok(None)
        }
    }

    /// Clean up old events and snapshots
    pub fn cleanup_before(&self, db: &Database, keep_after: u64) -> Result<(), String> {
        let tx = db.begin_write().unwrap();

        // Remove old log entries
        {
            let mut log_tbl = tx
                .open_table(self.log_table.unwrap())
                .map_err(|e| format!("Failed to open log table: {:?}", e))?;

            let mut to_remove = Vec::new();
            let range = log_tbl
                .range(..keep_after)
                .map_err(|e| format!("Failed to create range: {:?}", e))?;

            for result in range {
                match result {
                    Ok((index, _)) => to_remove.push(index.value()),
                    Err(e) => return Err(format!("Error reading log entry: {:?}", e)),
                }
            }

            for index in to_remove {
                log_tbl
                    .remove(index)
                    .map_err(|e| format!("Failed to remove log entry: {:?}", e))?;
            }
        }

        // Remove old snapshots
        {
            let mut snapshots_tbl = tx
                .open_table(self.snapshots_table.unwrap())
                .map_err(|e| format!("Failed to open snapshots table: {:?}", e))?;

            let mut to_remove = Vec::new();
            let range = snapshots_tbl
                .range(..keep_after)
                .map_err(|e| format!("Failed to create range: {:?}", e))?;

            for result in range {
                match result {
                    Ok((index, _)) => to_remove.push(index.value()),
                    Err(e) => return Err(format!("Error reading snapshot: {:?}", e)),
                }
            }

            for index in to_remove {
                snapshots_tbl
                    .remove(index)
                    .map_err(|e| format!("Failed to remove snapshot: {:?}", e))?;
            }
        }

        tx.commit()
            .map_err(|e| format!("Failed to commit cleanup: {:?}", e))?;

        Ok(())
    }
}
