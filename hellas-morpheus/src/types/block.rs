use std::hash::{Hash, Hasher};
use std::fmt;

/// The type of a block in the Morpheus consensus protocol.
///
/// The protocol defines three block types:
/// - Genesis block (`Genesis`): The starting block of the blockchain
/// - Leader blocks (`Lead`): Control the consensus process in each view
/// - Transaction blocks (`Tr`): Contain actual transaction data
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockType {
    /// Genesis block that starts the blockchain
    Genesis,
    /// Leader block that controls consensus in a view
    Lead,
    /// Transaction block containing application data
    Tr,
}

impl fmt::Debug for BlockType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BlockType::Genesis => write!(f, "Genesis"),
            BlockType::Lead => write!(f, "Lead"),
            BlockType::Tr => write!(f, "Tr"),
        }
    }
}

/// A process identifier within the Morpheus protocol.
///
/// Each process is identified by a unique ID.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProcessId(pub usize);

impl fmt::Debug for ProcessId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "P{}", self.0)
    }
}

/// A unique identifier for a block in the Morpheus protocol.
///
/// Each block is uniquely identified by a combination of:
/// - The block type (Leader or Transaction)
/// - The process that authored the block
/// - The view in which the block was created
/// - The slot within that view
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockId {
    /// The type of the block (Leader or Transaction)
    pub block_type: BlockType,
    /// The author of the block
    pub auth: ProcessId,
    /// The view in which the block was created
    pub view: usize,
    /// The slot in which the block was created
    pub slot: usize,
}

impl fmt::Debug for BlockId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}({:?},v{},s{})", 
               self.block_type, 
               self.auth, 
               self.view, 
               self.slot)
    }
}

/// A unique identifier for a Quorum Certificate (QC).
///
/// A QC references the block that it certifies.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct QcId {
    /// The identifier of the certified block
    pub block_id: BlockId,
}

impl fmt::Debug for QcId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "QC({:?})", self.block_id)
    }
}

/// A block in the Morpheus consensus protocol.
///
/// Blocks form the core data structure of the protocol and can be of two types:
/// - Leader blocks: Control consensus in each view
/// - Transaction blocks: Contain actual transaction data
///
/// Each block maintains references to previous blocks via their QC IDs.
#[derive(Clone, PartialEq, Eq)]
pub struct Block {
    /// The block's unique identifier
    pub id: BlockId,
    /// The height of the block in the blockchain
    pub height: usize,
    /// The previous blocks this block refers to (via their QC IDs)
    pub prev_qcs: Vec<QcId>,
    /// The 1-QC for this block (if any)
    pub one_qc: Option<QcId>,
    /// Justification for a leader block as (view, sender) pairs
    pub justification: Vec<(usize, ProcessId)>,
}

impl fmt::Debug for Block {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Block({:?},h{},prev:{},1qc:{},just:{})", 
               self.id, 
               self.height,
               self.prev_qcs.len(),
               if self.one_qc.is_some() { "✓" } else { "✗" },
               self.justification.len())
    }
}

impl Hash for Block {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.height.hash(state);
        // Skip hashing prev_qcs, one_qc, and justification to avoid recursion issues
        // This simplified hashing is sufficient for block identification
    }
}

/// A Quorum Certificate (QC) for a block in the Morpheus protocol.
///
/// A QC represents acknowledgment from a quorum of processes for a specific block.
/// QCs are essential for the protocol's progress and safety properties.
#[derive(Clone, PartialEq, Eq)]
pub struct QuorumCertificate {
    /// The identifier for this QC
    pub id: QcId,
    /// The height of the certified block
    pub height: usize,
}

impl fmt::Debug for QuorumCertificate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "QCert({:?},h{})", self.id, self.height)
    }
}

impl Hash for QuorumCertificate {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.height.hash(state);
        // Simplified hash implementation, focusing on stable identification
    }
} 