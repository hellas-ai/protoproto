use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::{Duration, Instant};
use log::{debug, info, warn};
use muchin::automaton::Dispatcher;
use muchin::callback;

use super::types::*;
use super::state::MorpheusState;
use super::actions::{ViewChangeAction, NetworkAction, MorpheusAction};

/// View State - Extracted from MorpheusState for view-specific operations
#[derive(Debug)]
pub struct ViewState {
    /// End-view messages by view
    pub end_view_messages: BTreeMap<View, Vec<EndViewMessage>>,
    
    /// View certificates by view
    pub view_certificates: BTreeMap<View, ViewCertificate>,
    
    /// View messages by view
    pub view_messages: BTreeMap<View, Vec<ViewMessage>>,
    
    /// Time when we entered each view
    pub view_entry_times: BTreeMap<View, Instant>,
    
    /// Timeouts for the current view
    pub view_timeouts: Option<ViewTimeouts>,
    
    /// QCs sent to leader in the current view
    pub qcs_sent_to_leader: BTreeSet<Hash>,
}

impl ViewState {
    /// Initialize a new ViewState
    pub fn new() -> Self {
        Self {
            end_view_messages: BTreeMap::new(),
            view_certificates: BTreeMap::new(),
            view_messages: BTreeMap::new(),
            view_entry_times: BTreeMap::new(),
            view_timeouts: None,
            qcs_sent_to_leader: BTreeSet::new(),
        }
    }
    
    /// Add an end-view message
    pub fn add_end_view_message(&mut self, message: EndViewMessage) {
        self.end_view_messages
            .entry(message.view)
            .or_insert_with(Vec::new)
            .push(message);
    }
    
    /// Add a view certificate
    pub fn add_view_certificate(&mut self, certificate: ViewCertificate) {
        self.view_certificates.insert(certificate.view, certificate);
    }
    
    /// Add a view message
    pub fn add_view_message(&mut self, message: ViewMessage) {
        self.view_messages
            .entry(message.view)
            .or_insert_with(Vec::new)
            .push(message);
    }
    
    /// Record that a QC was sent to the leader
    pub fn mark_qc_sent_to_leader(&mut self, qc_hash: Hash) {
        self.qcs_sent_to_leader.insert(qc_hash);
    }
    
    /// Check if a QC was sent to the leader
    pub fn was_qc_sent_to_leader(&self, qc_hash: &Hash) -> bool {
        self.qcs_sent_to_leader.contains(qc_hash)
    }
    
    /// Record the time when we entered a view
    pub fn record_view_entry(&mut self, view: View, delta: Duration) {
        let now = Instant::now();
        self.view_entry_times.insert(view, now);
        
        // Setup timeouts for this view
        self.view_timeouts = Some(ViewTimeouts {
            view,
            view_entry_time: now,
            complaint_timeout: delta * 6,
            end_view_timeout: delta * 12,
            complained: false,
            sent_end_view: false,
        });
        
        // Reset QCs sent to leader
        self.qcs_sent_to_leader.clear();
    }
    
    /// Get the time when we entered a view
    pub fn get_view_entry_time(&self, view: View) -> Option<Instant> {
        self.view_entry_times.get(&view).copied()
    }
    
    /// Prune old state (from views earlier than min_view)
    pub fn prune_old_state(&mut self, min_view: View) {
        // Only keep view-related data for recent views
        self.end_view_messages.retain(|view, _| *view >= min_view);
        self.view_certificates.retain(|view, _| *view >= min_view);
        self.view_messages.retain(|view, _| *view >= min_view);
        self.view_entry_times.retain(|view, _| *view >= min_view);
    }
    
    /// Check if we have a quorum of processes that have entered a view
    pub fn has_quorum_entered_view(&self, view: View, quorum_size: usize) -> bool {
        let view_messages = self.view_messages
            .get(&view)
            .map(|msgs| msgs.len())
            .unwrap_or(0);
        
        view_messages >= quorum_size
    }
}

/// Process an end-view message
pub fn process_end_view_message(
    state: &mut MorpheusState,
    message: EndViewMessage,
    dispatcher: &mut Dispatcher,
) {
    debug!(
        "Process {}: Processing end-view message for view {} from {}",
        state.process_id,
        message.view,
        message.signer
    );
    
    // Add message to state
    state.view_state.add_end_view_message(message.clone());
    
    // Check if we have enough messages to form a certificate
    let messages = state.view_state.end_view_messages
        .get(&message.view)
        .map(|m| m.len())
        .unwrap_or(0);
    
    // Need f+1 messages
    if messages > state.f {
        // Form a view certificate
        dispatcher.dispatch(MorpheusAction::ViewChange(ViewChangeAction::FormViewCertificate { 
            view: message.view 
        }));
    }
}

