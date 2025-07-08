//! Pure functional logic for processing actions
//!
//! This module contains all the pure logic functions that operate on
//! immutable ProcessState and return Effects without side effects.

use crate::state::ProcessState;
use crate::*;
use std::sync::Arc;

/// Main entry point for processing actions
/// This is a pure function that takes state and an action and returns effects
pub fn process_action<Tr: Transaction>(
    state: &ProcessState<Tr>,
    action: &Action<Tr>,
    kb: &KeyBook,
    id: &Identity,
    n: u32,
    f: u32,
    delta: u128,
) -> Vec<Effect<Tr>> {
    match action {
        Action::ProcessMessage { sender, payload } => {
            process_message_internal(state, sender, payload, kb, id, n, f)
        }
        Action::SetTime(time) => {
            vec![Effect::TimeUpdated(*time)]
        }
        Action::SetReadyTransactions(transactions) => {
            vec![Effect::TransactionsUpdated {
                transactions: transactions.clone(),
            }]
        }
        Action::CheckTimeouts => check_timeouts_effects(state, id, n, f, delta, kb),
        Action::CheckProduceBlocks => check_produce_blocks_effects(state, kb, id, n, f),
    }
}

/// Process an incoming message and produce effects
fn process_message_internal<Tr: Transaction>(
    state: &ProcessState<Tr>,
    sender: &Identity,
    message: &Message<Tr>,
    kb: &KeyBook,
    id: &Identity,
    n: u32,
    f: u32,
) -> Vec<Effect<Tr>> {
    let mut effects = Vec::new();

    match message {
        Message::Block(block) => {
            effects.extend(process_block(state, block, kb, n, f));
        }
        Message::NewVote(vote) => {
            effects.extend(process_vote(state, sender, vote, kb, id, n, f));
        }
        Message::QC(qc) => {
            effects.extend(process_qc(state, qc, kb, id, n, f));
        }
        Message::EndView(end_view) => {
            effects.extend(process_end_view(state, sender, end_view, kb, n, f));
        }
        Message::EndViewCert(cert) => {
            effects.extend(process_end_view_cert(state, cert, kb, id, n, f));
        }
        Message::StartView(start_view) => {
            effects.extend(process_start_view(state, sender, start_view, kb, id, n));
        }
    }

    // After processing any message, check if we can vote on pending blocks
    effects.extend(check_pending_votes(state, id, n));

    effects
}

/// Process a block and produce effects
fn process_block<Tr: Transaction>(
    state: &ProcessState<Tr>,
    block: &Arc<Signed<Block<Tr>>>,
    kb: &KeyBook,
    n: u32,
    f: u32,
) -> Vec<Effect<Tr>> {
    let mut effects = Vec::new();

    // Validate the block first
    if validate_block(state, block, kb, n, f).is_err() {
        return effects; // Invalid block, no effects
    }

    // Record the block
    effects.push(Effect::BlockRecorded {
        block: block.clone(),
    });

    // If it's a transaction block, send 0-vote
    if block.data.key.type_ == BlockType::Tr {
        effects.push(Effect::VoteSent {
            vote_type: 0,
            block_key: block.data.key.clone(),
            target: Some(block.data.key.author.clone().unwrap()),
        });
    }

    // Record any QCs in the block
    for qc in &block.data.prev {
        effects.extend(process_qc(state, qc, kb, &Identity(0), n, f));
    }
    effects.extend(process_qc(state, &block.data.one, kb, &Identity(0), n, f));

    effects
}

/// Process a vote and produce effects
fn process_vote<Tr: Transaction>(
    state: &ProcessState<Tr>,
    sender: &Identity,
    vote: &Arc<ThreshPartial<VoteData>>,
    kb: &KeyBook,
    _id: &Identity,
    n: u32,
    f: u32,
) -> Vec<Effect<Tr>> {
    let mut effects = Vec::new();

    // Validate signature
    if !vote.valid_signature(kb) {
        return effects;
    }

    // Check if this is a duplicate vote from the same sender
    if has_vote_from(state, sender, &vote.data) {
        return effects;
    }

    // Record the vote
    effects.push(Effect::VoteRecorded {
        voter: sender.clone(),
        vote_data: vote.data.clone(),
    });

    // Check if we have a quorum
    let vote_count = count_votes(state, &vote.data) + 1;
    if vote_count >= (n - f) as usize {
        // Get all votes for QC formation
        let votes = get_votes_for(state, &vote.data);
        let mut all_votes = votes.clone();
        all_votes.push(vote.clone());

        // Form QC
        let qc = form_qc_from_votes(&vote.data, &all_votes, kb, n, f);
        effects.push(Effect::QuorumReached {
            vote_data: vote.data.clone(),
            qc_formed: qc.clone(),
        });

        // If it's a 0-QC for our block, broadcast it
        if vote.data.z == 0 && vote.data.for_which.author == Some(sender.clone()) {
            effects.push(Effect::MessageSent {
                message: Message::QC(qc),
                target: None,
            });
        }
    }

    effects
}

