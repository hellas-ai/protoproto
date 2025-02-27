use std::cmp::Ordering;
use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::hash::{Hash as StdHash, Hasher};
use std::time::{Duration, Instant};
use serde::{Serialize, Deserialize};
use type_uuid::TypeUuid;

/// Wrapper type for view numbers
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Debug, Default)]
pub struct View(pub u64);

impl fmt::Display for View {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}", self.0)
    }
}

impl StdHash for View {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

/// Wrapper type for block heights
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Debug, Default)]
pub struct Height(pub u64);

impl fmt::Display for Height {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "h{}", self.0)
    }
}

impl StdHash for Height {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

/// Wrapper type for slot numbers
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Debug, Default)]
pub struct Slot(pub u64);

impl fmt::Display for Slot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "s{}", self.0)
    }
}

impl StdHash for Slot {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

/// Process ID
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Debug)]
pub struct ProcessId(pub usize);

impl fmt::Display for ProcessId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "p{}", self.0)
    }
}

impl StdHash for ProcessId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

/// Block types as defined in the paper
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Debug)]
pub enum BlockType {
    /// Genesis block (gen)
    Genesis,
    /// Leader block (lead) - note Leader < Transaction in ordering
    Leader,
    /// Transaction block (Tr)
    Transaction,
}

impl StdHash for BlockType {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (*self as u8).hash(state);
    }
}

impl BlockType {
    /// String representation matching the paper
    pub fn as_str(&self) -> &'static str {
        match self {
            BlockType::Genesis => "gen",
            BlockType::Transaction => "Tr",
            BlockType::Leader => "lead",
        }
    }
}

/// Throughput phase of the protocol
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum ThroughputPhase {
    /// High throughput phase (leader-based)
    High = 0,
    /// Low throughput phase (leaderless)
    Low = 1,
}

/// Cryptographic hash placeholder
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Debug)]
pub struct Hash(pub Vec<u8>);

impl Hash {
    /// Create a new hash for testing
    #[cfg(test)]
    pub fn new_for_testing(s: &str) -> Self {
        Self(s.as_bytes().to_vec())
    }
}

impl StdHash for Hash {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

/// Transaction representation
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub struct Transaction {
    pub data: Vec<u8>,
    // In a real implementation, would include sender, nonce, etc.
}

impl StdHash for Transaction {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.data.hash(state);
    }
}

/// Vote types
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Debug)]
pub enum VoteType {
    /// 0-vote: For data availability and non-equivocation in high throughput
    Vote0 = 0,
    /// 1-vote: First round of voting on a block
    Vote1 = 1,
    /// 2-vote: Second round of voting on a block
    Vote2 = 2,
}

impl VoteType {
    /// Get the numeric value of the vote type
    pub fn value(&self) -> u8 {
        *self as u8
    }
    
    /// Get the next vote type in sequence
    pub fn next(&self) -> Option<Self> {
        match self {
            VoteType::Vote0 => Some(VoteType::Vote1),
            VoteType::Vote1 => Some(VoteType::Vote2),
            VoteType::Vote2 => None,
        }
    }
}

/// Threshold signature placeholder
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub struct ThresholdSignature(pub Vec<u8>);

/// Signature placeholder
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub struct Signature(pub Vec<u8>);

/// Vote representation
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub struct Vote {
    /// Type of vote (0, 1, or 2)
    pub vote_type: VoteType,
    /// Type of block being voted for
    pub block_type: BlockType,
    /// View of the block
    pub view: View,
    /// Height of the block
    pub height: Height,
    /// Author of the block
    pub author: ProcessId,
    /// Slot of the block
    pub slot: Slot,
    /// Hash of the block
    pub block_hash: Hash,
    /// Process that signed this vote
    pub signer: ProcessId,
    /// Signature by the signer
    pub signature: Signature,
}

/// Quorum Certificate (QC)
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub struct QC {
    /// Type of votes in this QC
    pub vote_type: VoteType,
    /// Type of block this QC is for
    pub block_type: BlockType,
    /// View of the block
    pub view: View,
    /// Height of the block
    pub height: Height,
    /// Author of the block
    pub author: ProcessId,
    /// Slot of the block
    pub slot: Slot,
    /// Hash of the block
    pub block_hash: Hash,
    /// Threshold signature from n-f processes
    pub signatures: ThresholdSignature,
}

impl QC {
    /// Compare QCs according to the ordering defined in the paper (section 4)
    ///
    /// QCs are preordered first by view, then by type with lead < Tr, and then by height
    pub fn compare(&self, other: &Self) -> Ordering {
        self.view.cmp(&other.view)
            .then_with(|| self.block_type.cmp(&other.block_type))
            .then_with(|| self.height.cmp(&other.height))
    }

    /// Check if this QC is less than or equal to another QC
    pub fn is_less_than_or_equal(&self, other: &Self) -> bool {
        self.compare(other) != Ordering::Greater
    }

    /// Check if this QC is greater than another QC
    pub fn is_greater_than(&self, other: &Self) -> bool {
        self.compare(other) == Ordering::Greater
    }
}

