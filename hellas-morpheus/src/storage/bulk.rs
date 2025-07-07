//! Bulk storage implementation using redb for persistent storage

use crate::serialization::Postcard;
use crate::storage::{BlockRef, BulkStore, QCRef, VoteRef};
use crate::*;
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;
use std::sync::Arc;

/// We need a marker type for type-erased blocks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErasedBlock {
    pub key: BlockKey,
    pub prev: Vec<FinishedQC>,
    pub one: FinishedQC,
    pub data: Vec<u8>, // Serialized block data
}

/// Table definitions for bulk storage
const BLOCKS_TABLE: TableDefinition<'static, Postcard<BlockKey>, Postcard<ErasedBlock>> =
    TableDefinition::new("bulk_blocks");
const QCS_TABLE: TableDefinition<'static, Postcard<VoteData>, Postcard<FinishedQC>> =
    TableDefinition::new("bulk_qcs");
const VOTES_TABLE: TableDefinition<
    'static,
    Postcard<(Identity, VoteData)>,
    Postcard<Arc<ThreshPartial<VoteData>>>,
> = TableDefinition::new("bulk_votes");
const VIEW_BLOCKS_TABLE: TableDefinition<'static, Postcard<ViewNum>, Postcard<Vec<BlockRef>>> =
    TableDefinition::new("view_blocks");
const VIEW_QCS_TABLE: TableDefinition<'static, Postcard<ViewNum>, Postcard<Vec<QCRef>>> =
    TableDefinition::new("view_qcs");

/// Redb-based bulk storage implementation
#[derive(Debug, Clone)]
pub struct RedbBulkStore<Tr: Transaction> {
    db: Arc<Database>,
    _marker: PhantomData<Tr>,
}

impl<Tr: Transaction> RedbBulkStore<Tr> {
    /// Create a new bulk store with the given database
    pub fn new(db: Arc<Database>) -> Result<Self, String> {
        // Ensure tables exist
        let tx = db
            .begin_write()
            .map_err(|e| format!("Failed to begin write transaction: {:?}", e))?;

        tx.open_table(BLOCKS_TABLE)
            .map_err(|e| format!("Failed to open blocks table: {:?}", e))?;
        tx.open_table(QCS_TABLE)
            .map_err(|e| format!("Failed to open QCs table: {:?}", e))?;
        tx.open_table(VOTES_TABLE)
            .map_err(|e| format!("Failed to open votes table: {:?}", e))?;
        tx.open_table(VIEW_BLOCKS_TABLE)
            .map_err(|e| format!("Failed to open view blocks table: {:?}", e))?;
        tx.open_table(VIEW_QCS_TABLE)
            .map_err(|e| format!("Failed to open view QCs table: {:?}", e))?;

        tx.commit()
            .map_err(|e| format!("Failed to commit transaction: {:?}", e))?;

        Ok(Self {
            db,
            _marker: PhantomData,
        })
    }
}

impl<Tr: Transaction> BulkStore<Tr> for RedbBulkStore<Tr> {
    fn append_block(&mut self, block: Arc<Signed<Block<Tr>>>) -> Result<BlockRef, String> {
        let tx = self
            .db
            .begin_write()
            .map_err(|e| format!("Failed to begin write transaction: {:?}", e))?;

        let block_ref = BlockRef {
            key: block.data.key.clone(),
            hash: block.data.key.hash.clone(),
        };

        // Serialize the block data field only
        let serialized_data = postcard::to_stdvec(&block.data.data)
            .map_err(|e| format!("Failed to serialize block data: {:?}", e))?;

        // Store the block as an erased version
        let erased_block = ErasedBlock {
            key: block.data.key.clone(),
            prev: block.data.prev.clone(),
            one: block.data.one.clone(),
            data: serialized_data,
        };

        {
            let mut blocks_table = tx
                .open_table(BLOCKS_TABLE)
                .map_err(|e| format!("Failed to open blocks table: {:?}", e))?;

            blocks_table
                .insert(&block.data.key, &erased_block)
                .map_err(|e| format!("Failed to insert block: {:?}", e))?;
        }

        // Update view index
        {
            let mut view_blocks_table = tx
                .open_table(VIEW_BLOCKS_TABLE)
                .map_err(|e| format!("Failed to open view blocks table: {:?}", e))?;

            let mut blocks_in_view = view_blocks_table
                .get(&block.data.key.view)
                .map_err(|e| format!("Failed to get view blocks: {:?}", e))?
                .map(|v| v.value())
                .unwrap_or_default();

            if !blocks_in_view.contains(&block_ref) {
                blocks_in_view.push(block_ref.clone());
                view_blocks_table
                    .insert(&block.data.key.view, &blocks_in_view)
                    .map_err(|e| format!("Failed to update view blocks: {:?}", e))?;
            }
        }

        tx.commit()
            .map_err(|e| format!("Failed to commit transaction: {:?}", e))?;

        Ok(block_ref)
    }