/// Process a QC and produce effects
fn process_qc<Tr: Transaction>(
    state: &ProcessState<Tr>,
    qc: &FinishedQC,
    kb: &KeyBook,
    id: &Identity,
    n: u32,
    f: u32,
) -> Vec<Effect<Tr>> {
    let mut effects = Vec::new();

    // Validate signature
    if !qc.valid_signature(kb, n - f) {
        return effects;
    }

    // Check if we already have this QC
    if state.qcs.contains(qc) {
        return effects;
    }

    // Find blocks finalized by this QC
    let finalized_blocks = find_finalized_blocks(state, qc);

    // Record the QC
    effects.push(Effect::QcRecorded {
        qc: qc.clone(),
        finalized_blocks,
    });

    // Check if this triggers a view change
    if qc.data.for_which.view > state.current_view {
        effects.push(Effect::ViewChanged {
            old_view: state.current_view,
            new_view: qc.data.for_which.view,
            cause: format!("QC for view {}", qc.data.for_which.view.0),
        });

        // Send StartView to new leader
        effects.push(Effect::MessageSent {
            message: Message::StartView(Arc::new(Signed::from_data(
                StartView {
                    view: qc.data.for_which.view,
                    qc: state.max_1qc.clone(),
                },
                kb,
            ))),
            target: Some(leader(qc.data.for_which.view, n)),
        });
    }

    effects
}

/// Process an end-view message
fn process_end_view<Tr: Transaction>(
    state: &ProcessState<Tr>,
    _sender: &Identity,
    end_view: &Arc<ThreshPartial<ViewNum>>,
    kb: &KeyBook,
    _n: u32,
    f: u32,
) -> Vec<Effect<Tr>> {
    let mut effects = Vec::new();

    // Validate signature
    if !end_view.valid_signature(kb) {
        return effects;
    }

    // Check if we have enough end-view messages
    let count = count_end_views(state, &end_view.data) + 1;
    if count >= (f + 1) as usize && end_view.data >= state.current_view {
        // Form view certificate
        effects.push(Effect::ViewCertificateFormed {
            view: end_view.data.incr(),
        });

        let cert = form_view_certificate(state, &end_view.data, kb, f);
        effects.push(Effect::MessageSent {
            message: Message::EndViewCert(cert),
            target: None,
        });
    }

    effects
}

/// Process an end-view certificate
fn process_end_view_cert<Tr: Transaction>(
    state: &ProcessState<Tr>,
    cert: &Arc<ThreshSigned<ViewNum>>,
    kb: &KeyBook,
    _id: &Identity,
    n: u32,
    f: u32,
) -> Vec<Effect<Tr>> {
    let mut effects = Vec::new();

    // Validate signature
    if !cert.valid_signature(kb, f + 1) {
        return effects;
    }

    let new_view = cert.data.incr();
    if new_view > state.current_view {
        effects.push(Effect::ViewChanged {
            old_view: state.current_view,
            new_view,
            cause: format!("End-view certificate for view {}", cert.data.0),
        });

        // Send StartView to new leader
        effects.push(Effect::MessageSent {
            message: Message::StartView(Arc::new(Signed::from_data(
                StartView {
                    view: new_view,
                    qc: state.max_1qc.clone(),
                },
                kb,
            ))),
            target: Some(leader(new_view, n)),
        });
    }

    effects
}

/// Process a start-view message
fn process_start_view<Tr: Transaction>(
    _state: &ProcessState<Tr>,
    _sender: &Identity,
    start_view: &Arc<Signed<StartView>>,
    kb: &KeyBook,
    id: &Identity,
    n: u32,
) -> Vec<Effect<Tr>> {
    let effects = Vec::new();

    // Validate signature
    if !start_view.valid_signature(kb) {
        return effects;
    }

    // Only process if we're the leader
    if *id != leader(start_view.data.view, n) {
        return effects;
    }

    // The actual storage of start view messages is handled by state mutation
    effects
}

