/// # Morpheus Protocol Implementation
///
/// This crate implements the Morpheus consensus protocol as described in the paper.
/// Morpheus is a Byzantine fault-tolerant consensus protocol that provides high throughput
/// during normal operation and gracefully degrades to a more traditional consensus
/// approach during periods of network instability.
///
/// ## Protocol Overview
///
/// Morpheus uses a DAG (Directed Acyclic Graph) of blocks with two types of blocks:
/// - **Transaction blocks**: Contain actual transactions and are produced by all processes
/// - **Leader blocks**: Produced by the leader of each view to order transaction blocks
///
/// The protocol operates in views, with each view having a designated leader.
/// Within each view, there are two phases:
/// - **High throughput phase (0)**: Leader blocks help order transaction blocks
/// - **Low throughput phase (1)**: Transaction blocks can be finalized directly
///
/// ## Implementation Structure
///
/// - `process.rs`: Defines the core `MorpheusProcess` struct and message handling
/// - `block_production.rs`: Implements block creation logic
/// - `state_tracking.rs`: Manages protocol state (blocks, QCs, DAG structure)
/// - `types.rs`: Defines protocol data types
/// - `mock_harness.rs`: Testing framework for the protocol
/// - `tracing_setup.rs`: Structured logging with tracing-rs
/// - `hades/`: Web-based visualization and debugging interface
///
/// ## Key Protocol Concepts
///
/// - **Quorum Certificates (QCs)**: Proofs that n-f processes have voted for a block
/// - **z-votes**: Votes at different levels (0, 1, 2) for blocks
/// - **Observes relation**: Defines the DAG structure and block ordering
/// - **View changes**: Allow progress when a leader is faulty
///
/// See `pseudocode.txt` for the original algorithm description from the paper.

mod types;
mod process;
mod invariants;
mod state_tracking;
mod block_production;
mod block_validation;

pub mod mock_harness;
pub mod debug_impls;
pub mod tracing_setup;

pub use types::*;
pub use process::*;
pub use invariants::InvariantViolation;
pub use block_validation::BlockValidationError;

// Re-export tracing macros for convenience
pub use tracing::{debug, error, info, trace, warn};

// Initialize tracing on load
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn wasm_init() {
    tracing_setup::init_tracing();
}

// Function to initialize tracing for non-wasm targets
#[cfg(not(target_arch = "wasm32"))]
pub fn init() {
    tracing_setup::init_tracing();
}