use muchin::automaton::{Action, ActionKind, Redispatch, Uid};
use serde::{Deserialize, Serialize};
use type_uuid::TypeUuid;

use super::types::*;

/// Block-related actions
#[derive(Clone, PartialEq, Eq, TypeUuid, Serialize, Deserialize, Debug)]
#[uuid = "f8a7b6c5-d4e3-42f1-b0a9-c8d7e6f5a4b3"]
pub enum BlockAction {
    /// Process a received block
    ProcessBlock {
        block: Block,
    },
    
    /// Create a transaction block
    CreateTransactionBlock,
    
    /// Create a leader block
    CreateLeaderBlock,
    
    /// Block creation succeeded
    BlockCreated {
        block: Block,
        hash: Hash,
    },
}

/// Voting-related actions
#[derive(Clone, PartialEq, Eq, TypeUuid, Serialize, Deserialize, Debug)]
#[uuid = "e7d6c5b4-a3f2-41e0-9d8c-b7a6f5e4d3c2"]
pub enum VotingAction {
    /// Process a received vote
    ProcessVote {
        vote: Vote,
    },
    
    /// Process a received QC
    ProcessQC {
        qc: QC,
    },
    
    /// Form a QC from votes
    FormQC {
        vote_type: VoteType,
        block_hash: Hash,
    },
    
    /// Check if a block is eligible for voting
    CheckVoteEligibility {
        block: Block,
        block_hash: Hash,
    },
    
    /// Send a vote
    SendVote {
        vote_type: VoteType,
        block_type: BlockType,
        view: View,
        height: Height,
        author: ProcessId,
        slot: Slot,
        block_hash: Hash,
    },
}

/// View change-related actions
#[derive(Clone, PartialEq, Eq, TypeUuid, Serialize, Deserialize, Debug)]
#[uuid = "d6c5b4a3-92e1-40df-8c7b-a6f5e4d3c2b1"]
pub enum ViewChangeAction {
    /// Process an end-view message
    ProcessEndView {
        message: EndViewMessage,
    },
    
    /// Process a view certificate
    ProcessViewCertificate {
        certificate: ViewCertificate,
    },
    
    /// Process a view message
    ProcessViewMessage {
        message: ViewMessage,
    },
    
    /// Form a view certificate from end-view messages
    FormViewCertificate {
        view: View,
    },
    
    /// Send an end-view message
    SendEndView {
        view: View,
    },
    
    /// Update view
    UpdateView {
        new_view: View,
    },
    
    /// Check timeouts
    CheckTimeouts {
        current_time: u64,
    },
}

/// Unified action type for Morpheus protocol
#[derive(Clone, PartialEq, Eq, TypeUuid, Serialize, Deserialize, Debug)]
#[uuid = "c5b4a392-81d0-4cde-7b6a-f5e4d3c2b1a0"]
pub enum MorpheusAction {
    /// Block-related actions
    Block(BlockAction),
    
    /// Voting-related actions
    Voting(VotingAction),
    
    /// View change-related actions
    ViewChange(ViewChangeAction),
    
    /// Tick action
    Tick,
}

impl Action for MorpheusAction {
    const KIND: ActionKind = ActionKind::Pure;
}

impl Action for BlockAction {
    const KIND: ActionKind = ActionKind::Pure;
}

impl Action for VotingAction {
    const KIND: ActionKind = ActionKind::Pure;
}

impl Action for ViewChangeAction {
    const KIND: ActionKind = ActionKind::Pure;
}

/// Network-related actions (effectful)
#[derive(Clone, PartialEq, Eq, TypeUuid, Serialize, Deserialize, Debug)]
#[uuid = "b4a39281-70cf-4bcd-6a59-e4d3c2b1a098"]
pub enum NetworkAction {
    /// Broadcast a block to all processes
    BroadcastBlock {
        block: Block,
        on_success: Redispatch<(Block, Hash)>,
        on_error: Redispatch<String>,
    },
    
    /// Send a 0-vote to the block creator
    SendVoteToProcess {
        vote: Vote,
        recipient: ProcessId,
        on_success: Redispatch<()>,
        on_error: Redispatch<String>,
    },
    
    /// Broadcast a vote to all processes (for 1-votes and 2-votes)
    BroadcastVote {
        vote: Vote,
        on_success: Redispatch<Vote>,
        on_error: Redispatch<String>,
    },
    
    /// Broadcast a QC to all processes
    BroadcastQC {
        qc: QC,
        on_success: Redispatch<QC>,
        on_error: Redispatch<String>,
    },
    
    /// Send a view message to the leader
    SendViewMessage {
        message: ViewMessage,
        recipient: ProcessId,
        on_success: Redispatch<ViewMessage>,
        on_error: Redispatch<String>,
    },
    
    /// Broadcast an end-view message to all processes
    BroadcastEndView {
        message: EndViewMessage,
        on_success: Redispatch<EndViewMessage>,
        on_error: Redispatch<String>,
    },
    
    /// Broadcast a view certificate to all processes
    BroadcastViewCertificate {
        certificate: ViewCertificate,
        on_success: Redispatch<ViewCertificate>,
        on_error: Redispatch<String>,
    },
    
    /// Send QC to leader after not being finalized for 6Δ
    SendQCToLeader {
        qc: QC,
        recipient: ProcessId,
        on_success: Redispatch<()>,
        on_error: Redispatch<String>,
    },
}

impl Action for NetworkAction {
    const KIND: ActionKind = ActionKind::Effectful;
}