/// Check for timeouts and produce effects
fn check_timeouts_effects<Tr: Transaction>(
    state: &ProcessState<Tr>,
    id: &Identity,
    n: u32,
    _f: u32,
    delta: u128,
    kb: &KeyBook,
) -> Vec<Effect<Tr>> {
    let mut effects = Vec::new();

    let time_in_view = state.current_time.saturating_sub(state.view_entry_time);

    // Complaint timeout
    if time_in_view >= delta * 6 {
        if let Some(qc) = find_maximal_unfinalized(state) {
            if !state.complained_qcs.contains(qc) {
                effects.push(Effect::ComplaintSent {
                    qc: qc.clone(),
                    target: leader(state.current_view, n),
                });
            }
        }
    }

    // End-view timeout
    if time_in_view >= delta * 12 && has_unfinalized(state) {
        effects.push(Effect::EndViewSent {
            view: state.current_view,
        });
        effects.push(Effect::MessageSent {
            message: Message::EndView(Arc::new(ThreshPartial::from_data(
                state.current_view,
                kb,
            ))),
            target: None,
        });
    }

    // Also check pending votes after timeout check
    effects.extend(check_pending_votes(state, id, n));

    effects
}

/// Check if we should produce blocks
fn check_produce_blocks_effects<Tr: Transaction>(
    state: &ProcessState<Tr>,
    kb: &KeyBook,
    id: &Identity,
    n: u32,
    f: u32,
) -> Vec<Effect<Tr>> {
    let mut effects = Vec::new();

    // Check transaction block production
    if can_produce_tr_block(state) {
        let (block, block_effects) = produce_tr_block(state, kb, id);
        effects.extend(block_effects);
        effects.push(Effect::MessageSent {
            message: Message::Block(block),
            target: None,
        });
    }

    // Check leader block production
    if *id == leader(state.current_view, n)
        && can_produce_lead_block(state, id, f)
        && state.current_phase == Phase::High
        && state.tips.len() > 1
    {
        let (block, block_effects) = produce_lead_block(state, kb, id, n);
        effects.extend(block_effects);
        effects.push(Effect::MessageSent {
            message: Message::Block(block),
            target: None,
        });
    }

    effects
}

/// Check pending votes and produce voting effects
fn check_pending_votes<Tr: Transaction>(
    state: &ProcessState<Tr>,
    _id: &Identity,
    _n: u32,
) -> Vec<Effect<Tr>> {
    let mut effects = Vec::new();
    let current_view = state.current_view;
    let current_phase = state.current_phase;

    // Check transaction block votes if conditions are met
    if contains_lead_in_view(state, current_view)
        && has_unfinalized_lead_in_view(state, current_view)
    {
        // Check for eligible 1-votes on transaction blocks
        for (block_key, _block) in get_pending_tr_blocks(state, current_view) {
            if is_eligible_for_tr_1_vote(state, &block_key) {
                effects.push(Effect::VoteSent {
                    vote_type: 1,
                    block_key: block_key.clone(),
                    target: None,
                });
                effects.push(Effect::PhaseChanged {
                    view: current_view,
                    old_phase: Phase::High,
                    new_phase: Phase::Low,
                });
            }
        }

        // Check for eligible 2-votes on transaction blocks
        for qc in get_pending_tr_1qcs(state, current_view) {
            if is_eligible_for_tr_2_vote(state, &qc.data.for_which) {
                effects.push(Effect::VoteSent {
                    vote_type: 2,
                    block_key: qc.data.for_which.clone(),
                    target: None,
                });
                effects.push(Effect::PhaseChanged {
                    view: current_view,
                    old_phase: Phase::High,
                    new_phase: Phase::Low,
                });
            }
        }
    }

    // Check leader block votes if still in high phase
    if current_phase == Phase::High {
        // Check for eligible 1-votes on leader blocks
        for (block_key, _block) in get_pending_lead_blocks(state, current_view) {
            if block_key.view == current_view {
                effects.push(Effect::VoteSent {
                    vote_type: 1,
                    block_key: block_key.clone(),
                    target: None,
                });
            }
        }

        // Check for eligible 2-votes on leader blocks
        for qc in get_pending_lead_1qcs(state, current_view) {
            if qc.data.for_which.view == current_view {
                effects.push(Effect::VoteSent {
                    vote_type: 2,
                    block_key: qc.data.for_which.clone(),
                    target: None,
                });
            }
        }
    }

    effects
}

// === Helper Functions ===

fn leader(view: ViewNum, n: u32) -> Identity {
    Identity((view.0 as u32 % n) + 1)
}

fn validate_block<Tr: Transaction>(
    state: &ProcessState<Tr>,
    block: &Arc<Signed<Block<Tr>>>,
    kb: &KeyBook,
    n: u32,
    f: u32,
) -> Result<(), BlockValidationError> {
    crate::block_validation::block_valid(kb, n, f, &state.genesis_qc, block)
}

