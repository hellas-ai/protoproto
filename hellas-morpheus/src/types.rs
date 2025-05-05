use crate::crypto::*;
use crate::format;

use ark_serialize::Valid;
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

//==============================================================================
// Core Protocol Numerical Types
//==============================================================================

/// Represents a view number in the protocol
/// 
/// Views are logical time periods in the protocol, each with a designated leader.
/// View numbers start at 0 and increment when view changes occur.
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Debug,
    Serialize,
    Deserialize,
    CanonicalSerialize,
    CanonicalDeserialize,
)]
pub struct ViewNum(pub i64);

impl ViewNum {
    /// Returns the next view number (current + 1)
    pub fn incr(&self) -> Self {
        ViewNum(self.0 + 1)
    }
}

/// Represents a slot number within a view
/// 
/// Slot numbers are used to order blocks produced by the same process.
/// Each process maintains separate slot counters for leader and transaction blocks.
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Debug,
    Serialize,
    Deserialize,
    CanonicalSerialize,
    CanonicalDeserialize,
)]
pub struct SlotNum(pub u64);

impl SlotNum {
    /// Checks if this slot is the immediate predecessor of another slot
    pub fn is_pred(&self, other: SlotNum) -> bool {
        self.0 + 1 == other.0
    }
}

/// Represents a unique hash for a block
#[derive(
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Debug,
    Serialize,
    Deserialize,
    CanonicalSerialize,
    CanonicalDeserialize,
)]
pub struct BlockHash(pub u64);

/// Throughput phase within a view
/// 
/// - High (0): Leader blocks help order transaction blocks
/// - Low (1): Transaction blocks can be finalized directly
#[derive(Copy, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Phase {
    High = 0,
    Low = 1,
}

//==============================================================================
// Block Types and Data Structures
//==============================================================================

/// Type of block in the protocol
/// 
/// There are three block types:
/// - Genesis: The initial block that starts the DAG
/// - Lead: Leader blocks produced by the leader of a view
/// - Tr: Transaction blocks containing actual transactions
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub enum BlockType {
    Genesis,
    // IMPORTANT: Lead must be ordered before Tr
    Lead,
    Tr,
}

impl CanonicalSerialize for BlockType {
    fn serialize_with_mode<W: std::io::Write>(
        &self,
        writer: W,
        compress: ark_serialize::Compress,
    ) -> Result<(), ark_serialize::SerializationError> {
        u8::serialize_with_mode(&(*self as u8), writer, compress)
    }

    fn serialized_size(&self, _: ark_serialize::Compress) -> usize {
        1
    }
}

impl ark_serialize::Valid for BlockType {
    fn check(&self) -> Result<(), ark_serialize::SerializationError> {
        Ok(())
    }
}

impl CanonicalDeserialize for BlockType {
    fn deserialize_with_mode<R: std::io::Read>(
        reader: R,
        compress: ark_serialize::Compress,
        validate: ark_serialize::Validate,
    ) -> Result<Self, ark_serialize::SerializationError> {
        let b = u8::deserialize_with_mode(reader, compress, validate)?;
        match b {
            0 => Ok(BlockType::Genesis),
            1 => Ok(BlockType::Lead),
            2 => Ok(BlockType::Tr),
            _ => Err(ark_serialize::SerializationError::InvalidData),
        }
    }
}

/// A unique identifier for a block in the DAG
/// 
/// Contains all metadata needed to uniquely identify and position a block 
/// in the DAG structure
#[derive(
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    CanonicalSerialize,
    CanonicalDeserialize,
)]
pub struct BlockKey {
    /// Type of the block (Genesis, Lead, or Tr)
    pub type_: BlockType,
    
    /// View in which the block was created
    pub view: ViewNum,
    
    /// Height of the block in the DAG (longest path from genesis)
    pub height: usize,
    
    /// Identity of the block's author
    pub author: Option<Identity>, // TODO: refactor genesis handling to make this mandatory
    
    /// Slot number within the view for this author
    pub slot: SlotNum,
    
    /// Optional hash uniquely identifying the block
    pub hash: Option<BlockHash>,
}

impl std::fmt::Debug for BlockKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", format::format_block_key(self))
    }
}

/// The genesis block's key, used to initialize the DAG
pub const GEN_BLOCK_KEY: BlockKey = BlockKey {
    type_: BlockType::Genesis,
    view: ViewNum(-1),
    height: 0,
    author: None,
    slot: SlotNum(0),
    hash: None,
};

/// Data payload for a transaction
/// 
/// Currently represented as opaque bytes, but could be extended 
/// to support specific transaction types
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub enum Transaction {
    Opaque(Vec<u8>),
}

impl CanonicalSerialize for Transaction {
    fn serialize_with_mode<W: std::io::Write>(
        &self,
        writer: W,
        compress: ark_serialize::Compress,
    ) -> Result<(), ark_serialize::SerializationError> {
        match self {
            Transaction::Opaque(data) => data.serialize_with_mode(writer, compress),
        }
    }

    fn serialized_size(&self, compress: ark_serialize::Compress) -> usize {
        match self {
            Transaction::Opaque(data) => data.serialized_size(compress),
        }
    }
}

impl CanonicalDeserialize for Transaction {
    fn deserialize_with_mode<R: std::io::Read>(
        reader: R,
        compress: ark_serialize::Compress,
        validate: ark_serialize::Validate,
    ) -> Result<Self, ark_serialize::SerializationError> {
        let data = Vec::deserialize_with_mode(reader, compress, validate)?;
        Ok(Transaction::Opaque(data))
    }
}

impl Valid for Transaction {
    fn check(&self) -> Result<(), ark_serialize::SerializationError> {
        Ok(())
    }
}