/// Process a view certificate
pub fn process_view_certificate(
    state: &mut MorpheusState,
    certificate: ViewCertificate,
    dispatcher: &mut Dispatcher,
) {
    debug!(
        "Process {}: Processing view certificate for view {}",
        state.process_id,
        certificate.view
    );
    
    // Add certificate to state
    state.view_state.add_view_certificate(certificate.clone());
    
    // Update view if certificate's view is greater than current view
    if certificate.view.0 > state.current_view.0 {
        dispatcher.dispatch(MorpheusAction::ViewChange(ViewChangeAction::UpdateView { 
            new_view: certificate.view 
        }));
    }
}

/// Process a view message
pub fn process_view_message(
    state: &mut MorpheusState,
    message: ViewMessage,
) {
    debug!(
        "Process {}: Processing view message for view {} from {}",
        state.process_id,
        message.view,
        message.signer
    );
    
    // Add message to state
    state.view_state.add_view_message(message);
}

/// Form a view certificate
pub fn form_view_certificate(
    state: &mut MorpheusState,
    view: View,
    dispatcher: &mut Dispatcher,
) {
    // Get the messages
    let messages = state.view_state.end_view_messages
        .get(&view)
        .map(|m| m.len())
        .unwrap_or(0);
    
    // Need f+1 messages
    if messages <= state.f {
        return;
    }
    
    // Create the certificate
    let certificate = ViewCertificate {
        view: View(view.0 + 1), // Next view
        signatures: ThresholdSignature(vec![]), // placeholder
    };
    
    // Broadcast the certificate
    dispatcher.dispatch_effect(NetworkAction::BroadcastViewCertificate {
        certificate: certificate.clone(),
        on_success: callback!(|certificate: ViewCertificate| 
            MorpheusAction::ViewChange(ViewChangeAction::ProcessViewCertificate { certificate })),
        on_error: callback!(|error: String| 
            MorpheusAction::ViewChange(ViewChangeAction::SendEndView { view })),
    });
}

/// Send an end-view message
pub fn send_end_view(
    state: &mut MorpheusState,
    view: View,
    dispatcher: &mut Dispatcher,
) {
    // Create the message
    let message = EndViewMessage {
        view,
        signer: state.process_id,
        signature: Signature(vec![]), // placeholder
    };
    
    // Mark as sent in timeouts
    if let Some(timeouts) = &mut state.view_state.view_timeouts {
        if timeouts.view == view {
            timeouts.sent_end_view = true;
        }
    }
    
    // Broadcast the message
    dispatcher.dispatch_effect(NetworkAction::BroadcastEndView {
        message: message.clone(),
        on_success: callback!(|message: EndViewMessage| 
            MorpheusAction::ViewChange(ViewChangeAction::ProcessEndView { message })),
        on_error: callback!(|error: String| 
            MorpheusAction::ViewChange(ViewChangeAction::SendEndView { view })),
    });
}

/// Update view to a new view
pub fn update_view(
    state: &mut MorpheusState,
    new_view: View,
    dispatcher: &mut Dispatcher,
) -> bool {
    if new_view.0 <= state.current_view.0 {
        return false; // Not a higher view
    }
    
    // Log the view change
    info!(
        "Process {} transitioning from view {} to view {}",
        state.process_id,
        state.current_view,
        new_view
    );
    
    // Update view and reset state
    state.current_view = new_view;
    state.phase = ThroughputPhase::High; // Always start in high throughput
    state.transaction_slot = Slot(0);
    state.leader_slot = Slot(0);
    
    // Record entry time
    state.view_state.record_view_entry(new_view, state.delta);
    
    // Clear any vote state for the previous view
    state.vote_state.voted.retain(|key| {
        // Keep votes from other views, clear votes for current view
        key.block_type != BlockType::Transaction || 
        state.block_state.block_index.iter()
            .filter(|((block_type, view, _, _), _)| *block_type == key.block_type && *view == new_view)
            .any(|((_, _, slot, author), _)| *slot == key.slot && *author == key.author)
    });
    
    // Send certificate to all processes
    if let Some(certificate) = state.view_state.view_certificates.get(&new_view) {
        dispatcher.dispatch_effect(NetworkAction::BroadcastViewCertificate {
            certificate: certificate.clone(),
            on_success: callback!(|certificate: ViewCertificate| 
                MorpheusAction::ViewChange(ViewChangeAction::ProcessViewCertificate { certificate })),
            on_error: callback!(|error: String| 
                MorpheusAction::ViewChange(ViewChangeAction::SendEndView { view: state.current_view })),
        });
    }
    
    // Send tips to leader
    let leader = MorpheusState::get_leader(new_view, state.num_processes);
    
    // Send QCs for our own blocks to leader
    for ((vote_type, block_hash), qc) in &state.vote_state.qcs {
        if *vote_type == VoteType::Vote1 && 
           (qc.author == state.process_id || state.block_state.tips.contains(block_hash)) {
            dispatcher.dispatch_effect(NetworkAction::SendQCToLeader {
                qc: qc.clone(),
                recipient: leader,
                on_success: callback!(|()| MorpheusAction::Voting(
                    crate::voting::VotingAction::ProcessQC { qc: qc.clone() }
                )),
                on_error: callback!(|error: String| MorpheusAction::ViewChange(
                    ViewChangeAction::SendEndView { view: new_view }
                )),
            });
        }
    }
    
    // Send view message to leader
    let view_message = ViewMessage {
        view: new_view,
        qc: state.vote_state.greatest_1qc.clone().unwrap_or_else(|| QC {
            vote_type: VoteType::Vote1,
            block_type: BlockType::Genesis,
            view: View(0),
            height: Height(0),
            author: ProcessId(0),
            slot: Slot(0),
            block_hash: Hash(vec![]),
            signatures: ThresholdSignature(vec![]),
        }),
        signer: state.process_id,
        signature: Signature(vec![]), // placeholder
    };
    
    dispatcher.dispatch_effect(NetworkAction::SendViewMessage {
        message: view_message.clone(),
        recipient: leader,
        on_success: callback!(|message: ViewMessage| 
            MorpheusAction::ViewChange(ViewChangeAction::ProcessViewMessage { message })),
        on_error: callback!(|error: String| 
            MorpheusAction::ViewChange(ViewChangeAction::SendEndView { view: new_view })),
    });
    
    true
}

