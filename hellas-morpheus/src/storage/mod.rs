//! Storage V3: Pure persistence layer for the ProcessState architecture
//!
//! This module provides:
//! - Content-addressed bulk storage for deduplication
//! - Event journal for deterministic replay
//! - Efficient snapshots that only store essential (non-derived) state
//! - Clean separation between persistence and state management

use crate::state::ProcessState;
use crate::*;
use redb::{Database, ReadableTable};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// Submodules
pub(crate) mod bulk_store;
pub(crate) mod event_journal;
pub(crate) mod serialization;
pub(crate) mod tables;

// Re-exports
pub use bulk_store::{BlockRef, BulkStore, ContentHash, ObjectRef};
pub use event_journal::{EventJournal, JournalEntry};
pub use serialization::*;
pub use tables::{Tables};

/// The main storage struct - now a pure persistence layer
#[derive(Clone)]
pub struct Storage<Tr: Transaction> {
    /// The underlying redb database
    db: Arc<Database>,

    /// Bulk store for content-addressed storage
    pub bulk: BulkStore,

    /// Event journal for deterministic replay
    pub journal: EventJournal<Tr>,
}

impl<Tr: Transaction> Storage<Tr> {
    /// Create a new storage instance
    pub fn new(
        db: Arc<Database>,
        genesis_block: Arc<Signed<Block<Tr>>>,
        genesis_qc: FinishedQC,
    ) -> Result<Self, String> {
        // Initialize tables
        Tables::ensure_created(&db)?;

        // Create components
        let bulk = BulkStore::new(db.clone());
        let journal = EventJournal::new(db.clone());

        let mut storage = Self { db, bulk, journal };

        // Store genesis objects
        storage.store_genesis(genesis_block, genesis_qc)?;

        Ok(storage)
    }

    /// Store the genesis block and QC
    fn store_genesis(
        &mut self,
        genesis_block: Arc<Signed<Block<Tr>>>,
        genesis_qc: FinishedQC,
    ) -> Result<(), String> {
        // Store genesis block
        let (_, block_ref) = self.bulk.store_block(&genesis_block)?;
        
        // Store genesis QC
        let qc_ref = self.bulk.store_qc(&genesis_qc)?;

        // Create block index entry
        let tx = self
            .db
            .begin_write()
            .map_err(|e| format!("Failed to begin write transaction: {:?}", e))?;

        {
            let mut block_index = tx
                .open_table(tables::BLOCK_INDEX_TABLE)
                .map_err(|e| format!("Failed to open block index table: {:?}", e))?;
            
            block_index
                .insert(&GEN_BLOCK_KEY, &block_ref)
                .map_err(|e| format!("Failed to insert genesis block index: {:?}", e))?;
        }

        {
            let mut qc_index = tx
                .open_table(tables::QC_INDEX_TABLE)
                .map_err(|e| format!("Failed to open QC index table: {:?}", e))?;
            
            qc_index
                .insert(&genesis_qc.data, &qc_ref)
                .map_err(|e| format!("Failed to insert genesis QC index: {:?}", e))?;
        }

        tx.commit()
            .map_err(|e| format!("Failed to commit transaction: {:?}", e))?;

        Ok(())
    }

    /// Store a block from ProcessState
    pub fn store_block(&mut self, block: &Arc<Signed<Block<Tr>>>) -> Result<(), String> {
        // Store in bulk storage
        let (_, block_ref) = self.bulk.store_block(block)?;

        // Update indices
        let tx = self
            .db
            .begin_write()
            .map_err(|e| format!("Failed to begin write transaction: {:?}", e))?;

        // Update block index
        {
            let mut block_index = tx
                .open_table(tables::BLOCK_INDEX_TABLE)
                .map_err(|e| format!("Failed to open block index table: {:?}", e))?;
            
            block_index
                .insert(&block.data.key, &block_ref)
                .map_err(|e| format!("Failed to insert block index: {:?}", e))?;
        }

        // Update view-based index
        {
            let mut view_blocks = tx
                .open_table(tables::VIEW_BLOCKS_TABLE)
                .map_err(|e| format!("Failed to open view blocks table: {:?}", e))?;
            
            let mut blocks_in_view = view_blocks
                .get(&block.data.key.view)
                .map_err(|e| format!("Failed to get view blocks: {:?}", e))?
                .map(|v| v.value())
                .unwrap_or_default();
            
            blocks_in_view.push(block.data.key.clone());
            
            view_blocks
                .insert(&block.data.key.view, &blocks_in_view)
                .map_err(|e| format!("Failed to insert view blocks: {:?}", e))?;
        }

        tx.commit()
            .map_err(|e| format!("Failed to commit transaction: {:?}", e))?;

        Ok(())
    }

    /// Store a QC from ProcessState
    pub fn store_qc(&mut self, qc: &FinishedQC) -> Result<(), String> {
        // Store in bulk storage
        let qc_ref = self.bulk.store_qc(qc)?;

        // Update QC index
        let tx = self
            .db
            .begin_write()
            .map_err(|e| format!("Failed to begin write transaction: {:?}", e))?;

        {
            let mut qc_index = tx
                .open_table(tables::QC_INDEX_TABLE)
                .map_err(|e| format!("Failed to open QC index table: {:?}", e))?;
            
            qc_index
                .insert(&qc.data, &qc_ref)
                .map_err(|e| format!("Failed to insert QC index: {:?}", e))?;
        }

        tx.commit()
            .map_err(|e| format!("Failed to commit transaction: {:?}", e))?;

        Ok(())
    }

    /// Get a block by key
    pub fn get_block(&self, key: &BlockKey) -> Result<Option<Arc<Signed<Block<Tr>>>>, String> {
        let tx = self
            .db
            .begin_read()
            .map_err(|e| format!("Failed to begin read transaction: {:?}", e))?;

        let block_index = tx
            .open_table(tables::BLOCK_INDEX_TABLE)
            .map_err(|e| format!("Failed to open block index table: {:?}", e))?;

        match block_index.get(key).map_err(|e| format!("Failed to get block index: {:?}", e))? {
            Some(block_ref) => self.bulk.get_block(&block_ref.value()),
            None => Ok(None),
        }
    }

    /// Get a QC by vote data
    pub fn get_qc(&self, vote_data: &VoteData) -> Result<Option<FinishedQC>, String> {
        let tx = self
            .db
            .begin_read()
            .map_err(|e| format!("Failed to begin read transaction: {:?}", e))?;

        let qc_index = tx
            .open_table(tables::QC_INDEX_TABLE)
            .map_err(|e| format!("Failed to open QC index table: {:?}", e))?;

        match qc_index.get(vote_data).map_err(|e| format!("Failed to get QC index: {:?}", e))? {
            Some(qc_ref) => self.bulk.get_qc(&qc_ref.value()),
            None => Ok(None),
        }
    }

    /// Get all blocks in a view
    pub fn get_blocks_in_view(&self, view: ViewNum) -> Result<Vec<BlockKey>, String> {
        let tx = self
            .db
            .begin_read()
            .map_err(|e| format!("Failed to begin read transaction: {:?}", e))?;

        let view_blocks = tx
            .open_table(tables::VIEW_BLOCKS_TABLE)
            .map_err(|e| format!("Failed to open view blocks table: {:?}", e))?;

        Ok(view_blocks
            .get(&view)
            .map_err(|e| format!("Failed to get view blocks: {:?}", e))?
            .map(|v| v.value())
            .unwrap_or_default())
    }
}

/// Storage checkpoint - minimal data needed for recovery
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct StorageCheckpoint {
    pub snapshot_id: u64,
    pub event_count: u64,
} 