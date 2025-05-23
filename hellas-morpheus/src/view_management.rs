use std::{cmp::Ordering, sync::Arc};

use crate::*;

const COMPLAIN_TIMEOUT: u128 = 6;
const END_VIEW_TIMEOUT: u128 = 12;

impl<Tr: Transaction> MorpheusProcess<Tr> {
    pub fn set_now(&mut self, now: u128) {
        self.current_time = now;
    }

    pub fn set_phase(&mut self, phase: Phase) {
        self.phase_i.insert(self.view_i, phase);
    }

    pub fn verify_leader(&self, author: Identity, view: ViewNum) -> bool {
        author.0 as u32 == 1 + (view.0 as u32 % self.n)
    }

    pub fn lead(&self, view: ViewNum) -> Identity {
        Identity((view.0 as u32 % self.n as u32) + 1) // identities are 1-indexed... ok
    }

    pub(crate) fn end_view(
        &mut self,
        cause: Message<Tr>,
        new_view: ViewNum,
        to_send: &mut Vec<(Message<Tr>, Option<Identity>)>,
    ) {
        // Record view change with tracing
        crate::tracing_setup::protocol_transition(
            &self.id,
            "view_change",
            self.view_i,
            new_view,
            Some(&format::format_message(&cause, false)),
        );

        assert!(self.view_i <= new_view);

        self.view_i = new_view;
        self.view_entry_time = self.current_time;
        self.phase_i.insert(new_view, Phase::High);

        // View changed, we need to re-evaluate pending votes
        self.pending_votes.entry(new_view).or_default().dirty = true;

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
        self.send_msg(
            to_send,
            (
                Message::StartView(Arc::new(Signed::from_data(
                    StartView {
                        view: new_view,
                        qc: self.index.max_1qc.clone(),
                    },
                    &self.kb,
                ))),
                Some(self.lead(new_view)),
            ),
        );

        // Re-evaluate any pending voting decisions after view change
        self.reevaluate_pending_votes(to_send);
    }

    /// Implements the "Complain" section from Algorithm 1
    ///
    /// Checks timeouts and sends complaints:
    /// "If ∃q ∈ Q_i which is maximal according to ⪰ amongst those that have not been finalized for
    ///  time 6Δ since entering view view_i: Send q to lead(view_i) if not previously sent;"
    /// "If ∃q ∈ Q_i which has not been finalized for time 12Δ since entering view view_i:
    ///  Send the end-view message (view_i) signed by p_i to all processes;"
    pub fn check_timeouts(&mut self, to_send: &mut Vec<(Message<Tr>, Option<Identity>)>) {
        let time_in_view = self.current_time - self.view_entry_time;

        if time_in_view >= self.delta * COMPLAIN_TIMEOUT {
            let maximal_unfinalized = self
                .index
                .unfinalized
                .iter()
                .flat_map(|(_, qcs)| qcs)
                .max_by(|&qc1, &qc2| {
                    // FIXME: when the paper says maximal, does it mean unique?
                    if self.observes(qc1.clone(), qc2) {
                        Ordering::Greater
                    } else if self.observes(qc2.clone(), qc1) {
                        Ordering::Less
                    } else {
                        Ordering::Equal
                    }
                });

            if let Some(qc_data) = maximal_unfinalized {
                if !self.complained_qcs.insert(qc_data.clone()) {
                    self.send_msg(
                        to_send,
                        (
                            Message::QC(self.index.qcs.get(&qc_data).cloned().unwrap()),
                            Some(self.lead(self.view_i)),
                        ),
                    );
                }
            }
        }

        // Second timeout - 12Δ, send end-view message
        if time_in_view >= self.delta * END_VIEW_TIMEOUT && !self.index.unfinalized.is_empty() {
            self.send_msg(
                to_send,
                (
                    Message::EndView(Arc::new(ThreshPartial::from_data(self.view_i, &self.kb))),
                    None,
                ),
            );
        }
    }
}
