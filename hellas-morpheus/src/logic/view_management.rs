use crate::logic::*;
use crate::*;
use std::sync::Arc;

const COMPLAIN_TIMEOUT: u128 = 6;
const END_VIEW_TIMEOUT: u128 = 12;

/// Process end-view vote and produce effects
pub(crate) fn process_end_view<Tr: Transaction>(
    sender: &Identity,
    end_view: &Arc<ThreshPartial<ViewNum>>,
    kb: &KeyBook,
    _n: u32,
    _f: u32,
) -> ProcessingResult<Vec<Effect<Tr>>> {
    let mut effects = Vec::new();

    // Validate signature
    if !end_view.valid_signature(kb) {
        return Err(ProcessingError::InvalidSignature {
            message_type: "end-view",
        });
    }

    // Record the end-view vote
    effects.push(Effect::EndViewRecorded {
        voter: sender.clone(),
        view: end_view.data,
        vote: end_view.clone(),
    });

    Ok(effects)
}

/// Process end-view certificate and trigger view change
pub(crate) fn process_end_view_cert<Tr: Transaction>(
    state: &ProcessState<Tr>,
    cert: &Arc<ThreshSigned<ViewNum>>,
    kb: &KeyBook,
    id: &Identity,
    n: u32,
    f: u32,
) -> ProcessingResult<Vec<Effect<Tr>>> {
    let mut effects = Vec::new();

    // Validate certificate
    if !cert.valid_signature(kb, f + 1) {
        return Err(ProcessingError::InvalidSignature {
            message_type: "end-view certificate",
        });
    }

    let new_view = cert.data.incr();

    // Only process if this would advance our view
    if new_view > state.current_view {
        effects.extend(trigger_view_change(state, new_view, id, n, kb)?);
        
        // Broadcast the certificate to ensure all nodes enter the new view
        effects.push(Effect::MessageSent {
            message: Message::EndViewCert(cert.clone()),
            target: None,
        });
    }

    Ok(effects)
}

/// Process start view message from another node
pub(crate) fn process_start_view<Tr: Transaction>(
    state: &ProcessState<Tr>,
    sender: &Identity,
    start_view: &Arc<Signed<StartView>>,
    kb: &KeyBook,
    id: &Identity,
    n: u32,
) -> ProcessingResult<Vec<Effect<Tr>>> {
    let mut effects = Vec::new();

    // Validate signature
    if !start_view.valid_signature(kb) {
        return Err(ProcessingError::InvalidSignature {
            message_type: "start-view",
        });
    }

    // Check that the QC is a 1-QC (as in original implementation)
    if start_view.data.qc.data.z != 1 {
        return Ok(effects); // Not an error, just ignore non-1-QCs
    }

    // Only the leader should receive start view messages
    if leader(start_view.data.view, n) != *id {
        return Ok(effects);
    }

    // Record the start view message
    effects.push(Effect::StartViewRecorded {
        sender: sender.clone(),
        start_view: start_view.clone(),
    });

    Ok(effects)
}

/// Check for timeouts and produce appropriate effects
pub(crate) fn check_timeouts_effects<Tr: Transaction>(
    state: &ProcessState<Tr>,
    kb: &KeyBook,
    id: &Identity,
    n: u32,
    f: u32,
    delta: u128,
) -> Vec<Effect<Tr>> {
    let mut effects = Vec::new();
    let time_in_view = state.current_time.saturating_sub(state.view_entry_time);

    // First timeout - complain to leader about unfinalized QCs
    if time_in_view >= delta * COMPLAIN_TIMEOUT {
        if let Some(qc) = find_maximal_unfinalized(state) {
            if !state.complained_qcs.contains(qc) {
                effects.push(Effect::ComplaintSent {
                    qc: qc.clone(),
                    target: leader(state.current_view, n),
                });
            }
        }
    }

    // Second timeout - send end-view message
    if time_in_view >= delta * END_VIEW_TIMEOUT && has_unfinalized(state) {
        // Create end-view vote
        let vote = Arc::new(ThreshPartial::from_data(state.current_view, kb));
        effects.push(Effect::MessageSent {
            message: Message::EndView(vote.clone()),
            target: None,
        });
        effects.push(Effect::EndViewRecorded {
            voter: id.clone(),
            view: state.current_view,
            vote: vote.clone(),
        });
    }

    effects
}

