//! Tools for formatting Morpheus protocol types for logging and debugging.

use std::fmt::Write;

use ark_serialize::CanonicalDeserialize;
use ark_serialize::CanonicalSerialize;
use ark_serialize::Valid;

use crate::Transaction;
use crate::crypto::*;
use crate::types::*;

/// Format a BlockType in a concise way
pub fn format_block_type(block_type: &BlockType) -> String {
    match block_type {
        BlockType::Genesis => "Gen".to_string(),
        BlockType::Lead => "Lead".to_string(),
        BlockType::Tr => "Tr".to_string(),
    }
}

/// Format a ViewNum in a concise way
pub fn format_view_num(view_num: &ViewNum) -> String {
    format!("v{}", view_num.0)
}

/// Format a SlotNum in a concise way
pub fn format_slot_num(slot_num: &SlotNum) -> String {
    format!("s{}", slot_num.0)
}

/// Format an Identity in a concise way
pub fn format_identity(identity: &Identity) -> String {
    format!("p{}", identity.0)
}

/// Format a BlockHash in a concise way
pub fn format_block_hash(hash: &BlockHash) -> String {
    format!("#{:x}", hash.0)
}

/// Format a BlockKey in a concise way
pub fn format_block_key(key: &BlockKey) -> String {
    let mut result = format_block_type(&key.type_);

    match key.type_ {
        BlockType::Genesis => result.push_str("[Genesis]"),
        _ => {
            write!(
                result,
                "[{},{},h{}",
                format_view_num(&key.view),
                format_slot_num(&key.slot),
                key.height
            )
            .unwrap();

            if let Some(author) = &key.author {
                write!(result, ",{}", format_identity(author)).unwrap();
            }

            if let Some(hash) = &key.hash {
                write!(result, ",{}", format_block_hash(hash)).unwrap();
            }

            result.push(']');
        }
    }

    result
}

/// Format a VoteData in a concise way
pub fn format_vote_data(vote_data: &VoteData, verbose: bool) -> String {
    if verbose {
        format!(
            "VoteData{{ z: {}, for_which: {} }}",
            vote_data.z,
            format_block_key(&vote_data.for_which)
        )
    } else {
        format!("{}-{}", vote_data.z, format_block_key(&vote_data.for_which))
    }
}

/// Format a signed value in a concise way
pub fn format_signed<T: CanonicalSerialize + CanonicalDeserialize + Valid>(
    signed: &Signed<T>,
    value_formatter: impl Fn(&T) -> String,
    verbose: bool,
) -> String {
    if verbose {
        format!(
            "Signed{{ data: {}, author: {} }}",
            value_formatter(&signed.data),
            format_identity(&signed.author)
        )
    } else {
        format!(
            "{}[{}]",
            value_formatter(&signed.data),
            format_identity(&signed.author)
        )
    }
}

/// Format a signed value in a concise way
pub fn format_thresh_partial<T: CanonicalSerialize + CanonicalDeserialize + Valid>(
    signed: &ThreshPartial<T>,
    value_formatter: impl Fn(&T) -> String,
    verbose: bool,
) -> String {
    if verbose {
        format!(
            "ThreshPartial{{ data: {}, author: {} }}",
            value_formatter(&signed.data),
            format_identity(&signed.author)
        )
    } else {
        format!(
            "{}[{}]",
            value_formatter(&signed.data),
            format_identity(&signed.author)
        )
    }
}

/// Format a threshold signed value in a concise way
pub fn format_thresh_signed<T: CanonicalSerialize + CanonicalDeserialize + Valid>(
    signed: &ThreshSigned<T>,
    value_formatter: impl Fn(&T) -> String,
    verbose: bool,
) -> String {
    if verbose {
        format!("ThreshSigned{{ data: {} }}", value_formatter(&signed.data))
    } else {
        format!("QC({})", value_formatter(&signed.data))
    }
}

/// Format a StartView in a concise way
pub fn format_start_view(start_view: &StartView, verbose: bool) -> String {
    if verbose {
        format!(
            "StartView{{ view: {}, qc: {} }}",
            format_view_num(&start_view.view),
            format_thresh_signed(&start_view.qc, |vd| format_vote_data(vd, false), false)
        )
    } else {
        format!(
            "Start({},qc:{})",
            format_view_num(&start_view.view),
            format_vote_data(&start_view.qc.data, false)
        )
    }
}

