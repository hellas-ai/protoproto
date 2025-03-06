use crate::types::{Block, BlockId, ProcessId, QcId, QuorumCertificate, ViewNum};
use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum VoteKind {
    Zero,
    One,
    Two,
}

impl fmt::Display for VoteKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VoteKind::Zero => write!(f, "0"),
            VoteKind::One => write!(f, "1"),
            VoteKind::Two => write!(f, "2"),
        }
    }
}

/// A vote for a block in the Morpheus protocol.
///
/// Votes are used to form quorums and eventually create Quorum Certificates (QCs).
/// There are three types of votes (0, 1, and 2) representing different stages of agreement.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Vote {
    /// The vote number (0, 1, or 2)
    pub vote_num: VoteKind,
    /// The block being voted for
    pub block_id: BlockId,
    /// The process that cast this vote
    pub voter: ProcessId,
}

impl fmt::Debug for Vote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Vote{}({:?} by {:?})",
            self.vote_num, self.block_id, self.voter
        )
    }
}

/// A view message in the Morpheus protocol.
///
/// View messages are used for view synchronization and view changes.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ViewMessage {
    /// The view number this message refers to
    pub view: ViewNum,
    /// The QC associated with this view message (if any)
    pub qc_id: Option<QcId>,
    /// The sender of the message
    pub sender: ProcessId,
}

impl fmt::Debug for ViewMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.qc_id {
            Some(qc) => write!(f, "View(v{}, {:?}, {:?})", self.view.0, qc, self.sender),
            None => write!(f, "View(v{}, None, {:?})", self.view.0, self.sender),
        }
    }
}

/// An end-view message in the Morpheus protocol.
///
/// End-view messages are used to signal that a process wants to move to a new view,
/// typically because the current view is not making progress.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EndViewMessage {
    /// The view to end
    pub view: ViewNum,
    /// The sender of the message
    pub sender: ProcessId,
}

impl fmt::Debug for EndViewMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EndView(v{}, {:?})", self.view.0, self.sender)
    }
}

/// A message in the Morpheus consensus protocol.
///
/// The protocol defines various message types for different purposes:
/// - Block: A new block proposal
/// - Vote: A vote for a block
/// - QC: A quorum certificate for a block
/// - ViewMsg: A message related to view synchronization
/// - EndViewMsg: A message requesting to end the current view
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Message {
    /// A block proposal
    Block(Block),
    /// A vote for a block
    Vote(Vote),
    /// A quorum certificate
    QC(QuorumCertificate),
    /// A view synchronization message
    ViewMsg(ViewMessage),
    /// A message requesting to end the current view
    EndViewMsg(EndViewMessage),
}

impl fmt::Debug for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Message::Block(b) => write!(f, "{:?}", b),
            Message::Vote(v) => write!(f, "{:?}", v),
            Message::QC(q) => write!(f, "{:?}", q),
            Message::ViewMsg(vm) => write!(f, "{:?}", vm),
            Message::EndViewMsg(evm) => write!(f, "{:?}", evm),
        }
    }
}
