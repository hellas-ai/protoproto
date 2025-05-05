use std::collections::BTreeSet;
use std::sync::Arc;

use crate::*;

/// Block finalization logic for the Morpheus protocol
impl MorpheusProcess {
    /// Finalizes blocks according to the protocol rules
    /// 
    /// This implements the finalization logic from the paper:
    /// "Process p_i regards q ∈ Q_i (and q.b) as final if there exists q' ∈ Q_i 
    /// such that q' ⪰ q and q is a 2-QC (for any block)."
    /// 
    /// Arguments:
    /// - qc: The QC that might finalize other blocks
    /// 
    /// Returns a set of block keys that were finalized
    pub(crate) fn process_finalization(&mut self, qc: &Arc<ThreshSigned<VoteData>>) -> BTreeSet<BlockKey> {
        let mut finalized_blocks = BTreeSet::new();
        
        // Only attempt to finalize blocks when we receive a new QC
        // Finalization occurs when a QC observes a 2-QC
        
        // Find all unfinalized 2-QCs that this QC observes (and thus finalizes)
        let finalized_qcs = self
            .index
            .unfinalized_2qc
            .iter()
            .cloned()
            .filter(|unfinalized_2qc| self.observes(qc.data.clone(), unfinalized_2qc))
            .collect::<BTreeSet<_>>();
        
        // The current QC could also be a 2-QC (which might be finalized by future QCs)
        // Add it to unfinalized_2qc AFTER scanning to avoid incorrectly finalizing itself
        if qc.data.z == 2 {
            self.index.unfinalized_2qc.insert(qc.data.clone());
        }
        
        // Remove finalized QCs from unfinalized_2qc tracking
        self.index
            .unfinalized_2qc
            .retain(|unfinalized_2qc| !finalized_qcs.contains(unfinalized_2qc));
        
        // Finalize the blocks associated with these QCs
        for finalized_qc in finalized_qcs {
            self.finalize_block(&finalized_qc.for_which);
            finalized_blocks.insert(finalized_qc.for_which.clone());
        }
        
        finalized_blocks
    }
    
    /// Finalizes a specific block
    /// 
    /// Updates all the relevant tracking data structures when a block is finalized.
    fn finalize_block(&mut self, block_key: &BlockKey) {
        tracing::debug!(target: "finalized", block_key = ?block_key);
        
        // Remove the block from view-specific unfinalized leader tracking
        self.index
            .unfinalized_lead_by_view
            .entry(block_key.view)
            .or_default()
            .remove(block_key);
        
        // Remove the block from general unfinalized tracking
        self.index.unfinalized.remove(block_key);
        
        // Mark the block as finalized
        self.index
            .finalized
            .insert(block_key.clone(), true);
        
        // Re-evaluate pending votes for this view
        self.pending_votes
            .entry(block_key.view)
            .or_default()
            .dirty = true;
    }
    
    /// Determines if one QC observes another according to the observes relation ⪰
    ///
    /// Implements the observes relation from the pseudocode:
    /// "We define the 'observes' relation ⪰ on Q_i to be the minimal preordering satisfying (transitivity and):
    /// • If q,q' ∈ Q_i, q.type = q'.type, q.auth = q'.auth and q.slot > q'.slot, then q ⪰ q'.
    /// • If q,q' ∈ Q_i, q.type = q'.type, q.auth = q'.auth, q.slot = q'.slot, and q.z ≥ q'.z, then q ⪰ q'."
    /// • If q,q' ∈ Q_i, q.b = b, q'.b = b', b ∈ M_i and b points to b', then q ⪰ q'."
    ///
    /// Implementation uses a BFS on the points-to graph combined with direct observation checks.
    pub fn observes(&self, root: VoteData, needle: &VoteData) -> bool {
        // do a BFS from root to see if it observes needle
        let mut observed = false;
        let mut to_visit: std::collections::VecDeque<VoteData> = std::collections::VecDeque::from([root]);
        let mut visited = BTreeSet::new();
        
        while !to_visit.is_empty() {
            let node = to_visit.pop_front().unwrap();
            
            // Skip already visited nodes to avoid cycles
            if !visited.insert(node.clone()) {
                continue;
            }
            
            // Check direct observation first
            if self.directly_observes(&node, needle) {
                observed = true;
                break;
            }
            
            // Add block's predecessors to BFS queue (if block exists)
            if let Some(block) = self.index.blocks.get(&node.for_which) {
                for prev in &block.data.prev {
                    to_visit.push_back(prev.data.clone());
                }
            } else {
                tracing::warn!("Block not found for {:?}", node.for_which);
            }
        }
        
        observed
    }

    /// Determines if one QC directly observes another (without transitivity)
    ///
    /// Implements the direct observation component of the observes relation ⪰:
    /// 
    /// 1. Same type, same author, higher slot number
    /// 2. Same type, same author, same slot, higher or equal z-level
    /// 3. Direct pointer from one block to another
    pub fn directly_observes(&self, looks: &VoteData, seen: &VoteData) -> bool {
        // Rule 1: Same type, same author, higher slot
        if looks.for_which.type_ == seen.for_which.type_
            && looks.for_which.author == seen.for_which.author
            && looks.for_which.slot > seen.for_which.slot
        {
            return true;
        }
        
        // Rule 2: Same type, same author, same slot, higher or equal z
        if looks.for_which.type_ == seen.for_which.type_
            && looks.for_which.author == seen.for_which.author
            && looks.for_which.slot == seen.for_which.slot
            && looks.z >= seen.z
        {
            return true;
        }
        
        // Rule 3: Direct pointer from one block to another
        if let Some(block) = self.index.blocks.get(&looks.for_which) {
            if block
                .data
                .prev
                .iter()
                .any(|prev| prev.data.for_which == seen.for_which)
            {
                return true;
            }
        }
        
        false
    }
}