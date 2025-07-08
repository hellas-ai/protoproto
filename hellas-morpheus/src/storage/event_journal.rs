//! Simplified event journal for deterministic replay
//!
//! Unlike the old approach, this doesn't duplicate functionality with snapshots.
//! It's purely for recording events for replay.

use crate::logic::actions::Action;
use crate::logic::effects::Effect;
use crate::serialization::Postcard;
use crate::*;
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

fn default_table<Tr: Transaction>() -> TableDefinition<'static, u64, Postcard<JournalEntry<Tr>>> {
    TableDefinition::new("event_journal_v2")
}

/// A single entry in the event journal
#[derive(derivative::Derivative, Clone, Serialize, Deserialize)]
#[derivative(PartialEq, Debug)]
pub struct JournalEntry<Tr: Transaction> {
    /// The action that was taken
    #[serde(bound(serialize = "Tr: Transaction", deserialize = "Tr: Transaction"))]
    pub action: Action<Tr>,

    /// The effects that resulted
    #[serde(bound(serialize = "Tr: Transaction", deserialize = "Tr: Transaction"))]
    pub effects: Vec<Effect<Tr>>,

    /// Timestamp for debugging
    pub timestamp: u128,
}

/// Simple event journal that just records actions and effects
#[derive(Clone, derivative::Derivative)]
#[derivative(Debug)]
pub struct EventJournal<Tr: Transaction> {
    db: Arc<Database>,
    /// Current position in the journal
    pub position: u64,
    /// Whether we're currently replaying
    pub replaying: bool,

    /// Table definition for the event journal
    #[derivative(Debug = "ignore")]
    pub table: TableDefinition<'static, u64, Postcard<JournalEntry<Tr>>>,
    _phantom: std::marker::PhantomData<Tr>,
}

impl<Tr: Transaction> EventJournal<Tr> {
    /// Create a new event journal
    pub fn new(db: Arc<Database>) -> Self {
        // Ensure table exists
        if let Ok(tx) = db.begin_write() {
            let _ = tx.open_table(default_table::<Tr>());
            let _ = tx.commit();
        }

        Self {
            db,
            position: 0,
            replaying: false,
            table: default_table::<Tr>(),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Record an action and its effects
    pub fn record(
        &mut self,
        action: Action<Tr>,
        effects: Vec<Effect<Tr>>,
        timestamp: u128,
    ) -> Result<(), String> {
        if self.replaying {
            self.position += 1;
            return Ok(());
        }

        let entry = JournalEntry {
            action,
            effects,
            timestamp,
        };

        let tx = self
            .db
            .begin_write()
            .map_err(|e| format!("Failed to begin write transaction: {:?}", e))?;
        {
            let mut table = tx
                .open_table(self.table)
                .map_err(|e| format!("Failed to open event journal table: {:?}", e))?;
            table
                .insert(self.position, &entry)
                .map_err(|e| format!("Failed to insert journal entry: {:?}", e))?;
        }
        tx.commit()
            .map_err(|e| format!("Failed to commit transaction: {:?}", e))?;

        self.position += 1;
        Ok(())
    }

    /// Get the current event count
    pub fn event_count(&self) -> u64 {
        self.position
    }

    /// Replay events from a given position
    pub fn replay_from<F>(&mut self, start_position: u64, mut callback: F) -> Result<(), String>
    where
        F: FnMut(u64, JournalEntry<Tr>) -> Result<(), String>,
    {
        self.replaying = true;
        let result = self.replay_from_inner(start_position, &mut callback);
        self.replaying = false;
        result
    }

    fn replay_from_inner<F>(&mut self, start_position: u64, callback: &mut F) -> Result<(), String>
    where
        F: FnMut(u64, JournalEntry<Tr>) -> Result<(), String>,
    {
        let tx = self
            .db
            .begin_read()
            .map_err(|e| format!("Failed to begin read transaction: {:?}", e))?;
        let table = tx
            .open_table(self.table)
            .map_err(|e| format!("Failed to open event journal table: {:?}", e))?;

        let range = table
            .range(start_position..)
            .map_err(|e| format!("Failed to create range: {:?}", e))?;

        for result in range {
            match result {
                Ok((position, entry)) => {
                    let entry: JournalEntry<Tr> = entry.value();
                    callback(position.value(), entry)?;
                    self.position = position.value() + 1;
                }
                Err(e) => return Err(format!("Error reading journal entry: {:?}", e)),
            }
        }

        Ok(())
    }

    /// Get a specific entry
    pub fn get_entry(&self, position: u64) -> Result<Option<JournalEntry<Tr>>, String> {
        let tx = self
            .db
            .begin_read()
            .map_err(|e| format!("Failed to begin read transaction: {:?}", e))?;
        let table = tx
            .open_table(self.table)
            .map_err(|e| format!("Failed to open event journal table: {:?}", e))?;

        match table.get(position) {
            Ok(Some(entry)) => {
                let entry: JournalEntry<Tr> = entry.value();
                Ok(Some(entry))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(format!("Error reading journal entry: {:?}", e)),
        }
    }

    /// Truncate the journal at a specific position (for cleanup)
    pub fn truncate_before(&mut self, keep_after: u64) -> Result<(), String> {
        let tx = self
            .db
            .begin_write()
            .map_err(|e| format!("Failed to begin write transaction: {:?}", e))?;
        {
            let mut table = tx
                .open_table(self.table)
                .map_err(|e| format!("Failed to open event journal table: {:?}", e))?;

            let mut to_remove = Vec::new();
            let range = table
                .range(..keep_after)
                .map_err(|e| format!("Failed to create range: {:?}", e))?;

            for result in range {
                match result {
                    Ok((position, _)) => to_remove.push(position.value()),
                    Err(e) => return Err(format!("Error reading journal entry: {:?}", e)),
                }
            }

            for position in to_remove {
                table
                    .remove(position)
                    .map_err(|e| format!("Failed to remove journal entry: {:?}", e))?;
            }
        }
        tx.commit()
            .map_err(|e| format!("Failed to commit transaction: {:?}", e))?;

        Ok(())
    }
} 