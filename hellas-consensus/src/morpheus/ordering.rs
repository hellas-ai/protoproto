use std::collections::{BTreeMap, BTreeSet};
use log::{debug, warn};

use crate::types::*;
use crate::state::MorpheusState;

/// Result of block ordering
#[derive(Debug, Clone)]
pub struct OrderedTransactions {
    /// Sequence of transaction blocks in order
    pub blocks: Vec<Block>,
    /// Flattened list of all transactions in order
    pub transactions: Vec<Transaction>,
}

/// Compute a deterministic ordering of blocks that respects the "observes" relation
///
/// This implements tau_dagger([b] - [b']) from the paper
fn compute_deterministic_ordering(
    state: &MorpheusState,
    blocks: &BTreeSet<Hash>
) -> Vec<Hash> {
    // Convert to vector for sorting
    let mut block_vec: Vec<_> = blocks.iter().cloned().collect();
    
    // Sort blocks deterministically as defined in the paper
    block_vec.sort_by(|a, b| {
        let block_a = match state.block_state.blocks.get(a) {
            Some(block) => block,
            None => {
                warn!("Block hash {} not found in state", a);
                return std::cmp::Ordering::Equal;
            }
        };
        
        let block_b = match state.block_state.blocks.get(b) {
            Some(block) => block,
            None => {
                warn!("Block hash {} not found in state", b);
                return std::cmp::Ordering::Equal;
            }
        };
        
        // First sort by view
        match block_a.view.cmp(&block_b.view) {
            std::cmp::Ordering::Equal => {
                // Then by type (genesis < leader < transaction)
                match block_a.block_type.cmp(&block_b.block_type) {
                    std::cmp::Ordering::Equal => {
                        // Same types, sort by height
                        block_a.height.cmp(&block_b.height)
                    },
                    other => other,
                }
            },
            other => other,
        }
    });
    
    block_vec
}

/// Extract a total ordering of blocks and transactions recursively
///
/// This implements the tau function from the paper:
/// - tau(b_g) = b_g
/// - If b ≠ b_g, let q = b.1-QC and b' = q.b
///   Then tau(b) = tau(b') * tau_dagger([b] - [b'])
fn compute_tau(
    state: &MorpheusState,
    block_hash: &Hash
) -> Vec<Hash> {
    // Check if block exists
    if !state.block_state.blocks.contains_key(block_hash) {
        warn!("compute_tau: Block {} not found in state", block_hash);
        return Vec::new(); // Return empty ordering for missing blocks
    }
    
    let block = &state.block_state.blocks[block_hash];
    
    if block.block_type == BlockType::Genesis {
        // Base case: tau(b_g) = b_g
        return vec![block_hash.clone()];
    }
    
    // Get 1-QC's block
    let qc = &block.qc;
    let qc_block_hash = &qc.block_hash;
    
    // Check if QC block exists
    if !state.block_state.blocks.contains_key(qc_block_hash) {
        warn!("compute_tau: QC block {} not found in state", qc_block_hash);
        return vec![block_hash.clone()]; // Just return this block as fallback
    }
    
    // Get all blocks observed by current block
    let current_observed = state.block_state.observes.get(block_hash).cloned().unwrap_or_default();
    
    // Get all blocks observed by QC's block
    let qc_observed = state.block_state.observes.get(qc_block_hash).cloned().unwrap_or_default();
    
    // Get blocks in [b] - [b']
    let mut difference = current_observed;
    for hash in &qc_observed {
        difference.remove(hash);
    }
    
    // Compute tau(b')
    let mut result = compute_tau(state, qc_block_hash);
    
    // Compute tau_dagger([b] - [b'])
    let difference_ordered = compute_deterministic_ordering(state, &difference);
    
    // tau(b) = tau(b') * tau_dagger([b] - [b'])
    result.extend(difference_ordered);
    
    result
}

/// Compute the total ordering of transactions based on a 2-QC block
///
/// This implements the F function from the paper:
/// 1. Find the largest set of blocks M' in M that is downward closed
/// 2. Let q be a maximal 2-QC in M such that q.b ∈ M', and set b = q.b
/// 3. F(M) = Tr(tau(b))
pub fn compute_total_ordering(state: &MorpheusState) -> OrderedTransactions {
    // M' is already maintained as our blocks collection, which is downward closed
    
    // Get all finalized blocks (those with a 2-QC)
    if state.vote_state.finalized_blocks.is_empty() {
        // If no finalized blocks, use genesis
        for (hash, block) in &state.block_state.blocks {
            if block.block_type == BlockType::Genesis {
                return OrderedTransactions {
                    blocks: vec![block.clone()],
                    transactions: Vec::new(),
                };
            }
        }
        
        // No blocks at all
        return OrderedTransactions {
            blocks: Vec::new(),
            transactions: Vec::new(),
        };
    }
    
    // Find the block with the maximal 2-QC
    let mut max_qc = None;
    let mut max_block_hash = None;
    
    for hash in &state.vote_state.finalized_blocks {
        // Skip blocks that aren't actually finalized according to local state
        if !state.vote_state.is_finalized(hash) {
            continue;
        }
        
        if let Some(qc) = state.vote_state.get_qc(VoteType::Vote2, hash) {
            if let Some(current_max) = &max_qc {
                if qc.is_greater_than(current_max) {
                    max_qc = Some(qc.clone());
                    max_block_hash = Some(hash.clone());
                }
            } else {
                max_qc = Some(qc.clone());
                max_block_hash = Some(hash.clone());
            }
        }
    }
    
    let ordered_hashes = if let Some(block_hash) = max_block_hash {
        // Compute tau(b) for the block with maximal 2-QC
        compute_tau(state, &block_hash)
    } else {
        // Fallback to genesis if no 2-QCs (shouldn't happen if finalized_blocks is non-empty)
        for (hash, block) in &state.block_state.blocks {
            if block.block_type == BlockType::Genesis {
                return OrderedTransactions {
                    blocks: vec![block.clone()],
                    transactions: Vec::new(),
                };
            }
        }
        
        Vec::new()
    };
    
    // Map hashes to blocks
    let ordered_blocks: Vec<_> = ordered_hashes.iter()
        .filter_map(|hash| state.block_state.blocks.get(hash).cloned())
        .collect();
    
    // Extract transaction blocks and their transactions
    let transaction_blocks: Vec<_> = ordered_blocks.iter()
        .filter(|block| block.block_type == BlockType::Transaction)
        .collect();
    
    // Flatten transactions
    let mut all_transactions = Vec::new();
    for block in transaction_blocks {
        all_transactions.extend(block.transactions.clone());
    }
    
    OrderedTransactions {
        blocks: ordered_blocks,
        transactions: all_transactions,
    }
}

/// Get the extracted SMR log from the Morpheus state
///
/// This implements the F function from the paper for extractable SMR
pub fn extract_log(state: &MorpheusState) -> Vec<Transaction> {
    compute_total_ordering(state).transactions
}