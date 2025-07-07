use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::*;

/// Internal state mutations produced by processing actions
/// These are pure data representing state changes that can be recorded and replayed
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum Effect<Tr: Transaction> {
    /// Update current time
    TimeUpdated(u128),
    
    /// View transition
    ViewChanged {
        old_view: ViewNum,
        new_view: ViewNum,
        cause: String, // Description of what caused the view change
    },
    
    /// Phase transition within a view
    PhaseChanged {
        view: ViewNum,
        old_phase: Phase,
        new_phase: Phase,
    },
    
    /// Block was recorded
    BlockRecorded {
        #[serde(bound(serialize = "Tr: Transaction", deserialize = "Tr: Transaction"))]
        block: Arc<Signed<Block<Tr>>>,
    },
    
    /// QC was recorded
    QcRecorded {
        qc: FinishedQC,
        finalized_blocks: Vec<BlockKey>, // Blocks finalized by this QC
    },
    
    /// Vote was sent
    VoteSent {
        vote_type: u8, // 0, 1, or 2
        block_key: BlockKey,
        target: Option<Identity>, // None means broadcast
    },
    
    /// Vote was recorded from another process
    VoteRecorded {
        voter: Identity,
        vote_data: VoteData,
    },
    
    /// Quorum reached for a vote
    QuorumReached {
        vote_data: VoteData,
        qc_formed: FinishedQC,
    },
    
    /// Ready transactions updated
    TransactionsUpdated {
        #[serde(bound(serialize = "Tr: Transaction", deserialize = "Tr: Transaction"))]
        transactions: Vec<Tr>,
    },
    
    /// Block produced
    BlockProduced {
        block_type: BlockType,
        block_key: BlockKey,
    },
    
    /// Message sent
    MessageSent {
        #[serde(bound(serialize = "Tr: Transaction", deserialize = "Tr: Transaction"))]
        message: Message<Tr>,
        target: Option<Identity>, // None means broadcast
    },
    
    /// Complaint sent
    ComplaintSent {
        qc: FinishedQC,
        target: Identity,
    },
    
    /// End-view message sent
    EndViewSent {
        view: ViewNum,
    },
    
    /// View certificate formed
    ViewCertificateFormed {
        view: ViewNum,
    },
    
    /// Slot advanced
    SlotAdvanced {
        slot_type: BlockType, // Lead or Tr
        new_slot: SlotNum,
    },
    
    /// Marked that we produced a leader block in view
    LeaderBlockProducedInView {
        view: ViewNum,
    },
}

impl<Tr: Transaction> Effect<Tr> {
    /// Get a human-readable description of the effect for visualization
    pub fn description(&self) -> String {
        match self {
            Effect::TimeUpdated(time) => format!("Time updated to {}", time),
            Effect::ViewChanged { old_view, new_view, cause } => {
                format!("View changed from {} to {} ({})", old_view.0, new_view.0, cause)
            }
            Effect::PhaseChanged { view, old_phase, new_phase } => {
                format!("Phase changed in view {} from {:?} to {:?}", view.0, old_phase, new_phase)
            }
            Effect::BlockRecorded { block } => {
                format!("Recorded {:?} block at height {}", block.data.key.type_, block.data.key.height)
            }
            Effect::QcRecorded { qc, finalized_blocks } => {
                format!("{}-QC recorded, finalized {} blocks", qc.data.z, finalized_blocks.len())
            }
            Effect::VoteSent { vote_type, block_key, target } => {
                let target_str = target.as_ref().map_or("all".to_string(), |t| format!("{}", t.0));
                format!("Sent {}-vote for {:?} to {}", vote_type, block_key, target_str)
            }
            Effect::VoteRecorded { voter, vote_data } => {
                format!("Recorded {}-vote from {} for {:?}", vote_data.z, voter.0, vote_data.for_which)
            }
            Effect::QuorumReached { vote_data, .. } => {
                format!("Quorum reached for {}-vote on {:?}", vote_data.z, vote_data.for_which)
            }
            Effect::TransactionsUpdated { transactions } => {
                format!("Updated ready transactions ({})", transactions.len())
            }
            Effect::BlockProduced { block_type, block_key } => {
                format!("Produced {:?} block: {:?}", block_type, block_key)
            }
            Effect::MessageSent { message, target } => {
                let target_str = target.as_ref().map_or("all".to_string(), |t| format!("{}", t.0));
                format!("Sent {} to {}", message.description(), target_str)
            }
            Effect::ComplaintSent { qc, target } => {
                format!("Sent complaint about {}-QC to {}", qc.data.z, target.0)
            }
            Effect::EndViewSent { view } => {
                format!("Sent end-view for view {}", view.0)
            }
            Effect::ViewCertificateFormed { view } => {
                format!("Formed view certificate for view {}", view.0)
            }
            Effect::SlotAdvanced { slot_type, new_slot } => {
                format!("{:?} slot advanced to {}", slot_type, new_slot.0)
            }
            Effect::LeaderBlockProducedInView { view } => {
                format!("Marked leader block produced in view {}", view.0)
            }
        }
    }
    
    /// Check if this effect represents a message that should be sent
    pub fn is_outgoing_message(&self) -> bool {
        matches!(self, 
            Effect::MessageSent { .. } | 
            Effect::VoteSent { .. } | 
            Effect::ComplaintSent { .. } |
            Effect::EndViewSent { .. }
        )
    }
} 