fn has_vote_from<Tr: Transaction>(
    state: &ProcessState<Tr>,
    sender: &Identity,
    vote_data: &VoteData,
) -> bool {
    state.vote_tracker
        .get(vote_data)
        .map(|votes| votes.contains_key(sender))
        .unwrap_or(false)
}

fn count_votes<Tr: Transaction>(state: &ProcessState<Tr>, vote_data: &VoteData) -> usize {
    state.vote_tracker
        .get(vote_data)
        .map(|votes| votes.len())
        .unwrap_or(0)
}

fn count_end_views<Tr: Transaction>(state: &ProcessState<Tr>, view: &ViewNum) -> usize {
    state.end_views
        .get(view)
        .map(|votes| votes.len())
        .unwrap_or(0)
}

fn get_votes_for<Tr: Transaction>(
    state: &ProcessState<Tr>,
    vote_data: &VoteData,
) -> Vec<Arc<ThreshPartial<VoteData>>> {
    state.vote_tracker
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
) -> FinishedQC {
    // Collect partial signatures indexed by author
    let mut vote_sigs: Vec<(usize, hints::PartialSignature)> = Vec::new();

    for vote in votes {
        if vote.data == *vote_data {
            // Convert Identity to index (identities are 1-indexed)
            let author_index = vote.author.0 as usize - 1;
            vote_sigs.push((author_index, vote.signature.clone()));
        }
    }

    // Make sure we have enough votes
    if vote_sigs.len() < (n - f) as usize {
        panic!(
            "Not enough votes to form QC: {} < {}",
            vote_sigs.len(),
            n - f
        );
    }

    // Aggregate the signatures
    let agg = kb
        .hints_setup
        .as_ref()
        .expect("hints setup should be initialized")
        .aggregator();
    let mut data = Vec::new();
    vote_data.serialize_compressed(&mut data).unwrap();

    let signature = hints::sign_aggregate(
        &agg,
        hints::F::from((n - f) as u64),
        &vote_sigs,
        &data,
    )
    .expect("Failed to aggregate signatures");

    Arc::new(ThreshSigned {
        data: vote_data.clone(),
        signature,
    })
}

fn form_view_certificate<Tr: Transaction>(
    _state: &ProcessState<Tr>,
    view: &ViewNum,
    _kb: &KeyBook,
    _f: u32,
) -> Arc<ThreshSigned<ViewNum>> {
    // TODO: In a real implementation, we would aggregate the threshold signatures
    Arc::new(ThreshSigned {
        data: view.incr(),
        signature: hints::Signature::default(),
    })
}

fn find_finalized_blocks<Tr: Transaction>(
    _state: &ProcessState<Tr>,
    qc: &FinishedQC,
) -> Vec<BlockKey> {
    // A block is finalized if it has a 2-QC and we've received the QC
    if qc.data.z == 2 {
        vec![qc.data.for_which.clone()]
    } else {
        vec![]
    }
}

fn find_maximal_unfinalized<Tr: Transaction>(
    state: &ProcessState<Tr>,
) -> Option<&FinishedQC> {
    state.unfinalized_2qc.iter().max_by(|a, b| {
        if state.observes(&a.data, &b.data) {
            std::cmp::Ordering::Greater
        } else if state.observes(&b.data, &a.data) {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Equal
        }
    })
}

fn has_unfinalized<Tr: Transaction>(state: &ProcessState<Tr>) -> bool {
    !state.unfinalized_qcs.is_empty()
}

fn can_produce_tr_block<Tr: Transaction>(state: &ProcessState<Tr>) -> bool {
    let has_transactions = !state.ready_transactions.is_empty();
    let slot = state.slot_tr;

    if !slot.is_zero() {
        // Check if we have the previous slot QC
        state.latest_tr_qc.is_some()
    } else {
        has_transactions
    }
}

fn can_produce_lead_block<Tr: Transaction>(
    state: &ProcessState<Tr>,
    _id: &Identity,
    f: u32,
) -> bool {
    let view = state.current_view;
    let has_produced = state
        .produced_lead_in_view
        .get(&view)
        .copied()
        .unwrap_or(false);

    if has_produced {
        // Check for previous QC
        state.latest_leader_1qc.is_some()
    } else {
        has_enough_start_views(state, view, f)
    }
}

fn has_enough_start_views<Tr: Transaction>(
    state: &ProcessState<Tr>,
    view: ViewNum,
    f: u32,
) -> bool {
    state
        .start_views
        .get(&view)
        .map(|msgs| msgs.len() >= (state.genesis_block.author.0 - f) as usize) // Hack to get n
        .unwrap_or(false)
}

