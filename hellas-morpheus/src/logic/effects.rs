use crate::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Internal state mutations produced by processing actions
/// These are pure data representing state changes that can be recorded and replayed
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, derivative::Derivative)]
#[derivative(Debug)]
pub enum Effect<Tr: Transaction> {
    /// Update current time
    TimeUpdated(u128),

    /// Block was recorded
    BlockRecorded {
        #[serde(bound(serialize = "Tr: Transaction", deserialize = "Tr: Transaction"))]
        #[derivative(Debug = "ignore")]
        block: Arc<Signed<Block<Tr>>>,
    },

    /// QC was recorded
    QcRecorded {
        qc: FinishedQC,
    },

    /// Vote was sent
    VoteSent {
        vote: Arc<ThreshPartial<VoteData>>,
        target: Option<Identity>, // None means broadcast
    },

    /// Vote was recorded from another process
    VoteRecorded {
        voter: Identity,
        vote: Arc<ThreshPartial<VoteData>>,
    },

    /// Quorum reached for a vote
    QuorumReached {
        qc_formed: FinishedQC,
    },

    /// Ready transactions updated
    TransactionsUpdated {
        #[serde(bound(serialize = "Tr: Transaction", deserialize = "Tr: Transaction"))]
        #[derivative(Debug = "ignore")]
        transactions: Vec<Tr>,
    },

    /// Block produced
    BlockProduced {
        #[serde(bound(serialize = "Tr: Transaction", deserialize = "Tr: Transaction"))]
        #[derivative(Debug = "ignore")]
        block: Arc<Signed<Block<Tr>>>,
    },

    /// Message sent
    MessageSent {
        #[serde(bound(serialize = "Tr: Transaction", deserialize = "Tr: Transaction"))]
        message: Message<Tr>,
        target: Option<Identity>, // None means broadcast
    },
    
    /// Slot advanced
    SlotAdvanced {
        slot_type: BlockType, // Lead or Tr
        new_slot: SlotNum,
    },

    /// Marked that we produced a leader block in view
    LeaderBlockProducedInView { view: ViewNum },

    /// Phase changed within a view
    PhaseChanged {
        view: ViewNum,
        old_phase: Phase,
        new_phase: Phase,
    },

    /// View changed
    ViewChanged {
        old_view: ViewNum,
        new_view: ViewNum,
    },

    /// End-view vote recorded
    EndViewRecorded {
        voter: Identity,
        view: ViewNum,
        vote: Arc<ThreshPartial<ViewNum>>,
    },

    /// View certificate formed
    ViewCertFormed {
        cert: Arc<ThreshSigned<ViewNum>>,
    },

    /// Start view sent
    StartViewSent {
        view: ViewNum,
        qc: FinishedQC,
        tips: Vec<FinishedQC>,
        target: Identity,
    },

    /// Start view recorded
    StartViewRecorded {
        sender: Identity,
        start_view: Arc<Signed<StartView>>,
    },

    /// Complaint sent
    ComplaintSent {
        qc: FinishedQC,
        target: Identity,
    },

    /// Pending vote marked as processed
    PendingVoteProcessed {
        view: ViewNum,
        vote_type: u8,
        block_type: BlockType,
        block_key: BlockKey,
    },
}

impl<Tr: Transaction> Effect<Tr> {
    /// Get a human-readable description of the effect for visualization
    pub fn description(&self) -> String {
        match self {
            Effect::TimeUpdated(time) => format!("Time updated to {}", time),
            Effect::BlockRecorded { block } => {
                format!(
                    "Recorded {:?} block at height {}",
                    block.data.key.type_, block.data.key.height
                )
            }
            Effect::QcRecorded {
                qc,
            } => {
                format!(
                    "{}-QC recorded",
                    qc.data.z,
                )
            }
            Effect::VoteSent {
                vote,
                target,
            } => {
                let target_str = target
                    .as_ref()
                    .map_or("all".to_string(), |t| format!("{}", t.0));
                format!(
                    "Sent {}-vote for {:?} to {}",
                    vote.data.z, vote.data.for_which, target_str
                )
            }
            Effect::VoteRecorded { voter, vote } => {
                format!(
                    "Recorded {}-vote from {} for {:?}",
                    vote.data.z, voter.0, vote.data.for_which
                )
            }
            Effect::QuorumReached { qc_formed } => {
                format!(
                    "Quorum reached for {}-QC",
                    qc_formed.data.z
                )
            }
            Effect::TransactionsUpdated { transactions } => {
                format!("Updated ready transactions ({})", transactions.len())
            }
            Effect::BlockProduced { block } => {
                format!(
                    "Produced {:?} block: {:?}",
                    block.data.key.type_, block.data.key
                )
            }
            Effect::MessageSent { message, target } => {
                let target_str = target
                    .as_ref()
                    .map_or("all".to_string(), |t| format!("{}", t.0));
                format!("Sent {} to {}", message.description(), target_str)
            }
            Effect::SlotAdvanced {
                slot_type,
                new_slot,
            } => {
                format!("{:?} slot advanced to {}", slot_type, new_slot.0)
            }
            Effect::LeaderBlockProducedInView { view } => {
                format!("Marked leader block produced in view {}", view.0)
            }
            Effect::PhaseChanged {
                view,
                old_phase,
                new_phase,
            } => {
                format!(
                    "Phase changed in view {} from {:?} to {:?}",
                    view.0, old_phase, new_phase
                )
            }
            Effect::ViewChanged { old_view, new_view } => {
                format!("View changed from {} to {}", old_view.0, new_view.0)
            }
            Effect::EndViewRecorded { voter, view, .. } => {
                format!("Recorded end-view {} from {}", view.0, voter.0)
            }
            Effect::ViewCertFormed { cert } => {
                format!("View certificate formed for view {}", cert.data.0 + 1)
            }
            Effect::StartViewSent { view, target, .. } => {
                format!("Sent start view {} to {}", view.0, target.0)
            }
            Effect::StartViewRecorded { sender, start_view } => {
                format!(
                    "Recorded start view {} from {}",
                    start_view.data.view.0, sender.0
                )
            }
            Effect::ComplaintSent { qc, target } => {
                format!(
                    "Sent complaint about {:?} to {}",
                    qc.data.for_which, target.0
                )
            }
            Effect::PendingVoteProcessed {
                view,
                vote_type,
                block_type,
                block_key,
            } => {
                format!(
                    "Processed pending {}-vote for {:?} block {:?} in view {}",
                    vote_type, block_type, block_key, view.0
                )
            }
        }
    }

    /// Check if this effect represents a message that should be sent
    pub fn is_outgoing_message(&self) -> bool {
        matches!(
            self,
            Effect::MessageSent { .. }
                | Effect::BlockProduced { .. }
                | Effect::VoteSent { .. }
        )
    }
}