/// Reference to a QC in block.prev
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub struct BlockPointer {
    /// The hash of the block being pointed to
    pub block_hash: Hash,
    /// The QC for the block
    pub qc: QC,
}

/// Block structure
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub struct Block {
    /// Type of block (gen, Tr, or lead)
    pub block_type: BlockType,
    /// View number
    pub view: View,
    /// Block height
    pub height: Height,
    /// Creator of the block (None for genesis)
    pub author: Option<ProcessId>,
    /// Slot number
    pub slot: Slot,
    /// Transactions contained in the block (empty for leader/genesis)
    pub transactions: Vec<Transaction>,
    /// QCs for blocks that this block points to (b.prev in the paper)
    pub prev: Vec<BlockPointer>,
    /// The 1-QC for ordering (mandatory, b.1-QC in the paper)
    pub qc: QC,
    /// Justification for first leader block of a view (b.just in the paper)
    pub justification: Option<Vec<ViewMessage>>,
    /// Signature by the author
    pub signature: Option<Signature>,
}

/// View message sent at the start of a view
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub struct ViewMessage {
    /// View number
    pub view: View,
    /// Maximal 1-QC seen by the sender
    pub qc: QC,
    /// Process that signed this message
    pub signer: ProcessId,
    /// Signature
    pub signature: Signature,
}

/// End-view message
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub struct EndViewMessage {
    /// View to end
    pub view: View,
    /// Process that signed this message
    pub signer: ProcessId,
    /// Signature
    pub signature: Signature,
}

/// View certificate formed from f+1 end-view messages
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub struct ViewCertificate {
    /// Next view
    pub view: View,
    /// Threshold signature from f+1 processes
    pub signatures: ThresholdSignature,
}

/// Voted record key
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Debug)]
pub struct VotedKey {
    /// Vote type
    pub vote_type: VoteType,
    /// Block type
    pub block_type: BlockType,
    /// Slot
    pub slot: Slot,
    /// Block author
    pub author: ProcessId,
}

/// Timer for tracking timeouts
pub struct Timer {
    pub start_time: Instant,
    pub duration: Duration,
}

impl Timer {
    pub fn new(duration: Duration) -> Self {
        Self {
            start_time: Instant::now(),
            duration,
        }
    }
    
    pub fn is_expired(&self) -> bool {
        self.start_time.elapsed() >= self.duration
    }
    
    pub fn remaining(&self) -> Duration {
        if self.is_expired() {
            Duration::from_millis(0)
        } else {
            self.duration - self.start_time.elapsed()
        }
    }
}

/// View timeouts tracking structure
pub struct ViewTimeouts {
    /// The view these timeouts are for
    pub view: View,
    /// When we entered this view
    pub view_entry_time: Instant,
    /// Timeout for sending complaints (6Δ)
    pub complaint_timeout: Duration,
    /// Timeout for ending the view (12Δ)
    pub end_view_timeout: Duration,
    /// Whether we've complained about unfinalized blocks
    pub complained: bool,
    /// Whether we've sent an end-view message
    pub sent_end_view: bool,
}

impl Block {
    /// Create a new genesis block
    pub fn new_genesis() -> Self {
        Self {
            block_type: BlockType::Genesis,
            view: View(0),
            height: Height(0),
            author: None,
            slot: Slot(0),
            transactions: Vec::new(),
            prev: Vec::new(),
            qc: QC {
                vote_type: VoteType::Vote1,
                block_type: BlockType::Genesis,
                view: View(0),
                height: Height(0),
                author: ProcessId(0),
                slot: Slot(0),
                block_hash: Hash(vec![]),
                signatures: ThresholdSignature(vec![]),
            },
            justification: None,
            signature: None,
        }
    }
    
    /// Generate a proper hash for this block
    pub fn hash(&self) -> Hash {
        let mut hasher = DefaultHasher::new();
        
        // Hash the block fields
        self.block_type.hash(&mut hasher);
        self.view.0.hash(&mut hasher);
        self.height.0.hash(&mut hasher);
        if let Some(author) = self.author {
            author.0.hash(&mut hasher);
        }
        self.slot.0.hash(&mut hasher);
        
        // Hash transactions
        for tx in &self.transactions {
            tx.data.hash(&mut hasher);
        }
        
        // Hash prev pointers
        for pointer in &self.prev {
            pointer.block_hash.hash(&mut hasher);
        }
        
        // Hash 1-QC
        self.qc.block_hash.hash(&mut hasher);
        
        // Convert to bytes
        let hash_value = hasher.finish();
        let bytes = hash_value.to_be_bytes().to_vec();
        
        Hash(bytes)
    }
}

/// Error types for block operations
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockError {
    InvalidBlock,
    DuplicateBlock,
    MissingDependency,
    // Other error cases
}

/// Message types that can be sent between nodes
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Message {
    Block(Block),
    Vote(Vote),
    QC(QC),
    ViewMessage(ViewMessage),
    EndViewMessage(EndViewMessage),
    ViewCertificate(ViewCertificate),
}