fn contains_lead_in_view<Tr: Transaction>(state: &ProcessState<Tr>, view: ViewNum) -> bool {
    state
        .contains_lead_by_view
        .get(&view)
        .copied()
        .unwrap_or(false)
}

fn has_unfinalized_lead_in_view<Tr: Transaction>(
    state: &ProcessState<Tr>,
    view: ViewNum,
) -> bool {
    state
        .unfinalized_lead_by_view
        .get(&view)
        .map(|set| !set.is_empty())
        .unwrap_or(false)
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

fn get_pending_tr_1qcs<Tr: Transaction>(state: &ProcessState<Tr>, view: ViewNum) -> Vec<FinishedQC> {
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
    is_single_tip(state, block_key)
        && state.blocks.contains_key(block_key)
        && state.blocks[block_key]
            .data
            .one
            .data
            .compare_qc(&state.max_1qc.data)
            != std::cmp::Ordering::Less
}

fn is_eligible_for_tr_2_vote<Tr: Transaction>(
    state: &ProcessState<Tr>,
    block_key: &BlockKey,
) -> bool {
    let has_single_tip = state.tips.len() == 1
        && state.tips[0].data.z == 1
        && state.tips[0].data.for_which == *block_key;

    let no_higher_blocks = state.max_height.0 <= block_key.height;

    has_single_tip && no_higher_blocks
}

fn is_single_tip<Tr: Transaction>(state: &ProcessState<Tr>, block_key: &BlockKey) -> bool {
    if state.tips.len() != 1 {
        return false;
    }

    state.tips.first().map_or(false, |tip| {
        state
            .block_pointed_by
            .get(&tip.data.for_which)
            .map_or(false, |parents| {
                parents.len() == 1 && parents.contains(block_key)
            })
    })
}

// === Block Production Functions ===

fn produce_tr_block<Tr: Transaction>(
    state: &ProcessState<Tr>,
    kb: &KeyBook,
    id: &Identity,
) -> (Arc<Signed<Block<Tr>>>, Vec<Effect<Tr>>) {
    let slot = state.slot_tr;
    let view = state.current_view;

    // Determine previous QCs
    let mut prev_qcs = Vec::new();
    if !slot.is_zero() {
        if let Some(tr_qc) = &state.latest_tr_qc {
            if tr_qc.data.for_which.slot.is_pred(slot) {
                prev_qcs.push(tr_qc.clone());
            }
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

    let effects = vec![
        Effect::BlockProduced {
            block_type: BlockType::Tr,
            block_key: block_key.clone(),
        },
        Effect::SlotAdvanced {
            slot_type: BlockType::Tr,
            new_slot: SlotNum(slot.0 + 1),
        },
    ];

    (signed_block, effects)
}

fn produce_lead_block<Tr: Transaction>(
    state: &ProcessState<Tr>,
    kb: &KeyBook,
    id: &Identity,
    n: u32,
) -> (Arc<Signed<Block<Tr>>>, Vec<Effect<Tr>>) {
    let slot = state.slot_lead;
    let view = state.current_view;

    // Get tips as previous QCs
    let mut prev_qcs: Vec<FinishedQC> = state.tips.clone();

    // Add previous leader block QC if needed
    if !slot.is_zero() {
        if let Some(prev_qc) = &state.latest_leader_qc {
            if prev_qc.data.for_which.slot.is_pred(slot) && !prev_qcs
                .iter()
                .any(|qc| qc.data.for_which == prev_qc.data.for_which) {
                prev_qcs.push(prev_qc.clone());
            }
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
        let view_messages = state
            .start_views
            .get(&view)
            .cloned()
            .unwrap_or_default();

        let max_qc = view_messages
            .iter()
            .map(|msg| &msg.data.qc)
            .max_by(|a, b| a.data.compare_qc(&b.data))
            .cloned()
            .unwrap_or_else(|| state.max_1qc.clone());

        let final_qc =
            if max_qc.data.compare_qc(&state.max_1qc.data) == std::cmp::Ordering::Less {
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
            block_type: BlockType::Lead,
            block_key: block_key.clone(),
        },
        Effect::SlotAdvanced {
            slot_type: BlockType::Lead,
            new_slot: SlotNum(slot.0 + 1),
        },
        Effect::LeaderBlockProducedInView { view },
    ];

    (signed_block, effects)
}

fn has_produced_lead_in_view<Tr: Transaction>(
    state: &ProcessState<Tr>,
    view: ViewNum,
) -> bool {
    state
        .produced_lead_in_view
        .get(&view)
        .copied()
        .unwrap_or(false)
} 