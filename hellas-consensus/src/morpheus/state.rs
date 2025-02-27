use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;
use muchin::automaton::ModelState;

use super::types::*;
use super::blocks::BlockState;
use super::voting::VoteState;
use super::view_change::ViewState;

/// Main state for the Morpheus protocol
///
/// This unified state structure integrates all protocol state components
/// in a single cohesive interface. It provides access to block, vote, and
/// view change operations through dedicated sub-states.
#[derive(Debug)]
pub struct MorpheusState {
    //
    // Configuration
    //
    
    /// Process ID of this node
    pub process_id: ProcessId,
    
    /// Total number of processes
    pub num_processes: usize,
    
    /// Maximum number of Byzantine faults (usually (n-1)/3)
    pub f: usize,
    
    /// Bound on message delays (Δ in the paper)
    pub delta: Duration,
    
    //
    // Protocol state
    //
    
    /// Current view
    pub current_view: View,
    
    /// Current transaction slot
    pub transaction_slot: Slot,
    
    /// Current leader slot
    pub leader_slot: Slot,
    
    /// Phase within current view (High or Low throughput)
    pub phase: ThroughputPhase,
    
    /// Whether PayloadReady is true (ready to create transaction block)
    pub payload_ready: bool,
    
    /// Pending transactions to include in next block
    pub pending_transactions: Vec<Transaction>,
    
    //
    // Component states
    //
    
    /// Block-related state
    pub block_state: BlockState,
    
    /// Voting-related state
    pub vote_state: VoteState,
    
    /// View change-related state
    pub view_state: ViewState,
}

impl MorpheusState {
    /// Create a new Morpheus state with the given configuration
    pub fn new(
        process_id: ProcessId,
        num_processes: usize,
        f: usize,
        delta: Duration,
    ) -> Self {
        let mut state = Self {
            process_id,
            num_processes,
            f,
            delta,
            current_view: View(0),
            transaction_slot: Slot(0),
            leader_slot: Slot(0),
            phase: ThroughputPhase::High,
            payload_ready: false,
            pending_transactions: Vec::new(),
            block_state: BlockState::new(),
            vote_state: VoteState::new(),
            view_state: ViewState::new(),
        };
        
        // Initialize with genesis block
        state.initialize_genesis();
        
        state
    }
    
    /// Initialize the state with the genesis block
    fn initialize_genesis(&mut self) {
        // Create genesis block
        let genesis = Block::new_genesis();
        let genesis_hash = genesis.hash();
        
        // Add to state
        self.block_state.add_block(genesis).unwrap();
        
        // Create a 1-QC for genesis
        let genesis_qc = QC {
            vote_type: VoteType::Vote1,
            block_type: BlockType::Genesis,
            view: View(0),
            height: Height(0),
            author: ProcessId(0),
            slot: Slot(0),
            block_hash: genesis_hash.clone(),
            signatures: ThresholdSignature(vec![]), // placeholder
        };
        
        // Add QC
        self.vote_state.add_qc(genesis_qc);
        
        // Record view entry time
        self.view_state.record_view_entry(View(0), self.delta);
    }
    
    /// Check if this process is the leader of the given view
    pub fn is_leader(&self, view: View) -> bool {
        self.process_id.0 == view.0 as usize % self.num_processes
    }
    
    /// Get the leader for a given view
    pub fn get_leader(view: View, num_processes: usize) -> ProcessId {
        ProcessId(view.0 as usize % num_processes)
    }
    
    /// Get QC with highest vote type for a block
    pub fn get_highest_qc(&self, block_hash: &Hash) -> Option<QC> {
        self.vote_state.get_highest_qc(block_hash)
    }
    
    /// Check if LeaderReady conditions are met
    pub fn is_leader_ready(&self) -> bool {
        if !self.is_leader(self.current_view) {
            return false;
        }
        
        let v = self.current_view;
        
        // Have we produced a leader block in this view?
        let has_produced_leader_block = self.block_state.blocks_by_author
            .get(&self.process_id)
            .map(|blocks| {
                blocks.iter().any(|((block_type, slot), hash)| {
                    *block_type == BlockType::Leader && 
                    self.block_state.blocks
                        .get(hash)
                        .map(|block| block.view == v)
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);
        
        if !has_produced_leader_block {
            // Case 1: First leader block of the view
            // Check if received enough view messages
            let view_messages = self.view_state.view_messages
                .get(&v)
                .map(|msgs| msgs.len())
                .unwrap_or(0);
            
            let quorum_size = self.num_processes - self.f;
            
            return view_messages >= quorum_size && 
                   (self.leader_slot.0 == 0 || 
                    self.vote_state.latest_qcs.iter().any(|((block_type, author, slot), _)| {
                        *block_type == BlockType::Leader && 
                        *author == self.process_id && 
                        slot.0 == self.leader_slot.0 - 1
                    }));
        } else {
            // Case 2: Subsequent leader block in the view
            // Check if we have 1-QC for previous leader block
            return self.vote_state.latest_qcs.iter().any(|((block_type, author, slot), qc)| {
                *block_type == BlockType::Leader && 
                *author == self.process_id && 
                slot.0 == self.leader_slot.0 - 1 && 
                qc.vote_type == VoteType::Vote1
            });
        }
    }
    
    /// Prune old state (from views earlier than min_view)
    pub fn prune_old_state(&mut self, min_view: View) {
        // Prune block state first
        self.block_state.prune_old_state(min_view);
        
        // Get remaining blocks for other pruning operations
        let blocks_to_retain: BTreeSet<_> = self.block_state.blocks.keys().cloned().collect();
        
        // Prune vote state
        self.vote_state.prune_old_state(min_view, &blocks_to_retain);
        
        // Prune view state
        self.view_state.prune_old_state(min_view);
    }
}