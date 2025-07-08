//! Bulk store - content-addressed storage for protocol objects
//! 
//! This provides a "persistent heap" where objects are stored by their content hash,
//! enabling automatic deduplication and efficient storage.

use crate::serialization::Postcard;
use crate::*;
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Content hash for deduplication
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct ContentHash([u8; 32]);

impl ContentHash {
    /// Compute content hash for any serializable object
    pub fn of<T: Serialize>(obj: &T) -> Self {
        let bytes = postcard::to_stdvec(obj).expect("Serialization should not fail");
        let hash = blake3::hash(&bytes);
        Self(*hash.as_bytes())
    }
}

/// Reference to an object in bulk storage
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub enum ObjectRef {
    /// Direct reference by content hash
    Hash(ContentHash),
    /// Special reference for genesis
    Genesis,
}

/// Bulk storage tables
pub const BLOCK_DATA_TABLE: TableDefinition<'static, Postcard<ContentHash>, Vec<u8>> = 
    TableDefinition::new("bulk_block_data");

pub const QC_DATA_TABLE: TableDefinition<'static, Postcard<ContentHash>, Postcard<FinishedQC>> = 
    TableDefinition::new("bulk_qc_data");

pub const VOTE_DATA_TABLE: TableDefinition<'static, Postcard<ContentHash>, Postcard<Arc<ThreshPartial<VoteData>>>> = 
    TableDefinition::new("bulk_vote_data");

/// Lightweight reference to a block (for storage)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlockRef {
    pub key: BlockKey,
    pub prev: Vec<ObjectRef>,  // References to QCs
    pub one: ObjectRef,        // Reference to 1-QC
    pub data: ObjectRef,       // Reference to block data
    pub author: Identity,
    pub signature: hints::PartialSignature,
}

/// Bulk store for content-addressed storage
#[derive(Clone)]
pub struct BulkStore {
    db: Arc<Database>,
}

impl BulkStore {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Store a block and return its reference
    pub fn store_block<Tr: Transaction>(
        &self,
        block: &Arc<Signed<Block<Tr>>>,
    ) -> Result<(ObjectRef, BlockRef), String> {
        let tx = self
            .db
            .begin_write()
            .map_err(|e| format!("Failed to begin write transaction: {:?}", e))?;

        // Store block data
        let block_data_bytes = postcard::to_stdvec(&block.data.data)
            .map_err(|e| format!("Failed to serialize block data: {:?}", e))?;
        let data_hash = ContentHash(blake3::hash(&block_data_bytes).into());
        let data_ref = ObjectRef::Hash(data_hash);

        {
            let mut table = tx
                .open_table(BLOCK_DATA_TABLE)
                .map_err(|e| format!("Failed to open block data table: {:?}", e))?;
            
            // Only insert if not already present (deduplication)
            if table.get(&data_hash).map_err(|e| format!("Failed to check block data: {:?}", e))?.is_none() {
                table
                    .insert(&data_hash, &block_data_bytes)
                    .map_err(|e| format!("Failed to insert block data: {:?}", e))?;
            }
        }

        // Store QCs referenced by the block
        let mut prev_refs = Vec::new();
        for qc in &block.data.prev {
            let qc_ref = self.store_qc_in_tx(&tx, qc)?;
            prev_refs.push(qc_ref);
        }

        let one_ref = self.store_qc_in_tx(&tx, &block.data.one)?;

        tx.commit()
            .map_err(|e| format!("Failed to commit transaction: {:?}", e))?;

        // Create block reference
        let block_ref = BlockRef {
            key: block.data.key.clone(),
            prev: prev_refs,
            one: one_ref,
            data: data_ref,
            author: block.author.clone(),
            signature: block.signature.clone(),
        };

        // The block itself is referenced by its key hash
        let block_hash = ContentHash::of(&block.data.key);
        Ok((ObjectRef::Hash(block_hash), block_ref))
    }

    /// Store a QC and return its reference
    pub fn store_qc(&self, qc: &FinishedQC) -> Result<ObjectRef, String> {
        let tx = self
            .db
            .begin_write()
            .map_err(|e| format!("Failed to begin write transaction: {:?}", e))?;

        let qc_ref = self.store_qc_in_tx(&tx, qc)?;

        tx.commit()
            .map_err(|e| format!("Failed to commit transaction: {:?}", e))?;

        Ok(qc_ref)
    }

