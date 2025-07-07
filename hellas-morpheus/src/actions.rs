use serde::{Deserialize, Serialize};
use crate::*;

/// External actions that trigger state transitions in the protocol
/// These represent observable, externally-useful events that can be visualized
#[derive(Clone, PartialEq, Eq, PartialOrd, Hash, Ord, Serialize, Deserialize, Debug)]
pub enum Action<Tr: Transaction> {
    /// Process an incoming message from another node
    ProcessMessage {
        sender: Identity,
        #[serde(bound(serialize = "Tr: Transaction", deserialize = "Tr: Transaction"))]
        payload: Message<Tr>,
    },
    
    /// Update the current time
    SetTime(u128),
    
    /// Provide new transactions for block production
    #[serde(with = "ark_serialize::vec_compressed_checked")]
    SetReadyTransactions(Vec<Tr>),
    
    /// Check for timeouts and trigger appropriate actions
    CheckTimeouts,
    
    /// Attempt to produce blocks if conditions are met
    CheckProduceBlocks,
}

impl<Tr: Transaction> Action<Tr> {
    /// Get a human-readable description of the action for visualization
    pub fn description(&self) -> String {
        match self {
            Action::ProcessMessage { sender, payload } => {
                format!("Process message from {} - {}", sender.0, payload.description())
            }
            Action::SetTime(time) => format!("Set time to {}", time),
            Action::SetReadyTransactions(txs) => {
                format!("Set {} ready transactions", txs.len())
            }
            Action::CheckTimeouts => "Check for timeouts".to_string(),
            Action::CheckProduceBlocks => "Check block production".to_string(),
        }
    }
}

impl<Tr: Transaction> Message<Tr> {
    /// Get a human-readable description of the message for visualization
    pub fn description(&self) -> String {
        match self {
            Message::Block(block) => format!("{:?} block", block.data.key.type_),
            Message::NewVote(vote) => format!("{}-vote for {:?}", vote.data.z, vote.data.for_which),
            Message::QC(qc) => format!("{}-QC for {:?}", qc.data.z, qc.data.for_which),
            Message::EndView(view) => format!("End view {}", view.data.0),
            Message::EndViewCert(cert) => format!("End view certificate for view {}", cert.data.0),
            Message::StartView(sv) => format!("Start view {}", sv.data.view.0),
        }
    }
} 