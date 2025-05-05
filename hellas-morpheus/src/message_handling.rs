use std::cmp::Ordering;
use std::sync::Arc;

use crate::*;
use crate::format::format_message;

/// Functions for handling protocol messages in the Morpheus protocol
impl MorpheusProcess {
    /// Prepares and sends a message to one or all processes
    /// 
    /// If the target is None, the message is sent to all processes.
    /// If the target is Some, the message is sent only to that process.
    /// In either case, the process always processes its own messages immediately.
    pub(crate) fn send_msg(
        &mut self,
        to_send: &mut Vec<(Message, Option<Identity>)>,
        message: (Message, Option<Identity>),
    ) {
        // If message is sent to everyone or to self, process it immediately
        if message.1.is_none() || message.1.as_ref().unwrap() == &self.id {
            // IMPORTANT: implements note from page 8:
            // "In what follows, we suppose that, when a correct process sends a
            // message to 'all processes', it regards that message as
            // immediately received by itself"
            self.process_message(message.0.clone(), self.id.clone(), to_send);
        }
        to_send.push(message);
    }

    /// Processes a received message according to the protocol rules
    /// 
    /// This is the entry point for handling messages. It:
    /// 1. Checks if the message is a duplicate
    /// 2. Routes the message to the appropriate handler based on message type
    /// 3. Re-evaluates any pending voting decisions
    /// 4. Checks invariants (in debug mode)
    #[tracing::instrument(skip(self, sender, to_send), fields(process_id = ?self.id))]
    pub fn process_message(
        &mut self,
        message: Message,
        sender: Identity,
        to_send: &mut Vec<(Message, Option<Identity>)>,
    ) -> bool {
        // Check if we've seen this message before (duplicate detection)
        if cfg!(debug_assertions) {
            if self.received_messages.contains(&message) {
                tracing::error!(
                    target: "duplicate_message",
                    sender = ?sender,
                    full_message = format_message(&message, true),
                    "Ignoring duplicate message: why did we receive it?"
                );
                return false;
            }
        }

        // Record that we've received this message
        self.received_messages.insert(message.clone());
        tracing::debug!("received a message");

        // Route the message to the appropriate handler
        let result = match message {
            Message::Block(block) => self.handle_block_message(block, to_send),
            Message::NewVote(vote_data) => self.handle_vote_message(vote_data, to_send),
            Message::QC(qc) => self.handle_qc_message(qc, to_send),
            Message::EndView(end_view) => self.handle_end_view_message(end_view, to_send),
            Message::EndViewCert(end_view_cert) => self.handle_end_view_cert_message(end_view_cert, to_send),
            Message::StartView(start_view) => self.handle_start_view_message(start_view),
        };

        if !result {
            return false;
        }

        // Check invariants in debug mode
        if cfg!(debug_assertions) {
            let violations = self.check_invariants();
            assert!(
                violations.is_empty(),
                "Process {} has invariant violations: {:?}",
                self.id.0,
                violations
            );
        }

        // Re-evaluate any pending voting decisions
        self.reevaluate_pending_votes(to_send);

        true
    }

    /// Handles a received block message
    /// 
    /// Validates the block, votes for it if valid, and records it in the state.
    fn handle_block_message(
        &mut self,
        block: Arc<Signed<Block>>,
        to_send: &mut Vec<(Message, Option<Identity>)>,
    ) -> bool {
        // Validate the block
        if let Err(error) = self.block_valid(&block) {
            tracing::error!(
                target: "invalid_block",
                process_id = ?self.id,
                block_key = ?block.data.key,
                error = ?error,
            );
            return false;
        }

        // If valid, vote for it at level 0
        self.try_vote(
            0,
            &block.data.key,
            Some(block.data.key.author.clone().expect("validated")),
            to_send,
        );

        tracing::debug!(
            target: "valid_block",
            block_key = ?block.data.key,
        );

        // Record the block in our state
        self.record_block(&block);
        true
    }

    /// Handles a received vote message
    /// 
    /// Validates the vote signature and records it in the vote tracker.
    fn handle_vote_message(
        &mut self,
        vote_data: Arc<ThreshPartial<VoteData>>,
        to_send: &mut Vec<(Message, Option<Identity>)>,
    ) -> bool {
        // Validate the vote signature
        if !vote_data.valid_signature(&self.kb) {
            tracing::error!(
                target: "invalid_vote",
                process_id = ?self.id,
                vote_data = ?vote_data,
            );
            return false;
        }
        
        // Record the vote in our vote tracker
        self.record_vote(&vote_data, to_send)
    }

    /// Handles a received quorum certificate (QC) message
    /// 
    /// Validates the QC signature, records it, and updates the view if needed.
    fn handle_qc_message(
        &mut self,
        qc: Arc<ThreshSigned<VoteData>>,
        to_send: &mut Vec<(Message, Option<Identity>)>,
    ) -> bool {
        // Validate the QC signature
        if !qc.valid_signature(&self.kb) {
            tracing::error!(
                target: "invalid_qc",
                process_id = ?self.id,
                qc = ?qc,
            );
            return false;
        }
        
        // Record the QC in our state
        self.record_qc(qc);
        
        // If we've seen a QC for a newer view, update our view
        if self.index.max_view.0 > self.view_i {
            self.end_view(
                Message::QC(self.index.qcs.get(&self.index.max_view.1).cloned().unwrap()),
                self.index.max_view.0,
                to_send,
            );
        }
        
        true
    }