/// Data payload for a block, depends on the block type
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum BlockData {
    /// Genesis block has no data
    Genesis,
    
    /// Transaction block containing user transactions
    Tr {
        transactions: Vec<Transaction>,
    },
    
    /// Leader block containing justification for view changes
    Lead {
        justification: Vec<Arc<Signed<StartView>>>,
    },
}

impl CanonicalSerialize for BlockData {
    fn serialize_with_mode<W: std::io::Write>(
        &self,
        mut writer: W,
        compress: ark_serialize::Compress,
    ) -> Result<(), ark_serialize::SerializationError> {
        match self {
            BlockData::Genesis => u8::serialize_with_mode(&0, writer, compress),
            BlockData::Tr { transactions } => {
                u8::serialize_with_mode(&1, &mut writer, compress)?;
                transactions.serialize_with_mode(writer, compress)
            }
            BlockData::Lead { justification } => {
                u8::serialize_with_mode(&2, &mut writer, compress)?;
                justification.serialize_with_mode(writer, compress)
            }
        }
    }

    fn serialized_size(&self, compress: ark_serialize::Compress) -> usize {
        match self {
            BlockData::Genesis => 1,
            BlockData::Tr { transactions } => 1 + transactions.serialized_size(compress),
            BlockData::Lead { justification } => 1 + justification.serialized_size(compress),
        }
    }
}

impl Valid for BlockData {
    fn check(&self) -> Result<(), ark_serialize::SerializationError> {
        Ok(())
    }
}

impl CanonicalDeserialize for BlockData {
    fn deserialize_with_mode<R: std::io::Read>(
        mut reader: R,
        compress: ark_serialize::Compress,
        validate: ark_serialize::Validate,
    ) -> Result<Self, ark_serialize::SerializationError> {
        let b = u8::deserialize_with_mode(&mut reader, compress, validate)?;
        match b {
            0 => Ok(BlockData::Genesis),
            1 => Ok(BlockData::Tr {
                transactions: Vec::deserialize_with_mode(reader, compress, validate)?,
            }),
            2 => Ok(BlockData::Lead {
                justification: Vec::deserialize_with_mode(reader, compress, validate)?,
            }),
            _ => Err(ark_serialize::SerializationError::InvalidData),
        }
    }
}

/// Core block structure in the Morpheus protocol
/// 
/// A block contains its identifier (key), pointers to previous blocks (prev),
/// a 1-QC (one), and the actual block data.
#[derive(
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    CanonicalSerialize,
    CanonicalDeserialize,
    Serialize,
    Deserialize,
)]
pub struct Block {
    /// Unique identifier for this block
    pub key: BlockKey,
    
    /// Pointers to previous blocks (DAG edges)
    pub prev: Vec<ThreshSigned<VoteData>>,
    
    /// A 1-QC (quorum certificate) used for ordering
    pub one: ThreshSigned<VoteData>,
    
    /// The actual block data (transactions or leader data)
    pub data: BlockData,
}

impl std::fmt::Debug for Block {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", format::format_block(self, true))
    }
}

//==============================================================================
// Voting and Quorum Data Structures
//==============================================================================

/// Data for a vote on a block
/// 
/// Contains the vote level (z) and the block being voted for.
/// Vote levels: 0 (basic vote), 1 (ordering vote), 2 (finalization vote)
#[derive(
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    CanonicalSerialize,
    CanonicalDeserialize,
)]
pub struct VoteData {
    /// Vote level: 0, 1, or 2
    pub z: u8,
    
    /// Block being voted for
    pub for_which: BlockKey,
}

impl std::fmt::Debug for VoteData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", format::format_vote_data(self, false))
    }
}

impl VoteData {
    /// Compares two VoteData objects for QC ordering
    /// 
    /// Used to determine which QC is "greater" according to the protocol rules.
    /// Orders first by view, then by block type, then by height.
    pub fn compare_qc(&self, other: &Self) -> std::cmp::Ordering {
        self.for_which
            .view
            .cmp(&other.for_which.view)
            .then_with(|| self.for_which.type_.cmp(&other.for_which.type_))
            .then_with(|| self.for_which.height.cmp(&other.for_which.height))
    }
}

/// Represents a view change message sent to the new leader
/// 
/// This message is sent when a process enters a new view:
/// "Send (v, q') signed by p_i to lead(v), where q' is a maximal amongst 1-QCs seen by p_i"
#[derive(
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Debug,
    Serialize,
    Deserialize,
    CanonicalDeserialize,
    CanonicalSerialize,
)]
pub struct StartView {
    /// The new view number
    pub view: ViewNum,

    /// The maximal 1-QC seen by this process
    /// This is used by the new leader to determine which blocks to build upon
    pub qc: ThreshSigned<VoteData>,
}

//==============================================================================
// Protocol Messages
//==============================================================================

/// Message types used in the Morpheus protocol
/// 
/// These messages are exchanged between processes to implement the protocol.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Message {
    /// A new block being broadcast
    Block(Arc<Signed<Block>>),
    
    /// A single vote for a block
    NewVote(Arc<ThreshPartial<VoteData>>),
    
    /// A quorum certificate (n-f votes combined)
    QC(Arc<ThreshSigned<VoteData>>),
    
    /// Signal to end the current view (timeout message)
    EndView(Arc<ThreshPartial<ViewNum>>),
    
    /// Certificate proving f+1 EndView messages for a view
    EndViewCert(Arc<ThreshSigned<ViewNum>>),
    
    /// Message sent to a new leader when entering a view
    StartView(Arc<Signed<StartView>>),
}

impl std::fmt::Debug for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", format::format_message(self, false))
    }
}