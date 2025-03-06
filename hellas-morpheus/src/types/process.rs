use std::collections::{HashMap, HashSet};
use std::time::Instant;

use crate::types::{BlockId, BlockType, Message, ProcessId, QcId, QuorumCertificate, Block};

/// The state of a process in the Morpheus consensus protocol.
///
/// This structure represents the complete state of a participant in the Morpheus
/// protocol, including all messages received, certificates created, and view/phase
/// information.
pub struct MorpheusProcess {
    /// The unique identifier of this process
    pub id: ProcessId,
    /// The set of all messages received or created by this process
    pub m_i: HashSet<Message>,
    /// The set of quorum certificates known to this process
    pub q_i: HashSet<QuorumCertificate>,
    /// The current view number
    pub view_i: usize,
    /// The current slot number for each block type
    pub slot_i: HashMap<BlockType, usize>,
    /// Whether this process has voted for a specific block
    /// Key is (vote_num, block_id)
    pub voted_i: HashMap<(usize, BlockId), bool>,
    /// The current phase within each view
    pub phase_i: HashMap<usize, usize>,
    /// Total number of processes in the system
    pub n: usize,
    /// Maximum number of faulty processes the system can tolerate
    pub f: usize,
    /// The time when this process entered its current view
    pub view_entry_time: Instant,
    /// Tracks which blocks have already had 0-QCs created for them
    pub sent_zero_qc: HashSet<BlockId>,
    /// Storage for all blocks known to this process
    pub blocks: HashMap<BlockId, Block>,
    /// Storage for all QCs known to this process
    pub qcs: HashMap<QcId, QuorumCertificate>,
} 