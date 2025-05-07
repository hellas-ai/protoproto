use crate::Transaction;
use crate::crypto::*;
use crate::format;

use ark_serialize::Valid;
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
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

#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Debug,
    Serialize,
    Deserialize,
    CanonicalSerialize,
    CanonicalDeserialize,
)]
pub struct ViewNum(pub i64);
impl ViewNum {
    pub fn incr(&self) -> Self {
        ViewNum(self.0 + 1)
    }
}

#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Debug,
    Serialize,
    Deserialize,
    CanonicalSerialize,
    CanonicalDeserialize,
)]
pub struct SlotNum(pub u64);
impl SlotNum {
    pub fn is_pred(&self, other: SlotNum) -> bool {
        self.0 + 1 == other.0
    }
}

#[derive(
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Debug,
    Serialize,
    Deserialize,
    CanonicalSerialize,
    CanonicalDeserialize,
)]
pub struct BlockHash(pub u64);

#[derive(
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    CanonicalSerialize,
    CanonicalDeserialize,
)]
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

#[derive(
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    CanonicalSerialize,
    CanonicalDeserialize,
)]
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

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    CanonicalDeserialize,
    CanonicalSerialize,
)]
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

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum BlockData<Tr> {
    Genesis,
    Tr {
        transactions: Vec<Tr>,
    },
    Lead {
        justification: Vec<Arc<Signed<StartView>>>,
    },
}

impl<Tr: CanonicalSerialize> CanonicalSerialize for BlockData<Tr> {
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

impl<Tr: Sync> Valid for BlockData<Tr> {
    fn check(&self) -> Result<(), ark_serialize::SerializationError> {
        Ok(())
    }
}

impl<Tr: CanonicalDeserialize> CanonicalDeserialize for BlockData<Tr> {
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

#[derive(
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    CanonicalSerialize,
    CanonicalDeserialize,
)]
pub struct Block<Tr: Transaction> {
    pub key: BlockKey,
    pub prev: Vec<ThreshSigned<VoteData>>,
    pub one: ThreshSigned<VoteData>,
    pub data: BlockData<Tr>,
}

impl<Tr: Transaction> std::fmt::Debug for Block<Tr> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", format::format_block(self, true))
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Hash, Ord, Serialize, Deserialize)]
pub enum Message<Tr: Transaction> {
    Block(Arc<Signed<Block<Tr>>>),
    NewVote(Arc<ThreshPartial<VoteData>>),
    QC(Arc<ThreshSigned<VoteData>>),
    EndView(Arc<ThreshPartial<ViewNum>>),
    EndViewCert(Arc<ThreshSigned<ViewNum>>),
    StartView(Arc<Signed<StartView>>),
}

impl<Tr: Transaction> std::fmt::Debug for Message<Tr> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", format::format_message(self, false))
    }
}

#[derive(Copy, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Phase {
    High = 0,
    Low = 1,
}
