use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use muchin::automaton::{
    Dispatcher, EffectfulModel, Effectful, ModelState, PureModel, 
    RegisterModel, RunnerBuilder, State, Uid
};
use muchin::callback;
use log::{debug, info, warn};

use super::state::MorpheusState;
use super::actions::{MorpheusAction, NetworkAction, BlockAction, VotingAction, ViewChangeAction};
use super::types::*;
use super::blocks;
use super::voting;
use super::view_change;

/// Pure model for the Morpheus protocol
pub struct MorpheusModel;

impl RegisterModel for MorpheusModel {
    fn register<Substate: ModelState>(builder: RunnerBuilder<Substate>) -> RunnerBuilder<Substate> {
        builder
            .register::<NetworkModel>()
            .model_pure::<Self>()
    }
}

impl PureModel for MorpheusModel {
    type Action = MorpheusAction;

    fn process_pure<Substate: ModelState>(
        state: &mut State<Substate>,
        action: Self::Action,
        dispatcher: &mut Dispatcher,
    ) {
        let morph_state: &mut MorpheusState = state.substate_mut();
        
        match action {
            MorpheusAction::Block(block_action) => {
                debug!("Processing block action: {:?}", block_action);
                blocks::process_block_action(morph_state, block_action, dispatcher);
            },
            
            MorpheusAction::Voting(voting_action) => {
                debug!("Processing voting action: {:?}", voting_action);
                voting::process_voting_action(morph_state, voting_action, dispatcher);
            },
            
            MorpheusAction::ViewChange(view_change_action) => {
                debug!("Processing view change action: {:?}", view_change_action);
                view_change::process_view_change_action(morph_state, view_change_action, dispatcher);
            },
            
            MorpheusAction::Tick => {
                // Get current time
                let current_time = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64;
                
                // Check timeouts
                dispatcher.dispatch(MorpheusAction::ViewChange(
                    ViewChangeAction::CheckTimeouts { current_time }
                ));
                
                // Check if PayloadReady and trigger block creation if needed
                if morph_state.payload_ready && !morph_state.pending_transactions.is_empty() {
                    dispatcher.dispatch(MorpheusAction::Block(
                        BlockAction::CreateTransactionBlock
                    ));
                }
                
                // Check if we're the leader and ready to create a leader block
                if morph_state.is_leader(morph_state.current_view) &&
                   morph_state.is_leader_ready() &&
                   morph_state.phase == ThroughputPhase::High {
                    // Only create leader blocks in high throughput phase
                    if !morph_state.block_state.is_single_tip(
                        morph_state.block_state.tips.iter().next().unwrap_or(&Hash([0u8; 32]))) {
                        // Only if we don't have a single tip
                        dispatcher.dispatch(MorpheusAction::Block(
                            BlockAction::CreateLeaderBlock
                        ));
                    }
                }
                
                // Periodically prune old state (e.g., keep only most recent 10 views)
                if morph_state.current_view.0 > 10 {
                    let min_view = View(morph_state.current_view.0 - 10);
                    morph_state.prune_old_state(min_view);
                }
            },
        }
    }
}

/// Effectful model for network communication
pub struct NetworkModel;

impl EffectfulModel for NetworkModel {
    type Action = NetworkAction;

    fn process_effectful(&mut self, action: Self::Action, dispatcher: &mut Dispatcher) {
        match action {
            NetworkAction::BroadcastBlock { block, on_success, on_error } => {
                // In a real implementation, this would send the block to all processes
                // For now, we'll just simulate success
                debug!("Broadcasting block: {:?}", block.block_type);
                let hash = block.hash();
                dispatcher.dispatch_back(&on_success, (block, hash));
            },
            NetworkAction::SendVoteToProcess { vote, recipient, on_success, on_error } => {
                // In a real implementation, this would send the vote to the recipient
                debug!("Sending vote type {:?} to process {}", vote.vote_type, recipient);
                dispatcher.dispatch_back(&on_success, vote);
            },
            NetworkAction::BroadcastVote { vote, on_success, on_error } => {
                // In a real implementation, this would broadcast the vote to all processes
                debug!("Broadcasting vote type {:?}", vote.vote_type);
                dispatcher.dispatch_back(&on_success, vote);
            },
            NetworkAction::BroadcastQC { qc, on_success, on_error } => {
                // In a real implementation, this would broadcast the QC to all processes
                debug!("Broadcasting QC type {:?}", qc.vote_type);
                dispatcher.dispatch_back(&on_success, qc);
            },
            NetworkAction::SendViewMessage { message, recipient, on_success, on_error } => {
                // In a real implementation, this would send the view message to the leader
                debug!("Sending view message for view {} to {}", message.view, recipient);
                dispatcher.dispatch_back(&on_success, message);
            },
            NetworkAction::BroadcastEndView { message, on_success, on_error } => {
                // In a real implementation, this would broadcast the end-view message to all processes
                debug!("Broadcasting end-view message for view {}", message.view);
                dispatcher.dispatch_back(&on_success, message);
            },
            NetworkAction::BroadcastViewCertificate { certificate, on_success, on_error } => {
                // In a real implementation, this would broadcast the view certificate to all processes
                debug!("Broadcasting view certificate for view {}", certificate.view);
                dispatcher.dispatch_back(&on_success, certificate);
            },
            NetworkAction::SendQCToLeader { qc, recipient, on_success, on_error } => {
                // In a real implementation, this would send the QC to the leader
                debug!("Sending QC for block {:?} to leader {}", qc.block_hash, recipient);
                dispatcher.dispatch_back(&on_success, qc);
            },
        }
    }
}

impl RegisterModel for NetworkModel {
    fn register<Substate: ModelState>(builder: RunnerBuilder<Substate>) -> RunnerBuilder<Substate> {
        builder.model_effectful(Effectful::<Self>(Self))
    }
}