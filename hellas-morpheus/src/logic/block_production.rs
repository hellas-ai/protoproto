use crate::logic::*;
use crate::*;
use std::sync::Arc;

/// Check if we should produce blocks
pub(crate) fn check_produce_blocks_effects<Tr: Transaction>(
    state: &ProcessState<Tr>,
    kb: &KeyBook,
    id: &Identity,
    n: u32,
    f: u32,
) -> Vec<Effect<Tr>> {
    let mut effects = Vec::new();

    // Check transaction block production
    if can_produce_tr_block(state) {
        if let Ok((_block, block_effects)) = produce_tr_block(state, kb, id) {
            effects.extend(block_effects);
        }
    }

    // Check leader block production
    if *id == leader(state.current_view, n)
        && can_produce_lead_block(state, id, n, f)
        && state.current_phase == Phase::High
        && state.tips.len() > 1
    {
        if let Ok((_block, block_effects)) = produce_lead_block(state, kb, id, n, f) {
            effects.extend(block_effects);
        }
    }

    effects
}
fn can_produce_tr_block<Tr: Transaction>(state: &ProcessState<Tr>) -> bool {
    let has_transactions = !state.ready_transactions.is_empty();
    let slot = state.slot_tr;

    if !slot.is_zero() {
        // Check if we have the previous slot QC for slot s-1
        let has_prev_slot_qc = state
            .latest_tr_qc
            .as_ref()
            .map(|qc| qc.data.for_which.slot.is_pred(slot))
            .unwrap_or(false);

        has_prev_slot_qc && has_transactions
    } else {
        has_transactions
    }
}

fn can_produce_lead_block<Tr: Transaction>(
    state: &ProcessState<Tr>,
    _id: &Identity,
    n: u32,
    f: u32,
) -> bool {
    let view = state.current_view;
    let slot = state.slot_lead;
    let has_produced = state
        .produced_lead_in_view
        .get(&view)
        .copied()
        .unwrap_or(false);

    if has_produced {
        // Check for previous 1-QC for slot s-1
        state
            .latest_leader_1qc
            .as_ref()
            .map(|qc| qc.data.for_which.slot.is_pred(slot))
            .unwrap_or(false)
    } else {
        // In view 0, don't require start view messages since it's the initial view
        if view == ViewNum(0) {
            true
        } else {
            has_enough_start_views(state, view, n, f)
        }
    }
}

fn has_enough_start_views<Tr: Transaction>(
    state: &ProcessState<Tr>,
    view: ViewNum,
    n: u32,
    f: u32,
) -> bool {
    // Need n-f start view messages
    state
        .start_views
        .get(&view)
        .map(|msgs| msgs.len() >= (n - f) as usize)
        .unwrap_or(false)
}

fn produce_tr_block<Tr: Transaction>(
    state: &ProcessState<Tr>,
    kb: &KeyBook,
    id: &Identity,
) -> ProcessingResult<(Arc<Signed<Block<Tr>>>, Vec<Effect<Tr>>)> {
    let slot = state.slot_tr;
    let view = state.current_view;

    // Determine previous QCs
    let mut prev_qcs = Vec::new();
    if !slot.is_zero() {
        if let Some(tr_qc) = &state.latest_tr_qc {
            if tr_qc.data.for_which.slot.is_pred(slot) {
                prev_qcs.push(tr_qc.clone());
            } else {
                return Err(ProcessingError::StateInconsistency {
                    reason: "Missing previous slot QC for transaction block".to_string(),
                });
            }
        } else {
            return Err(ProcessingError::StateInconsistency {
                reason: "No previous transaction QC found".to_string(),
            });
        }
    } else {
        prev_qcs.push(state.genesis_qc.clone());
    }

    // Add single tip if exists
    if state.tips.len() == 1 {
        let tip = &state.tips[0];
        if !prev_qcs
            .iter()
            .any(|qc| qc.data.for_which == tip.data.for_which)
        {
            prev_qcs.push(tip.clone());
        }
    }

    let height = prev_qcs
        .iter()
        .map(|qc| qc.data.for_which.height)
        .max()
        .unwrap_or(0)
        + 1;

    let block_key = BlockKey {
        type_: BlockType::Tr,
        view,
        height,
        author: Some(id.clone()),
        slot,
        hash: Some(BlockHash(id.0 as u64 * 0x100 + slot.0)),
    };

    let block = Block {
        key: block_key.clone(),
        prev: prev_qcs,
        one: state.max_1qc.clone(),
        data: BlockData::Tr {
            transactions: state.ready_transactions.clone(),
        },
    };

    let signed_block = Arc::new(Signed::from_data(block, kb));

    let vote = make_vote(kb, 0, &block_key);

    let effects = vec![
        Effect::BlockProduced {
            block: signed_block.clone(),
        },
        Effect::TransactionsUpdated {
            transactions: vec![],
        },
        Effect::SlotAdvanced {
            slot_type: BlockType::Tr,
            new_slot: SlotNum(slot.0 + 1),
        },
        Effect::VoteRecorded {
            voter: id.clone(),
            vote: vote.clone(),
        },
    ];

    Ok((signed_block, effects))
}