    /// Internal: Store QC within a transaction
    fn store_qc_in_tx(
        &self,
        tx: &redb::WriteTransaction,
        qc: &FinishedQC,
    ) -> Result<ObjectRef, String> {
        // Genesis QC is special
        if qc.data.for_which == GEN_BLOCK_KEY {
            return Ok(ObjectRef::Genesis);
        }

        let qc_hash = ContentHash::of(qc);
        let mut table = tx
            .open_table(QC_DATA_TABLE)
            .map_err(|e| format!("Failed to open QC table: {:?}", e))?;

        // Only insert if not already present
        if table.get(&qc_hash).map_err(|e| format!("Failed to check QC: {:?}", e))?.is_none() {
            table
                .insert(&qc_hash, qc)
                .map_err(|e| format!("Failed to insert QC: {:?}", e))?;
        }

        Ok(ObjectRef::Hash(qc_hash))
    }

    /// Store a vote
    pub fn store_vote(
        &self,
        vote: &Arc<ThreshPartial<VoteData>>,
    ) -> Result<ObjectRef, String> {
        let vote_hash = ContentHash::of(vote);

        let tx = self
            .db
            .begin_write()
            .map_err(|e| format!("Failed to begin write transaction: {:?}", e))?;

        {
            let mut table = tx
                .open_table(VOTE_DATA_TABLE)
                .map_err(|e| format!("Failed to open vote table: {:?}", e))?;

            if table.get(&vote_hash).map_err(|e| format!("Failed to check vote: {:?}", e))?.is_none() {
                table
                    .insert(&vote_hash, vote)
                    .map_err(|e| format!("Failed to insert vote: {:?}", e))?;
            }
        }

        tx.commit()
            .map_err(|e| format!("Failed to commit transaction: {:?}", e))?;

        Ok(ObjectRef::Hash(vote_hash))
    }

    /// Retrieve a block by its reference
    pub fn get_block<Tr: Transaction>(
        &self,
        block_ref: &BlockRef,
    ) -> Result<Option<Arc<Signed<Block<Tr>>>>, String> {
        let tx = self
            .db
            .begin_read()
            .map_err(|e| format!("Failed to begin read transaction: {:?}", e))?;

        // Retrieve block data
        let block_data = match &block_ref.data {
            ObjectRef::Hash(hash) => {
                let table = tx
                    .open_table(BLOCK_DATA_TABLE)
                    .map_err(|e| format!("Failed to open block data table: {:?}", e))?;
                
                match table.get(hash).map_err(|e| format!("Failed to get block data: {:?}", e))? {
                    Some(data) => {
                        let block_data: BlockData<Tr> = postcard::from_bytes(&data.value())
                            .map_err(|e| format!("Failed to deserialize block data: {:?}", e))?;
                        block_data
                    }
                    None => return Ok(None),
                }
            }
            ObjectRef::Genesis => BlockData::Genesis,
        };

        // Retrieve prev QCs
        let mut prev_qcs = Vec::new();
        for qc_ref in &block_ref.prev {
            if let Some(qc) = self.get_qc_in_tx(&tx, qc_ref)? {
                prev_qcs.push(qc);
            } else {
                return Ok(None);
            }
        }

        // Retrieve one QC
        let one_qc = match self.get_qc_in_tx(&tx, &block_ref.one)? {
            Some(qc) => qc,
            None => return Ok(None),
        };

        // Reconstruct the block
        let block = Block {
            key: block_ref.key.clone(),
            prev: prev_qcs,
            one: one_qc,
            data: block_data,
        };

        Ok(Some(Arc::new(Signed {
            data: block,
            author: block_ref.author.clone(),
            signature: block_ref.signature.clone(),
        })))
    }

    /// Retrieve a QC by its reference
    pub fn get_qc(&self, qc_ref: &ObjectRef) -> Result<Option<FinishedQC>, String> {
        let tx = self
            .db
            .begin_read()
            .map_err(|e| format!("Failed to begin read transaction: {:?}", e))?;

        self.get_qc_in_tx(&tx, qc_ref)
    }

    /// Internal: Get QC within a transaction
    fn get_qc_in_tx(
        &self,
        tx: &redb::ReadTransaction,
        qc_ref: &ObjectRef,
    ) -> Result<Option<FinishedQC>, String> {
        match qc_ref {
            ObjectRef::Hash(hash) => {
                let table = tx
                    .open_table(QC_DATA_TABLE)
                    .map_err(|e| format!("Failed to open QC table: {:?}", e))?;
                
                Ok(table.get(hash)
                    .map_err(|e| format!("Failed to get QC: {:?}", e))?
                    .map(|v| v.value()))
            }
            ObjectRef::Genesis => {
                // Return the standard genesis QC
                Ok(Some(Arc::new(ThreshSigned {
                    data: VoteData {
                        z: 1,
                        for_which: GEN_BLOCK_KEY,
                    },
                    signature: hints::Signature::default(),
                })))
            }
        }
    }
} 