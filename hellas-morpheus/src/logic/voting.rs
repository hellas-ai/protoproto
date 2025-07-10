use crate::logic::*;
use crate::*;
use std::sync::Arc;

/// Process a vote and produce effects
pub(crate) fn process_vote<Tr: Transaction>(
    state: &ProcessState<Tr>,
    sender: &Identity,
    vote: &Arc<ThreshPartial<VoteData>>,
    kb: &KeyBook,
) -> ProcessingResult<Vec<Effect<Tr>>> {
    let mut effects = Vec::new();

    // Validate signature
    if !vote.valid_signature(kb) {
        return Err(ProcessingError::InvalidSignature {
            message_type: "vote",
        });
    }

    // Check if this is a duplicate vote from the same sender
    if has_vote_from(state, sender, &vote.data) {
        return Ok(effects); // Not an error, just ignore duplicate
    }

    // Record the vote
    effects.push(Effect::VoteRecorded {
        voter: sender.clone(),
        vote: vote.clone(),
    });

    Ok(effects)
}

pub(crate) fn make_vote(
    kb: &KeyBook,
    vote_type: u8,
    block_key: &BlockKey,
) -> Arc<ThreshPartial<VoteData>> {
    Arc::new(ThreshPartial::from_data(
        VoteData {
            z: vote_type,
            for_which: block_key.clone(),
        },
        kb,
    ))
}

/// Check pending votes and produce voting effects
pub(crate) fn check_pending_votes<Tr: Transaction>(
    state: &ProcessState<Tr>,
    kb: &KeyBook,
    _id: &Identity,
    _n: u32,
) -> ProcessingResult<Vec<Effect<Tr>>> {
    let mut effects = Vec::new();
    let current_view = state.current_view;
    let mut current_phase = state.current_phase;

    // Track votes sent in this pass to avoid duplicates
    let mut votes_sent_this_pass = std::collections::HashSet::new();

    // Determine if we can vote for transaction blocks
    let contains_lead = state
        .contains_lead_by_view
        .get(&current_view)
        .copied()
        .unwrap_or(false);

    let finalized_lead_exists = state.blocks.keys().any(|key| {
        key.type_ == BlockType::Lead && key.view == current_view && state.finalized.contains(key)
    });

    let unfinalized_lead_exists = state
        .unfinalized_lead_by_view
        .get(&current_view)
        .map(|blocks| !blocks.is_empty())
        .unwrap_or(false);

    // Per the paper: can vote for TR blocks if:
    // 1. No leader blocks seen at all (quiet leader case), OR
    // 2. Leader blocks exist, at least one is finalized, and none are unfinalized
    let can_vote_for_tr_blocks =
        !contains_lead || (finalized_lead_exists && !unfinalized_lead_exists);

    if can_vote_for_tr_blocks {
        // Check for eligible 1-votes on transaction blocks
        for (block_key, _block) in get_pending_tr_blocks(state, current_view) {
            if is_eligible_for_tr_1_vote(state, &block_key)
                && !has_voted(state, 1, &block_key)
                && !votes_sent_this_pass.contains(&(1, block_key.clone()))
            {
                effects.push(Effect::VoteSent {
                    vote: make_vote(kb, 1, &block_key),
                    target: None,
                });
                votes_sent_this_pass.insert((1, block_key.clone()));

                // Mark pending vote as processed
                effects.push(Effect::PendingVoteProcessed {
                    view: current_view,
                    vote_type: 1,
                    block_type: BlockType::Tr,
                    block_key: block_key.clone(),
                });

                // Enter low phase when voting for transaction blocks
                if current_phase != Phase::Low {
                    effects.push(Effect::PhaseChanged {
                        view: current_view,
                        old_phase: current_phase,
                        new_phase: Phase::Low,
                    });
                    current_phase = Phase::Low; // Update local tracking
                }
            }
        }

        // Check for eligible 2-votes on transaction blocks
        for qc in get_pending_tr_1qcs(state, current_view) {
            if is_eligible_for_tr_2_vote(state, &qc)
                && !has_voted(state, 2, &qc.data.for_which)
                && !votes_sent_this_pass.contains(&(2, qc.data.for_which.clone()))
            {
                effects.push(Effect::VoteSent {
                    vote: make_vote(kb, 2, &qc.data.for_which),
                    target: None,
                });
                votes_sent_this_pass.insert((2, qc.data.for_which.clone()));

                // Mark pending vote as processed
                effects.push(Effect::PendingVoteProcessed {
                    view: current_view,
                    vote_type: 2,
                    block_type: BlockType::Tr,
                    block_key: qc.data.for_which.clone(),
                });

                // Ensure we're in low phase
                if current_phase != Phase::Low {
                    effects.push(Effect::PhaseChanged {
                        view: current_view,
                        old_phase: current_phase,
                        new_phase: Phase::Low,
                    });
                    current_phase = Phase::Low;
                }
            }
        }
    }

    // Check leader block votes only if still in high phase
    if current_phase == Phase::High {
        // Check for eligible 1-votes on leader blocks
        for (block_key, _block) in get_pending_lead_blocks(state, current_view) {
            if block_key.view == current_view
                && !has_voted(state, 1, &block_key)
                && !votes_sent_this_pass.contains(&(1, block_key.clone()))
            {
                effects.push(Effect::VoteSent {
                    vote: make_vote(kb, 1, &block_key),
                    target: None,
                });
                votes_sent_this_pass.insert((1, block_key.clone()));

                // Mark pending vote as processed
                effects.push(Effect::PendingVoteProcessed {
                    view: current_view,
                    vote_type: 1,
                    block_type: BlockType::Lead,
                    block_key: block_key.clone(),
                });
            }
        }

        // Check for eligible 2-votes on leader blocks
        for qc in get_pending_lead_1qcs(state, current_view) {
            if qc.data.for_which.view == current_view
                && !has_voted(state, 2, &qc.data.for_which)
                && !votes_sent_this_pass.contains(&(2, qc.data.for_which.clone()))
            {
                effects.push(Effect::VoteSent {
                    vote: make_vote(kb, 2, &qc.data.for_which),
                    target: None,
                });
                votes_sent_this_pass.insert((2, qc.data.for_which.clone()));

                // Mark pending vote as processed
                effects.push(Effect::PendingVoteProcessed {
                    view: current_view,
                    vote_type: 2,
                    block_type: BlockType::Lead,
                    block_key: qc.data.for_which.clone(),
                });
            }
        }
    }

    Ok(effects)
}

