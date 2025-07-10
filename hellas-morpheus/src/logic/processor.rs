//! Pure functional logic for processing actions
//!
//! This module contains all the pure logic functions that operate on
//! immutable ProcessState and return Effects without side effects.

use crate::logic::*;
use crate::*;
use std::sync::Arc;

/// Errors that can occur during action processing
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessingError {
    /// Invalid signature on a message
    InvalidSignature { message_type: &'static str },

    /// Block validation failed
    InvalidBlock(BlockValidationError),

    /// Vote processing error
    InvalidVote { reason: String },

    /// QC formation error  
    QcFormationError { reason: String },

    /// State inconsistency detected
    StateInconsistency { reason: String },
}

impl std::fmt::Display for ProcessingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProcessingError::InvalidSignature { message_type } => {
                write!(f, "Invalid signature on {}", message_type)
            }
            ProcessingError::InvalidBlock(err) => write!(f, "Invalid block: {}", err),
            ProcessingError::InvalidVote { reason } => write!(f, "Invalid vote: {}", reason),
            ProcessingError::QcFormationError { reason } => {
                write!(f, "QC formation error: {}", reason)
            }
            ProcessingError::StateInconsistency { reason } => {
                write!(f, "State inconsistency: {}", reason)
            }
        }
    }
}

impl std::error::Error for ProcessingError {}

/// Result type for processing operations
pub type ProcessingResult<T> = Result<T, ProcessingError>;

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
) -> ProcessingResult<Vec<Effect<Tr>>> {
    match action {
        Action::ProcessMessage { sender, payload } => {
            process_message_internal(state, sender, payload, kb, id, n, f)
        }
        Action::SetTime(time) => Ok(vec![Effect::TimeUpdated(*time)]),
        Action::SetReadyTransactions(transactions) => Ok(vec![Effect::TransactionsUpdated {
            transactions: transactions.clone(),
        }]),
        Action::CheckTimeouts => Ok(check_timeouts_effects(state, kb, id, n, f, delta)),
        Action::CheckProduceBlocks => Ok(check_produce_blocks_effects(state, kb, id, n, f)),
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
) -> ProcessingResult<Vec<Effect<Tr>>> {
    let mut effects = Vec::new();

    match message {
        Message::Block(block) => {
            effects.extend(process_block(state, block, kb, id, n, f)?);
        }
        Message::NewVote(vote) => {
            effects.extend(process_vote(state, sender, vote, kb)?);
        }
        Message::QC(qc) => {
            effects.extend(process_qc(state, qc, kb, id, n, f)?);
        }
        Message::EndView(end_view) => {
            effects.extend(process_end_view(sender, end_view, kb, n, f)?);
        }
        Message::EndViewCert(cert) => {
            effects.extend(process_end_view_cert(state, cert, kb, id, n, f)?);
        }
        Message::StartView(start_view) => {
            effects.extend(process_start_view(state, sender, start_view, kb, id, n)?);
        }
    }

    Ok(effects)
}

/// Process a block and produce effects
pub(crate) fn process_block<Tr: Transaction>(
    state: &ProcessState<Tr>,
    block: &Arc<Signed<Block<Tr>>>,
    kb: &KeyBook,
    id: &Identity,
    n: u32,
    f: u32,
) -> ProcessingResult<Vec<Effect<Tr>>> {
    let mut effects = Vec::new();

    // Validate the block first
    validate_block(state, block, kb, n, f).map_err(ProcessingError::InvalidBlock)?;

    // Record the block
    effects.push(Effect::BlockRecorded {
        block: block.clone(),
    });

    // If it's a transaction block, send 0-vote
    if block.data.key.type_ == BlockType::Tr {
        if let Some(author) = &block.data.key.author {
            effects.push(Effect::VoteSent {
                vote: make_vote(kb, 0, &block.data.key),
                target: Some(author.clone()),
            });
        }
    }

    // Record any QCs in the block
    for qc in &block.data.prev {
        effects.extend(process_qc(state, qc, kb, id, n, f)?);
    }
    effects.extend(process_qc(state, &block.data.one, kb, id, n, f)?);

    Ok(effects)
}

/// Process a QC and produce effects
pub(crate) fn process_qc<Tr: Transaction>(
    state: &ProcessState<Tr>,
    qc: &FinishedQC,
    kb: &KeyBook,
    id: &Identity,
    n: u32,
    f: u32,
) -> ProcessingResult<Vec<Effect<Tr>>> {
    let mut effects = Vec::new();

    // Skip signature validation for genesis QC
    if qc != &state.genesis_qc && !qc.valid_signature(kb, n - f) {
        return Err(ProcessingError::InvalidSignature { message_type: "QC" });
    }

    // Check if we already have this QC
    if state.qcs.contains(qc) {
        return Ok(effects);
    }

    // Record the QC
    effects.push(Effect::QcRecorded {
        qc: qc.clone(),
    });

    Ok(effects)
}

pub fn leader(view: ViewNum, n: u32) -> Identity {
    Identity((view.0 as u32 % n) + 1)
}

pub(crate) fn validate_block<Tr: Transaction>(
    state: &ProcessState<Tr>,
    block: &Arc<Signed<Block<Tr>>>,
    kb: &KeyBook,
    n: u32,
    f: u32,
) -> Result<(), BlockValidationError> {
    block_valid(kb, n, f, &state.genesis_qc, block)
}

pub(crate) fn find_maximal_unfinalized<Tr: Transaction>(
    state: &ProcessState<Tr>,
) -> Option<&FinishedQC> {
    state.unfinalized_qcs.values().flat_map(|v| v.iter()).max_by(|a, b| {
        if state.observes(&a.data, &b.data) {
            std::cmp::Ordering::Greater
        } else if state.observes(&b.data, &a.data) {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Equal
        }
    })
}

pub(crate) fn has_unfinalized<Tr: Transaction>(state: &ProcessState<Tr>) -> bool {
    !state.unfinalized_qcs.is_empty()
}
