// lib.rs - Root module that ties everything together

mod types;
mod state;
mod actions;
mod models;
mod utils;

// Core protocol modules
pub mod blocks;
pub mod voting;
pub mod view_change;
pub mod ordering;

// Re-exports for public API
pub use types::*;
pub use state::*;
pub use actions::*;
pub use models::*;

use std::time::Duration;
use muchin::automaton::{RegisterModel, RunnerBuilder};

/// Configuration for the Morpheus protocol
#[derive(Clone, Debug)]
pub struct MorpheusConfig {
    /// Process ID for this node
    pub process_id: ProcessId,
    /// Total number of processes
    pub num_processes: usize,
    /// Maximum number of Byzantine faults (usually (n-1)/3)
    pub f: usize,
    /// Bound on message delays (Δ in the paper)
    pub delta: Duration,
}

/// Morpheus protocol instance
///
/// This is the main entry point for using the Morpheus protocol.
/// It provides a simple interface for adding transactions and querying
/// the current state.
pub struct Morpheus {
    /// The runner
    runner: muchin::automaton::Runner<MorpheusState>,
}

impl Morpheus {
    /// Create a new instance of the Morpheus protocol
    pub fn new(config: MorpheusConfig) -> Self {
        // Validate configuration
        assert!(config.f * 3 < config.num_processes, 
                "f must be less than n/3 for Byzantine fault tolerance");
        
        // Create the state
        let state = MorpheusState::new(
            config.process_id,
            config.num_processes,
            config.f,
            config.delta,
        );
        
        // Initialize the runner
        let runner = RunnerBuilder::<MorpheusState>::new()
            .register::<MorpheusModel>()
            .instance(state, || MorpheusAction::Tick.into())
            .build();
        
        Self { runner }
    }
    
    /// Run the protocol continuously
    pub fn run(&mut self) {
        self.runner.run();
    }
    
    /// Take a single step in the protocol
    pub fn step(&mut self) -> bool {
        self.runner.step()
    }
    
    /// Add a new transaction to the pending list
    pub fn add_transaction(&mut self, transaction: Transaction) {
        // Access state
        let state = &mut self.runner.state.substates[0];
        
        // Add to pending transactions
        state.pending_transactions.push(transaction);
        
        // Mark as ready to create a block
        state.payload_ready = true;
    }
    
    /// Get the current log of finalized transactions
    pub fn get_log(&self) -> Vec<Transaction> {
        // Access state
        let state = &self.runner.state.substates[0];
        
        // Extract log using the F function from the paper
        ordering::extract_log(state)
    }
    
    /// Get a reference to the underlying state
    pub fn get_state(&self) -> &MorpheusState {
        &self.runner.state.substates[0]
    }
    
    /// Get a mutable reference to the underlying state
    pub fn get_state_mut(&mut self) -> &mut MorpheusState {
        &mut self.runner.state.substates[0]
    }
    
    /// Get the current view
    pub fn get_current_view(&self) -> View {
        self.runner.state.substates[0].current_view
    }
    
    /// Get the number of finalized blocks
    pub fn get_num_finalized_blocks(&self) -> usize {
        self.runner.state.substates[0].finalized_blocks.len()
    }
    
    /// Check if a block is finalized
    pub fn is_block_finalized(&self, block_hash: &Hash) -> bool {
        self.runner.state.substates[0].finalized_blocks.contains(block_hash)
    }
    
    /// Check if this process is the leader of the current view
    pub fn is_current_leader(&self) -> bool {
        let state = &self.runner.state.substates[0];
        state.is_leader(state.current_view)
    }
    
    /// Get the current throughput phase (High/Low)
    pub fn get_current_phase(&self) -> ThroughputPhase {
        self.runner.state.substates[0].phase
    }
    
    /// Get the current leader for a given view 
    pub fn get_leader_for_view(&self, view: View) -> ProcessId {
        let state = &self.runner.state.substates[0];
        MorpheusState::get_leader(view, state.num_processes)
    }
}
