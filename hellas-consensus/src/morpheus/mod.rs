// lib.rs - Root module that ties everything together

mod actions;
mod models;
mod state;
mod types;
mod utils;

// Core protocol modules
pub mod blocks;
pub mod ordering;
pub mod view_change;
pub mod voting;

// Re-exports for public API
pub use actions::*;
pub use models::*;
pub use state::*;
pub use types::*;

use muchin::automaton::{RegisterModel, RunnerBuilder};
use std::time::Duration;

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
        assert!(
            config.f * 3 < config.num_processes,
            "f must be less than n/3 for Byzantine fault tolerance"
        );

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
}