/// Format BlockData in a concise way
pub fn format_block_data<Tr: Transaction>(data: &BlockData<Tr>, verbose: bool) -> String {
    match data {
        BlockData::Genesis => "Genesis".to_string(),
        BlockData::Tr { transactions } => {
            if verbose {
                let tx_strs: Vec<_> = transactions
                    .iter()
                    .map(|tx| format_transaction(tx, false))
                    .collect();
                format!("Tr{{ transactions: [{}] }}", tx_strs.join(", "))
            } else {
                format!("Tr[{} txs]", transactions.len())
            }
        }
        BlockData::Lead { justification } => {
            if verbose {
                let just_strs: Vec<_> = justification
                    .iter()
                    .map(|j| format_signed(j, |sv| format_start_view(sv, false), false))
                    .collect();
                format!("Lead{{ justification: [{}] }}", just_strs.join(", "))
            } else {
                format!("Lead[{} just]", justification.len())
            }
        }
    }
}

/// Format a Transaction in a concise way
pub fn format_transaction<Tr: Transaction>(tx: &Tr, _verbose: bool) -> String {
    format!("Tx({:?})", tx)
}

/// Format a Block in a concise way
pub fn format_block<Tr: Transaction>(block: &Block<Tr>, verbose: bool) -> String {
    if verbose {
        format!(
            "Block{{ key: {}, prev: [{}], one: {}, data: {} }}",
            format_block_key(&block.key),
            block
                .prev
                .iter()
                .map(|qc| format_thresh_signed(qc, |vd| format_vote_data(vd, false), false))
                .collect::<Vec<_>>()
                .join(", "),
            format_thresh_signed(&block.one, |vd| format_vote_data(vd, false), false),
            format_block_data(&block.data, true)
        )
    } else {
        format!(
            "Block{}[prev:{},1qc:{}]",
            format_block_key(&block.key),
            block.prev.len(),
            format_vote_data(&block.one.data, false)
        )
    }
}

/// Format a Message in a concise way
pub fn format_message<Tr: Transaction>(message: &Message<Tr>, verbose: bool) -> String {
    match message {
        Message::Block(signed_block) => {
            if verbose {
                format!(
                    "Block({})",
                    format_signed(signed_block, |b| format_block(b, true), true)
                )
            } else {
                format!("Block({})", format_block_key(&signed_block.data.key))
            }
        }
        Message::NewVote(vote) => {
            if verbose {
                format!(
                    "NewVote({})",
                    format_thresh_partial(vote, |vd| format_vote_data(vd, true), true)
                )
            } else {
                format!(
                    "Vote({},{})",
                    format_vote_data(&vote.data, false),
                    format_identity(&vote.author)
                )
            }
        }
        Message::QC(qc) => {
            if verbose {
                format!(
                    "QC({})",
                    format_thresh_signed(qc, |vd| format_vote_data(vd, true), true)
                )
            } else {
                format!("QC({})", format_vote_data(&qc.data, false))
            }
        }
        Message::EndView(view) => {
            if verbose {
                format!(
                    "EndView({})",
                    format_thresh_partial(view, |v| format_view_num(v), true)
                )
            } else {
                format!(
                    "EndView({},{})",
                    format_view_num(&view.data),
                    format_identity(&view.author)
                )
            }
        }
        Message::EndViewCert(cert) => {
            if verbose {
                format!(
                    "EndViewCert({})",
                    format_thresh_signed(cert, |v| format_view_num(v), true)
                )
            } else {
                format!("EndViewCert({})", format_view_num(&cert.data))
            }
        }
        Message::StartView(start_view) => {
            if verbose {
                format!(
                    "StartView({})",
                    format_signed(start_view, |sv| format_start_view(sv, true), true)
                )
            } else {
                format!(
                    "StartView({},{})",
                    format_view_num(&start_view.data.view),
                    format_identity(&start_view.author)
                )
            }
        }
    }
}

/// Format a Phase in a concise way
pub fn format_phase(phase: &Phase) -> String {
    match phase {
        Phase::High => "High".to_string(),
        Phase::Low => "Low".to_string(),
    }
}

// Add logging macros that use our custom formatters
#[macro_export]
macro_rules! protocol_log {
    ($($arg:tt)*) => {
        println!("[PROTOCOL] {}", format!($($arg)*))
    };
}

#[macro_export]
macro_rules! block_log {
    ($block:expr) => {
        println!("[BLOCK] {}", $crate::format::format_block($block, false))
    };
    ($block:expr, true) => {
        println!("[BLOCK] {}", $crate::format::format_block($block, true))
    };
}

#[macro_export]
macro_rules! vote_log {
    ($vote:expr) => {
        println!("[VOTE] {}", $crate::format::format_vote_data($vote, false))
    };
}

#[macro_export]
macro_rules! qc_log {
    ($qc:expr) => {
        println!(
            "[QC] {}",
            $crate::format::format_thresh_signed(
                $qc,
                |vd| $crate::format::format_vote_data(vd, false),
                false
            )
        )
    };
}

#[macro_export]
macro_rules! message_log {
    ($msg:expr) => {
        println!("[MSG] {}", $crate::format::format_message($msg, false))
    };
    ($msg:expr, true) => {
        println!("[MSG] {}", $crate::format::format_message($msg, true))
    };
}
