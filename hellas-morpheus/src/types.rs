use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet, VecDeque, vec_deque},
    sync::Arc,
};
use serde::{Serialize, Deserialize};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub enum BlockType {
    Genesis,
    Lead,
    Tr,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct ThreshSignature {}

#[derive(PartialEq, Clone, Debug, Serialize, Deserialize)]
pub struct Signature {}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum Transaction {
    Opaque(Vec<u8>),
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub struct ViewNum(pub i64);
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Signed<T> {
    pub data: T,
    pub author: Identity,
    pub signature: Signature,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ThreshSigned<T> {
    pub data: T,
    pub signature: ThreshSignature,
}

impl<T> Signed<T> {
    pub fn is_valid(&self) -> bool {
        true
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub struct BlockKey {
    pub type_: BlockType,
    pub view: ViewNum,
    pub height: usize,
    pub author: Option<Identity>, // TODO: refactor genesis handling to make this mandatory
    pub slot: SlotNum,
    pub hash: Option<BlockHash>,
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

impl VoteData {
    pub fn compare_qc(&self, other: &Self) -> std::cmp::Ordering {
        self.for_which
            .view
            .cmp(&other.for_which.view)
            .then_with(|| self.for_which.type_.cmp(&other.for_which.type_))
            .then_with(|| self.for_which.height.cmp(&other.for_which.height))
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct StartView {
    pub view: ViewNum,
    pub max_1_qc: ThreshSigned<VoteData>,
}

#[derive(Clone, Serialize, Deserialize)]
pub enum BlockData {
    Genesis,
    Tr {
        transactions: Vec<Transaction>,
    },
    Lead {
        justification: Vec<Signed<StartView>>,
    },
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Block {
    pub key: BlockKey,
    pub prev: Vec<ThreshSigned<VoteData>>,
    pub one: ThreshSigned<VoteData>,
    pub data: BlockData,
}

#[derive(Clone, Serialize, Deserialize)]
pub enum Message {
    Block(Signed<Arc<Block>>),
    NewVote(Signed<VoteData>),
    QC(ThreshSigned<VoteData>),
    EndView(Signed<ViewNum>),
    EndViewCert(ThreshSigned<ViewNum>),
    StartView(Signed<StartView>),
}

#[derive(Copy, Clone, PartialEq, Serialize, Deserialize)]
pub enum Phase {
    High = 0,
    Low = 1,
}
