//! Table definitions for the redesigned storage system
//! 
//! This defines all database tables used by the storage layer

use crate::serialization::Postcard;
use crate::storage::bulk_store::{BlockRef, ContentHash, ObjectRef};
use crate::*;
use redb::{Database, TableDefinition};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// === Bulk Storage Tables (Content-Addressed) ===

/// Block data storage (content-addressed)
pub const BLOCK_DATA_TABLE: TableDefinition<'static, Postcard<ContentHash>, Vec<u8>> = 
    TableDefinition::new("bulk_block_data_v3");

/// QC storage (content-addressed)
pub const QC_DATA_TABLE: TableDefinition<'static, Postcard<ContentHash>, Postcard<FinishedQC>> = 
    TableDefinition::new("bulk_qc_data_v3");

/// Vote storage (content-addressed)
pub const VOTE_DATA_TABLE: TableDefinition<'static, Postcard<ContentHash>, Postcard<Arc<ThreshPartial<VoteData>>>> = 
    TableDefinition::new("bulk_vote_data_v3");

/// Start view message storage (content-addressed)
pub const START_VIEW_TABLE: TableDefinition<'static, Postcard<ContentHash>, Postcard<Arc<Signed<StartView>>>> = 
    TableDefinition::new("bulk_start_view_data_v3");

/// End view message storage (content-addressed)
pub const END_VIEW_TABLE: TableDefinition<'static, Postcard<ContentHash>, Postcard<Arc<ThreshPartial<ViewNum>>>> = 
    TableDefinition::new("bulk_end_view_data_v3");

// === Index Tables (For Efficient Lookup) ===

/// Block key to block reference mapping
pub const BLOCK_INDEX_TABLE: TableDefinition<'static, Postcard<BlockKey>, Postcard<BlockRef>> = 
    TableDefinition::new("block_index_v3");

/// QC vote data to reference mapping
pub const QC_INDEX_TABLE: TableDefinition<'static, Postcard<VoteData>, Postcard<ObjectRef>> = 
    TableDefinition::new("qc_index_v3");

/// View number to block keys mapping (for view-based queries)
pub const VIEW_BLOCKS_TABLE: TableDefinition<'static, Postcard<ViewNum>, Postcard<Vec<BlockKey>>> = 
    TableDefinition::new("view_blocks_v3");

// === Helper to ensure all tables exist ===

pub struct Tables;

impl Tables {
    pub fn ensure_created(db: &Database) -> Result<(), String> {
        let tx = db.begin_write()
            .map_err(|e| format!("Failed to begin write transaction: {:?}", e))?;
        
        // Create bulk storage tables
        tx.open_table(BLOCK_DATA_TABLE)
            .map_err(|e| format!("Failed to create block data table: {:?}", e))?;
        tx.open_table(QC_DATA_TABLE)
            .map_err(|e| format!("Failed to create QC data table: {:?}", e))?;
        tx.open_table(VOTE_DATA_TABLE)
            .map_err(|e| format!("Failed to create vote data table: {:?}", e))?;
        tx.open_table(START_VIEW_TABLE)
            .map_err(|e| format!("Failed to create start view table: {:?}", e))?;
        tx.open_table(END_VIEW_TABLE)
            .map_err(|e| format!("Failed to create end view table: {:?}", e))?;
        
        // Create index tables
        tx.open_table(BLOCK_INDEX_TABLE)
            .map_err(|e| format!("Failed to create block index table: {:?}", e))?;
        tx.open_table(QC_INDEX_TABLE)
            .map_err(|e| format!("Failed to create QC index table: {:?}", e))?;
        tx.open_table(VIEW_BLOCKS_TABLE)
            .map_err(|e| format!("Failed to create view blocks table: {:?}", e))?;
        
        tx.commit()
            .map_err(|e| format!("Failed to commit transaction: {:?}", e))?;
        
        Ok(())
    }
} 