    fn append_qc(&mut self, qc: FinishedQC) -> Result<QCRef, String> {
        let tx = self
            .db
            .begin_write()
            .map_err(|e| format!("Failed to begin write transaction: {:?}", e))?;

        let qc_ref = QCRef {
            vote_data: qc.data.clone(),
            hash: None, // TODO: compute hash if needed
        };

        {
            let mut qcs_table = tx
                .open_table(QCS_TABLE)
                .map_err(|e| format!("Failed to open QCs table: {:?}", e))?;

            qcs_table
                .insert(&qc.data, &qc)
                .map_err(|e| format!("Failed to insert QC: {:?}", e))?;
        }

        // Update view index
        {
            let mut view_qcs_table = tx
                .open_table(VIEW_QCS_TABLE)
                .map_err(|e| format!("Failed to open view QCs table: {:?}", e))?;

            let mut qcs_in_view = view_qcs_table
                .get(&qc.data.for_which.view)
                .map_err(|e| format!("Failed to get view QCs: {:?}", e))?
                .map(|v| v.value())
                .unwrap_or_default();

            if !qcs_in_view.contains(&qc_ref) {
                qcs_in_view.push(qc_ref.clone());
                view_qcs_table
                    .insert(&qc.data.for_which.view, &qcs_in_view)
                    .map_err(|e| format!("Failed to update view QCs: {:?}", e))?;
            }
        }

        tx.commit()
            .map_err(|e| format!("Failed to commit transaction: {:?}", e))?;

        Ok(qc_ref)
    }

    fn append_vote(&mut self, vote: Arc<ThreshPartial<VoteData>>) -> Result<VoteRef, String> {
        let tx = self
            .db
            .begin_write()
            .map_err(|e| format!("Failed to begin write transaction: {:?}", e))?;

        let vote_ref = VoteRef {
            voter: vote.author.clone(),
            vote_data: vote.data.clone(),
            hash: None, // TODO: compute hash if needed
        };

        {
            let mut votes_table = tx
                .open_table(VOTES_TABLE)
                .map_err(|e| format!("Failed to open votes table: {:?}", e))?;

            votes_table
                .insert(&(vote.author.clone(), vote.data.clone()), &vote)
                .map_err(|e| format!("Failed to insert vote: {:?}", e))?;
        }

        tx.commit()
            .map_err(|e| format!("Failed to commit transaction: {:?}", e))?;

        Ok(vote_ref)
    }

    fn get_block(&self, block_ref: &BlockRef) -> Result<Option<Arc<Signed<Block<Tr>>>>, String> {
        let tx = self
            .db
            .begin_read()
            .map_err(|e| format!("Failed to begin read transaction: {:?}", e))?;

        let blocks_table = tx
            .open_table(BLOCKS_TABLE)
            .map_err(|e| format!("Failed to open blocks table: {:?}", e))?;

        let erased_block_opt = blocks_table
            .get(&block_ref.key)
            .map_err(|e| format!("Failed to get block: {:?}", e))?
            .map(|v| v.value());

        // Reconstruct the block
        match erased_block_opt {
            Some(erased) => {
                // Deserialize the block data
                let block_data: BlockData<Tr> = postcard::from_bytes(&erased.data)
                    .map_err(|e| format!("Failed to deserialize block data: {:?}", e))?;

                let block = Block {
                    key: erased.key.clone(),
                    prev: erased.prev.clone(),
                    one: erased.one.clone(),
                    data: block_data,
                };

                // We need to get the author and signature from the key
                let author = erased.key.author.clone().unwrap_or(Identity(u32::MAX));

                // Create signed block (signature will be invalid but we don't verify it)
                let signed_block = Arc::new(Signed {
                    data: block,
                    author,
                    signature: hints::PartialSignature::default(),
                });

                Ok(Some(signed_block))
            }
            None => Ok(None),
        }
    }

    fn get_qc(&self, qc_ref: &QCRef) -> Result<Option<FinishedQC>, String> {
        let tx = self
            .db
            .begin_read()
            .map_err(|e| format!("Failed to begin read transaction: {:?}", e))?;

        let qcs_table = tx
            .open_table(QCS_TABLE)
            .map_err(|e| format!("Failed to open QCs table: {:?}", e))?;

        Ok(qcs_table
            .get(&qc_ref.vote_data)
            .map_err(|e| format!("Failed to get QC: {:?}", e))?
            .map(|v| v.value()))
    }

    fn get_vote(&self, vote_ref: &VoteRef) -> Result<Option<Arc<ThreshPartial<VoteData>>>, String> {
        let tx = self
            .db
            .begin_read()
            .map_err(|e| format!("Failed to begin read transaction: {:?}", e))?;

        let votes_table = tx
            .open_table(VOTES_TABLE)
            .map_err(|e| format!("Failed to open votes table: {:?}", e))?;

        Ok(votes_table
            .get(&(vote_ref.voter.clone(), vote_ref.vote_data.clone()))
            .map_err(|e| format!("Failed to get vote: {:?}", e))?
            .map(|v| v.value()))
    }

    fn get_blocks_in_view(&self, view: ViewNum) -> Result<Vec<BlockRef>, String> {
        let tx = self
            .db
            .begin_read()
            .map_err(|e| format!("Failed to begin read transaction: {:?}", e))?;

        let view_blocks_table = tx
            .open_table(VIEW_BLOCKS_TABLE)
            .map_err(|e| format!("Failed to open view blocks table: {:?}", e))?;

        Ok(view_blocks_table
            .get(&view)
            .map_err(|e| format!("Failed to get view blocks: {:?}", e))?
            .map(|v| v.value())
            .unwrap_or_default())
    }

    fn get_qcs_in_view(&self, view: ViewNum) -> Result<Vec<QCRef>, String> {
        let tx = self
            .db
            .begin_read()
            .map_err(|e| format!("Failed to begin read transaction: {:?}", e))?;

        let view_qcs_table = tx
            .open_table(VIEW_QCS_TABLE)
            .map_err(|e| format!("Failed to open view QCs table: {:?}", e))?;

        Ok(view_qcs_table
            .get(&view)
            .map_err(|e| format!("Failed to get view QCs: {:?}", e))?
            .map(|v| v.value())
            .unwrap_or_default())
    }
}
