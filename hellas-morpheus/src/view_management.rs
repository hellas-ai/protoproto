use std::sync::Arc;

use crate::format::format_message;
use crate::*;

/// View management logic for the Morpheus protocol
impl MorpheusProcess {
    /// Sets the current time for this process
    ///
    /// Used to track timeouts for the protocol.
    pub fn set_now(&mut self, now: u128) {
        self.current_time = now;
    }

    /// Sets the throughput phase for the current view
    ///
    /// The protocol operates in two phases:
    /// - High throughput phase (0): Using leader blocks to order transaction blocks
    /// - Low throughput phase (1): Directly finalizing transaction blocks
    pub fn set_phase(&mut self, phase: Phase) {
        self.phase_i.insert(self.view_i, phase);
    }

    /// Determines if a given process is the leader for a specified view
    ///
    /// The leader for each view is deterministically chosen based on the view number.
    /// This function validates whether a given process is indeed the leader for a view.
    pub fn verify_leader(&self, author: Identity, view: ViewNum) -> bool {
        author.0 as usize == 1 + (view.0 as usize % self.n)
    }

    /// Returns the leader for a specified view
    ///
    /// The leader for view v is process p_(v mod n)+1, ensuring leader rotation
    /// across all n processes.
    pub fn lead(&self, view: ViewNum) -> Identity {
        Identity((view.0 as u64 % self.n as u64) + 1) // identities are 1-indexed
    }

    /// Transitions to a new view
    ///
    /// Handles all necessary operations when changing to a new view:
    /// 1. Updates the current view number
    /// 2. Resets the view entry time
    /// 3. Sets the initial phase (High)
    /// 4. Sends the new view information to all processes
    /// 5. Sends tips to the new leader
    /// 6. Re-evaluates pending votes
    pub(crate) fn end_view(
        &mut self,
        cause: Message,
        new_view: ViewNum,
        to_send: &mut Vec<(Message, Option<Identity>)>,
    ) {
        // Record view change with tracing
        crate::tracing_setup::protocol_transition(
            &self.id,
            "view_change",
            self.view_i,
            new_view,
            Some(&format_message(&cause, false)),
        );

        // Ensure we only move forward in views, not backward
        assert!(self.view_i <= new_view);

        // Update view state
        self.view_i = new_view;
        self.view_entry_time = self.current_time;
        self.phase_i.insert(new_view, Phase::High);

        // Mark pending votes for reevaluation in the new view
        self.pending_votes.entry(new_view).or_default().dirty = true;

        // Broadcast the cause of the view change to all processes
        self.send_msg(to_send, (cause, None));

        // Send all tips we've created to the new leader
        // "Send all tips q' of Q_i such that q'.auth = p_i to lead(v)"
        for tip in self.index.tips.clone() {
            if tip.for_which.author == Some(self.id.clone()) {
                self.send_msg(
                    to_send,
                    (
                        Message::QC(self.index.qcs.get(&tip).unwrap().clone()),
                        Some(self.lead(new_view)),
                    ),
                );
            }
        }

        // Send a StartView message to the new leader containing our highest 1-QC
        self.send_msg(
            to_send,
            (
                Message::StartView(Arc::new(Signed::from_data(
                    StartView {
                        view: new_view,
                        qc: ThreshSigned::clone(&self.index.max_1qc),
                    },
                    &self.kb,
                ))),
                Some(self.lead(new_view)),
            ),
        );

        // Re-evaluate any pending voting decisions after view change
        self.reevaluate_pending_votes(to_send);
    }
}