fn has_vote_from<Tr: Transaction>(
    state: &ProcessState<Tr>,
    sender: &Identity,
    vote_data: &VoteData,
) -> bool {
    state
        .vote_tracker
        .get(vote_data)
        .map(|votes| votes.contains_key(sender))
        .unwrap_or(false)
}

fn get_votes_for<Tr: Transaction>(
    state: &ProcessState<Tr>,
    vote_data: &VoteData,
) -> Vec<Arc<ThreshPartial<VoteData>>> {
    state
        .vote_tracker
        .get(vote_data)
        .map(|votes| votes.values().cloned().collect())
        .unwrap_or_default()
}

fn form_qc_from_votes(
    vote_data: &VoteData,
    votes: &[Arc<ThreshPartial<VoteData>>],
    kb: &KeyBook,
    n: u32,
    f: u32,
) -> ProcessingResult<FinishedQC> {
    // Collect partial signatures indexed by author
    let mut vote_sigs: Vec<(usize, hints::PartialSignature)> = Vec::new();

    for vote in votes {
        if vote.data == *vote_data {
            // Convert Identity to index (identities are 1-indexed)
            let author_index = vote.author.0.saturating_sub(1) as usize;
            vote_sigs.push((author_index, vote.signature.clone()));
        }
    }

    // Make sure we have enough votes
    if vote_sigs.len() < (n - f) as usize {
        return Err(ProcessingError::QcFormationError {
            reason: format!(
                "Not enough votes to form QC: {} < {}",
                vote_sigs.len(),
                n - f
            ),
        });
    }

    // Sort by index for consistent ordering
    vote_sigs.sort_by_key(|(idx, _)| *idx);

    // Aggregate the signatures
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
            reason: format!("Failed to serialize vote data: {}", e),
        })?;

    #[cfg(not(test))]
    let signature = hints::sign_aggregate(&agg, hints::F::from((n - f) as u64), &vote_sigs, &data)
        .map_err(|e| ProcessingError::QcFormationError {
            reason: format!("Failed to aggregate signatures: {:?}", e),
        })?;

    #[cfg(test)]
    let signature = hints::Signature::default();

    Ok(Arc::new(ThreshSigned {
        data: vote_data.clone(),
        signature,
    }))
}

/// Form a QC from votes in the state
pub(crate) fn form_qc_from_state<Tr: Transaction>(
    state: &ProcessState<Tr>,
    vote_data: &VoteData,
    kb: &KeyBook,
    n: u32,
    f: u32,
) -> ProcessingResult<FinishedQC> {
    let votes = get_votes_for(state, vote_data);
    if votes.len() >= (n - f) as usize {
        form_qc_from_votes(vote_data, &votes, kb, n, f)
    } else {
        Err(ProcessingError::QcFormationError {
            reason: format!("Not enough votes: {} < {}", votes.len(), n - f),
        })
    }
}

fn get_pending_tr_blocks<Tr: Transaction>(
    state: &ProcessState<Tr>,
    view: ViewNum,
) -> Vec<(BlockKey, Arc<Signed<Block<Tr>>>)> {
    let pending_keys = get_unvoted_blocks(state, view, 1, BlockType::Tr);
    let mut result = Vec::new();
    for key in pending_keys {
        if !has_voted(state, 1, &key) {
            if let Some(block) = state.blocks.get(&key) {
                result.push((key, block.clone()));
            }
        }
    }
    result
}