    /// Handles a received end-view message
    /// 
    /// Records the end-view vote and forms an end-view certificate when enough votes collected.
    fn handle_end_view_message(
        &mut self,
        end_view: Arc<ThreshPartial<ViewNum>>,
        to_send: &mut Vec<(Message, Option<Identity>)>,
    ) -> bool {
        // Validate the end-view signature
        if !end_view.valid_signature(&self.kb) {
            tracing::error!(
                target: "invalid_end_view",
                process_id = ?self.id,
                end_view = ?end_view,
            );
            return false;
        }
        
        // Record the end-view vote and check if we have enough votes (f+1)
        match self.end_views.record_vote(end_view.clone()) {
            Ok(num_votes) => {
                if end_view.data >= self.view_i && num_votes >= self.f + 1 {
                    // We have f+1 votes for ending this view, form an end-view certificate
                    let votes_now = self
                        .end_views
                        .votes
                        .get(&end_view.data)
                        .unwrap()
                        .values()
                        .map(|v| (v.author.0 as usize - 1, v.signature.clone()))
                        .collect::<Vec<_>>();
                    
                    // Aggregate the signatures into an end-view certificate
                    let agg = self.kb.hints_setup.aggregator();
                    let mut data = Vec::new();
                    end_view.data.serialize_compressed(&mut data).unwrap();
                    let signed = hints::sign_aggregate(
                        &agg,
                        hints::F::from((self.f + 1) as u64),
                        &votes_now,
                        &data,
                    )
                    .unwrap();
                    
                    // Send the end-view certificate to all processes
                    self.send_msg(
                        to_send,
                        (
                            Message::EndViewCert(Arc::new(ThreshSigned {
                                data: end_view.data,
                                signature: signed,
                            })),
                            None,
                        ),
                    );
                }
                true
            }
            Err(Duplicate) => false,
        }
    }

    /// Handles a received end-view certificate message
    /// 
    /// Validates the certificate and initiates a view change to the next view.
    fn handle_end_view_cert_message(
        &mut self,
        end_view_cert: Arc<ThreshSigned<ViewNum>>,
        to_send: &mut Vec<(Message, Option<Identity>)>,
    ) -> bool {
        // Validate the end-view certificate
        if !end_view_cert.valid_signature(&self.kb) {
            tracing::error!(
                target: "invalid_end_view_cert",
                process_id = ?self.id,
                end_view_cert = ?end_view_cert,
            );
            return false;
        }
        
        // Calculate the next view number
        let view = end_view_cert.data.incr();
        
        // If the new view is greater than or equal to our current view, change to it
        if view >= self.view_i {
            self.end_view(Message::EndViewCert(end_view_cert), view, to_send);
        }
        
        true
    }

    /// Handles a received start-view message
    /// 
    /// This message is sent to the leader of a new view, containing the highest QC seen.
    fn handle_start_view_message(&mut self, start_view: Arc<Signed<StartView>>) -> bool {
        // Validate the start-view signature
        if !start_view.valid_signature(&self.kb) {
            tracing::error!(
                target: "invalid_start_view",
                process_id = ?self.id,
                start_view = ?start_view,
            );
            return false;
        }
        
        // Only accept start-view messages with 1-QCs (for ordering)
        if start_view.data.qc.data.z != 1 {
            return false;
        }
        
        // Record the QC from the start-view message
        self.record_qc(Arc::new(start_view.data.qc.clone()));
        
        // Store the start-view message for the leader to use when creating a leader block
        self.start_views
            .entry(start_view.data.view)
            .or_insert(Vec::new())
            .push(start_view);
            
        true
    }

    /// Implements the "Complain" section from Algorithm 1
    ///
    /// Checks timeouts and sends complaints:
    /// - After 6Δ, send any unfinalized maximal QCs to the leader
    /// - After 12Δ, send an end-view message to all processes
    pub fn check_timeouts(&mut self, to_send: &mut Vec<(Message, Option<Identity>)>) {
        let time_in_view = self.current_time - self.view_entry_time;

        // First timeout - 6Δ, complain to the leader
        if time_in_view >= self.delta * COMPLAIN_TIMEOUT {
            // Find the maximal unfinalized QC according to the observes relation
            let maximal_unfinalized = self
                .index
                .unfinalized
                .iter()
                .flat_map(|(_, qcs)| qcs)
                .max_by(|&qc1, &qc2| {
                    // Compare QCs using the observes relation
                    if self.observes(qc1.clone(), qc2) {
                        Ordering::Greater
                    } else if self.observes(qc2.clone(), qc1) {
                        Ordering::Less
                    } else {
                        Ordering::Equal
                    }
                });

            // If we found a maximal unfinalized QC and haven't already complained about it,
            // send it to the leader
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