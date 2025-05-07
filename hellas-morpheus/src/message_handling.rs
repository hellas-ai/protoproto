use std::sync::Arc;

use ark_serialize::CanonicalSerialize;

use crate::{format::format_message, *};

impl MorpheusProcess {
    pub(crate) fn send_msg(
        &mut self,
        to_send: &mut Vec<(Message, Option<Identity>)>,
        message: (Message, Option<Identity>),
    ) {
        if message.1.is_none() || message.1.as_ref().unwrap() == &self.id {
            // IMPORTANT: implements note from page 8:
            // In what follows, we suppose that, when a correct process sends a
            // message to ‘all processes’, it regards that message as
            // immediately received by itself
            self.process_message(message.0.clone(), self.id.clone(), to_send);
        }
        to_send.push(message);
    }

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

        match message {
            Message::Block(block) => {
                if let Err(error) = self.block_valid(&block) {
                    tracing::error!(
                        target: "invalid_block",
                        process_id = ?self.id,
                        block_key = ?block.data.key,
                        error = ?error,
                    );
                    return false;
                }
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
                self.record_block(&block);
            }
            Message::NewVote(vote_data) => {
                if !vote_data.valid_signature(&self.kb) {
                    tracing::error!(
                        target: "invalid_vote",
                        process_id = ?self.id,
                        vote_data = ?vote_data,
                    );
                    return false;
                }
                self.record_vote(&vote_data, to_send);
            }
            Message::QC(qc) => {
                if !qc.valid_signature(&self.kb, self.n - self.f) {
                    tracing::error!(
                        target: "invalid_qc",
                        process_id = ?self.id,
                        qc = ?qc,
                    );
                    return false;
                }
                self.record_qc(qc);
                if self.index.max_view.0 > self.view_i {
                    self.end_view(
                        Message::QC(self.index.qcs.get(&self.index.max_view.1).cloned().unwrap()),
                        self.index.max_view.0,
                        to_send,
                    );
                }
            }
            Message::EndView(end_view) => {
                if !end_view.valid_signature(&self.kb) {
                    tracing::error!(
                        target: "invalid_end_view",
                        process_id = ?self.id,
                        end_view = ?end_view,
                    );
                    return false;
                }
                match self.end_views.record_vote(end_view.clone()) {
                    Ok(num_votes) => {
                        if end_view.data >= self.view_i && num_votes >= self.f as usize + 1 {
                            let votes_now = self
                                .end_views
                                .votes
                                .get(&end_view.data)
                                .unwrap()
                                .values()
                                .map(|v| (v.author.0 as usize - 1, v.signature.clone()))
                                .collect::<Vec<_>>();
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
                    }
                    Err(Duplicate) => return false,
                }
            }
            Message::EndViewCert(end_view_cert) => {
                if !end_view_cert.valid_signature(&self.kb, self.f + 1) {
                    tracing::error!(
                        target: "invalid_end_view_cert",
                        process_id = ?self.id,
                        end_view_cert = ?end_view_cert,
                    );
                    return false;
                }
                let view = end_view_cert.data.incr();
                if view >= self.view_i {
                    self.end_view(Message::EndViewCert(end_view_cert), view, to_send);
                }
            }
            Message::StartView(start_view) => {
                if !start_view.valid_signature(&self.kb) {
                    tracing::error!(
                        target: "invalid_start_view",
                        process_id = ?self.id,
                        start_view = ?start_view,
                    );
                    return false;
                }
                if start_view.data.qc.data.z != 1 {
                    return false;
                }
                self.record_qc(Arc::new(start_view.data.qc.clone()));
                self.start_views
                    .entry(start_view.data.view)
                    .or_insert(Vec::new())
                    .push(start_view);
            }
        }

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

}