/// Trigger a view change to a new view
pub(crate) fn trigger_view_change<Tr: Transaction>(
    state: &ProcessState<Tr>,
    new_view: ViewNum,
    id: &Identity,
    n: u32,
    kb: &KeyBook,
) -> ProcessingResult<Vec<Effect<Tr>>> {
    let mut effects = Vec::new();

    if new_view <= state.current_view {
        return Ok(effects);
    }

    // Record the view change
    effects.push(Effect::ViewChanged {
        old_view: state.current_view,
        new_view,
    });

    // Collect tips that we authored
    let my_tips: Vec<FinishedQC> = state
        .tips
        .iter()
        .filter(|tip| tip.data.for_which.author == Some(id.clone()))
        .cloned()
        .collect();

    // Send start view message to new leader
    let new_leader = leader(new_view, n);
    effects.push(Effect::StartViewSent {
        view: new_view,
        qc: state.max_1qc.clone(),
        tips: my_tips,
        target: new_leader,
    });

    // Actually send the tips as QC messages
    for tip in &state.tips {
        if tip.data.for_which.author == Some(id.clone()) {
            effects.push(Effect::MessageSent {
                message: Message::QC(tip.clone()),
                target: Some(leader(new_view, n)),
            });
        }
    }

    // Send the start view message
    let start_view = Arc::new(Signed::from_data(
        StartView {
            view: new_view,
            qc: state.max_1qc.clone(),
        },
        kb,
    ));
    
    effects.push(Effect::MessageSent {
        message: Message::StartView(start_view),
        target: Some(new_leader),
    });

    Ok(effects)
}

/// Check if we have enough end-view votes to form a certificate
pub(crate) fn check_view_cert_formation<Tr: Transaction>(
    state: &ProcessState<Tr>,
    view: ViewNum,
    kb: &KeyBook,
    n: u32,
    f: u32,
) -> ProcessingResult<Option<Arc<ThreshSigned<ViewNum>>>> {
    if let Some(votes) = state.end_views.get(&view) {
        if votes.len() >= (f + 1) as usize {
            // Form the certificate
            let vote_data = view;
            let votes_vec: Vec<Arc<ThreshPartial<ViewNum>>> = votes.values().cloned().collect();
            
            // Collect partial signatures
            let mut vote_sigs: Vec<(usize, hints::PartialSignature)> = Vec::new();
            for vote in &votes_vec {
                if vote.data == vote_data {
                    let author_index = vote.author.0.saturating_sub(1) as usize;
                    vote_sigs.push((author_index, vote.signature.clone()));
                }
            }

            if vote_sigs.len() >= (f + 1) as usize {
                // Sort and aggregate
                vote_sigs.sort_by_key(|(idx, _)| *idx);
                
                let agg = kb
                    .hints_setup
                    .as_ref()
                    .ok_or_else(|| ProcessingError::QcFormationError {
                        reason: "hints setup not initialized".to_string(),
                    })?
                    .aggregator();

                let mut data = Vec::new();
                vote_data
                    .serialize_compressed(&mut data)
                    .map_err(|e| ProcessingError::QcFormationError {
                        reason: format!("Failed to serialize view number: {}", e),
                    })?;

                    #[cfg(not(test))]
                let signature = hints::sign_aggregate(
                    &agg,
                    hints::F::from((f + 1) as u64),
                    &vote_sigs,
                    &data,
                )
                .map_err(|e| ProcessingError::QcFormationError {
                    reason: format!("Failed to aggregate signatures: {:?}", e),
                })?;
                #[cfg(test)]
                let signature = hints::Signature::default();

                return Ok(Some(Arc::new(ThreshSigned {
                    data: vote_data,
                    signature,
                })));
            }
        }
    }
    
    Ok(None)
} 