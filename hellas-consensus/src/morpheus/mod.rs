// lib.rs - Main library file
mod types;
mod state;
mod protocol;

pub use types::*;
pub use state::*;
pub use protocol::*;

// Morpheus protocol driver
pub struct Morpheus {
    state: MorpheusState,
}

impl Morpheus {
    // Create a new protocol instance
    pub fn new(process_id: ProcessId, num_processes: usize, f: usize, delta: Duration) -> Self {
        assert!(f * 3 < num_processes, "f must be less than n/3 for BFT");
        Self { state: MorpheusState::new(process_id, num_processes, f, delta) }
    }
    
    // Public API methods
    pub fn process_block(&mut self, block: Block) -> Vec<Effect> {
        let (new_state, effects) = protocol::process_block(&self.state, block);
        self.state = new_state;
        effects
    }
    
    pub fn process_vote(&mut self, vote: Vote) -> Vec<Effect> {
        let (new_state, effects) = protocol::process_vote(&self.state, vote);
        self.state = new_state;
        effects
    }
    
    pub fn create_transaction_block(&mut self) -> (Block, Vec<Effect>) {
        let (new_state, block, effects) = protocol::create_transaction_block(&self.state);
        self.state = new_state;
        (block, effects)
    }
    
    // Additional API methods (abbreviated)
    // ...
    
    // Get state for inspection
    pub fn state(&self) -> &MorpheusState {
        &self.state
    }
}