fn get_pending_tr_1qcs<Tr: Transaction>(
    state: &ProcessState<Tr>,
    view: ViewNum,
) -> Vec<FinishedQC> {
    let pending_keys = get_unvoted_blocks(state, view, 2, BlockType::Tr);
    let mut result = Vec::new();
    for qc in &state.qcs {
        if qc.data.z == 1
            && qc.data.for_which.type_ == BlockType::Tr
            && qc.data.for_which.view == view
            && pending_keys.contains(&qc.data.for_which)
            && !has_voted(state, 2, &qc.data.for_which)
        {
            result.push(qc.clone());
        }
    }
    result
}

fn get_pending_lead_blocks<Tr: Transaction>(
    state: &ProcessState<Tr>,
    view: ViewNum,
) -> Vec<(BlockKey, Arc<Signed<Block<Tr>>>)> {
    let pending_keys = get_unvoted_blocks(state, view, 1, BlockType::Lead);
    let mut result = Vec::new();
    for key in pending_keys {
        if !has_voted(state, 1, &key) {
            if let Some(block) = state.blocks.get(&key) {
                result.push((key, block.clone()));
            }
        }
    }
    result
}

fn get_pending_lead_1qcs<Tr: Transaction>(
    state: &ProcessState<Tr>,
    view: ViewNum,
) -> Vec<FinishedQC> {
    let pending_keys = get_unvoted_blocks(state, view, 2, BlockType::Lead);
    let mut result = Vec::new();
    for qc in &state.qcs {
        if qc.data.z == 1
            && qc.data.for_which.type_ == BlockType::Lead
            && qc.data.for_which.view == view
            && pending_keys.contains(&qc.data.for_which)
            && !has_voted(state, 2, &qc.data.for_which)
        {
            result.push(qc.clone());
        }
    }
    result
}

fn get_unvoted_blocks<Tr: Transaction>(
    state: &ProcessState<Tr>,
    view: ViewNum,
    vote_type: u8,
    block_type: BlockType,
) -> Vec<BlockKey> {
    let pending = state.pending_votes.get(&view);
    match (vote_type, block_type) {
        (1, BlockType::Tr) => pending
            .map(|p| p.tr_1.keys().cloned().collect())
            .unwrap_or_default(),
        (2, BlockType::Tr) => pending
            .map(|p| p.tr_2.keys().cloned().collect())
            .unwrap_or_default(),
        (1, BlockType::Lead) => pending
            .map(|p| p.lead_1.keys().cloned().collect())
            .unwrap_or_default(),
        (2, BlockType::Lead) => pending
            .map(|p| p.lead_2.keys().cloned().collect())
            .unwrap_or_default(),
        _ => vec![],
    }
}

fn has_voted<Tr: Transaction>(
    state: &ProcessState<Tr>,
    vote_type: u8,
    block_key: &BlockKey,
) -> bool {
    if let Some(author) = &block_key.author {
        state
            .voted
            .contains(&(vote_type, block_key.type_, block_key.slot, author.clone()))
    } else {
        false
    }
}

fn is_eligible_for_tr_1_vote<Tr: Transaction>(
    state: &ProcessState<Tr>,
    block_key: &BlockKey,
) -> bool {
    // According to the paper, we can vote if:
    // 1. The block is a "single tip of M_i".
    // 2. The block's one-QC is >= our max_1qc.
    if !state.blocks.contains_key(block_key) {
        return false;
    }

    let block = &state.blocks[block_key];

    // Check if block's one-QC is >= max_1qc
    if block.data.one.data.compare_qc(&state.max_1qc.data) == std::cmp::Ordering::Less {
        return false;
    }

    // Check if this block is a single tip
    is_block_single_tip(state, block_key)
}

fn is_eligible_for_tr_2_vote<Tr: Transaction>(state: &ProcessState<Tr>, qc: &FinishedQC) -> bool {
    // According to the paper (lines 43-45), we can vote if:
    // 1. The 1-QC is a "single tip of Q_i".
    // 2. No blocks exist with height greater than this block

    // A 1-QC is a single tip if the set of tips contains only this QC.
    let is_single_tip = state.tips.len() == 1 && &state.tips[0] == qc;

    // Check if no blocks have greater height
    let no_higher_blocks = state.max_height.0 <= qc.data.for_which.height;

    is_single_tip && no_higher_blocks
}

/// Check if a block is a single tip of M_i according to the paper's definition.
/// Definition: b is a single tip of M_i if there exists q which is a single tip of Q_i,
/// and b is the unique block in M_i pointing to q.b.
fn is_block_single_tip<Tr: Transaction>(state: &ProcessState<Tr>, block_key: &BlockKey) -> bool {
    if state.tips.len() != 1 {
        return false;
    }
    if let Some(tip) = state.tips.get(0) {
        state
            .block_pointed_by
            .get(&tip.data.for_which)
            .map_or(false, |parents| {
                parents.len() == 1 && parents.contains(block_key)
            })
    } else {
        false
    }
}