fn produce_lead_block<Tr: Transaction>(
    state: &ProcessState<Tr>,
    kb: &KeyBook,
    id: &Identity,
    n: u32,
    f: u32,
) -> ProcessingResult<(Arc<Signed<Block<Tr>>>, Vec<Effect<Tr>>)> {
    let slot = state.slot_lead;
    let view = state.current_view;

    // Get tips as previous QCs
    let mut prev_qcs: Vec<FinishedQC> = state.tips.clone();

    // Add previous leader block QC if needed
    if !slot.is_zero() {
        if let Some(prev_qc) = &state.latest_leader_qc {
            if prev_qc.data.for_which.slot.is_pred(slot)
                && !prev_qcs
                    .iter()
                    .any(|qc| qc.data.for_which == prev_qc.data.for_which)
            {
                prev_qcs.push(prev_qc.clone());
            } else {
                return Err(ProcessingError::StateInconsistency {
                    reason: "Missing or invalid previous leader QC".to_string(),
                });
            }
        } else {
            return Err(ProcessingError::StateInconsistency {
                reason: "No previous leader QC found".to_string(),
            });
        }
    }

    let height = prev_qcs
        .iter()
        .map(|qc| qc.data.for_which.height)
        .max()
        .unwrap_or(0)
        + 1;

    let has_produced = has_produced_lead_in_view(state, view);

    let (one_qc, justification) = if !has_produced {
        let view_messages = state.start_views.get(&view).cloned().unwrap_or_default();

        if view_messages.len() < (n - f) as usize {
            return Err(ProcessingError::StateInconsistency {
                reason: format!(
                    "Not enough start view messages: {} < {}",
                    view_messages.len(),
                    n - f
                ),
            });
        }

        let max_qc = view_messages
            .iter()
            .map(|msg| &msg.data.qc)
            .max_by(|a, b| a.data.compare_qc(&b.data))
            .cloned()
            .unwrap_or_else(|| state.max_1qc.clone());

        let final_qc = if max_qc.data.compare_qc(&state.max_1qc.data) == std::cmp::Ordering::Less {
            state.max_1qc.clone()
        } else {
            max_qc
        };

        (final_qc, view_messages)
    } else {
        let prev_qc = state
            .latest_leader_1qc
            .clone()
            .unwrap_or_else(|| state.max_1qc.clone());
        (prev_qc, vec![])
    };

    let block_key = BlockKey {
        type_: BlockType::Lead,
        view,
        height,
        author: Some(id.clone()),
        slot,
        hash: Some(BlockHash(slot.0)),
    };

    let block = Block {
        key: block_key.clone(),
        prev: prev_qcs,
        one: one_qc,
        data: BlockData::Lead { justification },
    };

    let signed_block = Arc::new(Signed::from_data(block, kb));

    let effects = vec![
        Effect::BlockProduced {
            block: signed_block.clone(),
        },
        Effect::SlotAdvanced {
            slot_type: BlockType::Lead,
            new_slot: SlotNum(slot.0 + 1),
        },
        Effect::LeaderBlockProducedInView { view },
    ];

    Ok((signed_block, effects))
}

fn has_produced_lead_in_view<Tr: Transaction>(state: &ProcessState<Tr>, view: ViewNum) -> bool {
    state
        .produced_lead_in_view
        .get(&view)
        .copied()
        .unwrap_or(false)
}
