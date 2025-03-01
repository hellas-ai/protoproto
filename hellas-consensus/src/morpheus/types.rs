// types.rs - Core type definitions
use std::collections::HashSet;
use std::time::Duration;

// Basic type wrappers
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct View(pub u64);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Height(pub u64);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Slot(pub u64);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ProcessId(pub usize);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum BlockType {
    Genesis,
    Leader,
    Transaction,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThroughputPhase {
    High = 0,
    Low = 1,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Hash([u8; 32]);

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Transaction {
    pub data: Vec<u8>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum VoteType {
    Vote0 = 0,
    Vote1 = 1,
    Vote2 = 2,
}

// Vote and QC structures
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Vote {
    pub vote_type: VoteType,
    pub block_type: BlockType,
    pub view: View,
    pub height: Height,
    pub author: ProcessId,
    pub slot: Slot,
    pub block_hash: Hash,
    pub signer: ProcessId,
    pub signature: Vec<u8>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct QC {
    pub vote_type: VoteType,
    pub block_type: BlockType,
    pub view: View,
    pub height: Height,
    pub author: ProcessId,
    pub slot: Slot,
    pub block_hash: Hash,
    pub signatures: Vec<u8>,
}

// Block and pointer structures
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BlockPointer {
    pub block_hash: Hash,
    pub qc: QC,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Block {
    pub block_type: BlockType,
    pub view: View,
    pub height: Height,
    pub author: Option<ProcessId>,
    pub slot: Slot,
    pub transactions: Vec<Transaction>,
    pub prev: Vec<BlockPointer>,
    pub qc: QC,
    pub justification: Option<Vec<ViewMessage>>,
    pub signature: Option<Vec<u8>>,
}

// View change structures
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ViewMessage {
    pub view: View,
    pub qc: QC,
    pub signer: ProcessId,
    pub signature: Vec<u8>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EndViewMessage {
    pub view: View,
    pub signer: ProcessId,
    pub signature: Vec<u8>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ViewCertificate {
    pub view: View,
    pub signatures: Vec<u8>,
}

// Protocol effect model
#[derive(Clone, Debug)]
pub enum Effect {
    BroadcastBlock(Block),
    SendVoteToProcess(Vote, ProcessId),
    BroadcastVote(Vote),
    BroadcastQC(QC),
    SendViewMessage(ViewMessage, ProcessId),
    BroadcastEndView(EndViewMessage),
    BroadcastViewCertificate(ViewCertificate),
    SendQCToLeader(QC, ProcessId),
    ScheduleTimeout(Duration),
}