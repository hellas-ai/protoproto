use crate::format;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub enum BlockType {
    Genesis,
    // IMPORTANT: Lead must be ordered before Tr
    Lead,
    Tr,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub struct ThreshSignature {}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub struct Signature {}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub enum Transaction {
    Opaque(Vec<u8>),
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub struct ViewNum(pub i64);
impl ViewNum {
    pub fn incr(&self) -> Self {
        ViewNum(self.0 + 1)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub struct SlotNum(pub u64);
impl SlotNum {
    pub fn is_pred(&self, other: SlotNum) -> bool {
        self.0 + 1 == other.0
    }
}

#[derive(PartialEq, Clone, PartialOrd, Eq, Ord, Debug, Serialize, Deserialize)]
pub struct Identity(pub u64);

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub struct BlockHash(pub u64);

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub struct Signed<T> {
    pub data: T,
    pub author: Identity,
    pub signature: Signature,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub struct ThreshSigned<T> {
    pub data: T,
    pub signature: ThreshSignature,
}

impl<T> ThreshSigned<T> {
    pub fn valid_signature(&self) -> bool {
        true
    }
}

impl<T> Signed<T> {
    pub fn valid_signature(&self) -> bool {
        true
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BlockKey {
    pub type_: BlockType,
    pub view: ViewNum,
    pub height: usize,
    pub author: Option<Identity>, // TODO: refactor genesis handling to make this mandatory
    pub slot: SlotNum,
    pub hash: Option<BlockHash>,
}

impl std::fmt::Debug for BlockKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", format::format_block_key(self))
    }
}

pub const GEN_BLOCK_KEY: BlockKey = BlockKey {
    type_: BlockType::Genesis,
    view: ViewNum(-1),
    height: 0,
    author: None,
    slot: SlotNum(0),
    hash: None,
};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct VoteData {
    pub z: u8,
    pub for_which: BlockKey,
}

impl std::fmt::Debug for VoteData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", format::format_vote_data(self, false))
    }
}

impl VoteData {
    pub fn compare_qc(&self, other: &Self) -> std::cmp::Ordering {
        self.for_which
            .view
            .cmp(&other.for_which.view)
            .then_with(|| self.for_which.type_.cmp(&other.for_which.type_))
            .then_with(|| self.for_which.height.cmp(&other.for_which.height))
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
/// Represents a view change message sent to the new leader
///
/// This message is sent when a process enters a new view:
/// "Send (v, q') signed by p_i to lead(v), where q' is a maximal amongst 1-QCs seen by p_i"
pub struct StartView {
    /// The new view number
    pub view: ViewNum,

    /// The maximal 1-QC seen by this process
    /// This is used by the new leader to determine which blocks to build upon
    pub qc: ThreshSigned<VoteData>,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum BlockData {
    Genesis,
    Tr {
        transactions: Vec<Transaction>,
    },
    Lead {
        justification: Vec<Signed<StartView>>,
    },
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Block {
    pub key: BlockKey,
    pub prev: Vec<ThreshSigned<VoteData>>,
    pub one: ThreshSigned<VoteData>,
    pub data: BlockData,
}

impl std::fmt::Debug for Block {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", format::format_block(self, true))
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Message {
    Block(Arc<Signed<Block>>),
    NewVote(Arc<Signed<VoteData>>),
    QC(Arc<ThreshSigned<VoteData>>),
    EndView(Arc<Signed<ViewNum>>),
    EndViewCert(Arc<ThreshSigned<ViewNum>>),
    StartView(Arc<Signed<StartView>>),
}

impl std::fmt::Debug for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", format::format_message(self, true))
    }
}

#[derive(Copy, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Phase {
    High = 0,
    Low = 1,
}