/// Check for timeouts
pub fn check_timeouts(
    state: &mut MorpheusState,
    current_time: u64,
    dispatcher: &mut Dispatcher,
) {
    if let Some(timeouts) = &mut state.view_state.view_timeouts {
        if timeouts.view != state.current_view {
            // Timeouts don't match current view, reset them
            state.view_state.view_timeouts = Some(ViewTimeouts {
                view: state.current_view,
                view_entry_time: Instant::now(),
                complaint_timeout: state.delta * 6,
                end_view_timeout: state.delta * 12,
                complained: false,
                sent_end_view: false,
            });
            return;
        }
        
        let elapsed = timeouts.view_entry_time.elapsed();
        
        // Check for 6Δ timeout (complaint timeout)
        if !timeouts.complained && elapsed >= timeouts.complaint_timeout {
            // Send pending QCs to leader
            timeouts.complained = true;
            
            // Per pseudocode line 56, send QCs that haven't been finalized
            for ((vote_type, block_hash), qc) in &state.vote_state.qcs {
                if !state.vote_state.is_finalized(block_hash) &&
                   !state.view_state.was_qc_sent_to_leader(block_hash) {
                    
                    // Send QC to leader
                    let leader = MorpheusState::get_leader(state.current_view, state.num_processes);
                    
                    dispatcher.dispatch_effect(NetworkAction::SendQCToLeader {
                        qc: qc.clone(),
                        recipient: leader,
                        on_success: callback!(|()| MorpheusAction::Tick),
                        on_error: callback!(|error: String| MorpheusAction::ViewChange(
                            ViewChangeAction::SendEndView { view: state.current_view }
                        )),
                    });
                    
                    // Mark as sent to leader
                    state.view_state.mark_qc_sent_to_leader(block_hash.clone());
                }
            }
        }
        
        // Check for 12Δ timeout (end-view timeout)
        if !timeouts.sent_end_view && elapsed >= timeouts.end_view_timeout {
            // Send end-view message (line 58-59 in pseudocode)
            dispatcher.dispatch(MorpheusAction::ViewChange(
                ViewChangeAction::SendEndView { view: state.current_view }
            ));
        }
    }
}

/// Process a view change action
pub fn process_view_change_action(
    state: &mut MorpheusState,
    action: ViewChangeAction,
    dispatcher: &mut Dispatcher,
) {
    match action {
        ViewChangeAction::ProcessEndView { message } => {
            process_end_view_message(state, message, dispatcher);
        },
        ViewChangeAction::ProcessViewCertificate { certificate } => {
            process_view_certificate(state, certificate, dispatcher);
        },
        ViewChangeAction::ProcessViewMessage { message } => {
            process_view_message(state, message);
        },
        ViewChangeAction::FormViewCertificate { view } => {
            form_view_certificate(state, view, dispatcher);
        },
        ViewChangeAction::SendEndView { view } => {
            send_end_view(state, view, dispatcher);
        },
        ViewChangeAction::UpdateView { new_view } => {
            update_view(state, new_view, dispatcher);
        },
        ViewChangeAction::CheckTimeouts { current_time } => {
            check_timeouts(state, current_time, dispatcher);
        },
    }
}