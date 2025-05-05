//! # Morpheus Protocol Implementation
//!
//! This crate implements the Morpheus consensus protocol as described in the paper.
//! Morpheus is a Byzantine fault-tolerant consensus protocol that provides high throughput
//! during normal operation and gracefully degrades to a more traditional consensus
//! approach during periods of network instability.
//!
//! ## Protocol Overview
//!
//! Morpheus uses a DAG (Directed Acyclic Graph) of blocks with two types of blocks:
//! - **Transaction blocks**: Contain actual transactions and are produced by all processes
//! - **Leader blocks**: Produced by the leader of each view to order transaction blocks
//!
//! The protocol operates in views, with each view having a designated leader.
//! Within each view, there are two phases:
//! - **High throughput phase (0)**: Leader blocks help order transaction blocks
//! - **Low throughput phase (1)**: Transaction blocks can be finalized directly
//!
//! ## Implementation Structure
//!
//! - `process.rs`: Defines the core `MorpheusProcess` struct and message handling
//! - `block_production.rs`: Implements block creation logic
//! - `state_tracking.rs`: Manages protocol state (blocks, QCs, DAG structure)
//! - `types.rs`: Defines protocol data types
//! - `mock_harness.rs`: Testing framework for the protocol
//! - `tracing_setup.rs`: Structured logging with tracing-rs
//! - `hades/`: Web-based visualization and debugging interface
//!
//! ## Key Protocol Concepts
//!
//! - **Quorum Certificates (QCs)**: Proofs that n-f processes have voted for a block
//! - **z-votes**: Votes at different levels (0, 1, 2) for blocks
//! - **Observes relation**: Defines the DAG structure and block ordering
//! - **View changes**: Allow progress when a leader is faulty

mod block_production;
mod block_validation;
mod crypto;
mod invariants;
mod process;
mod state_tracking;
mod types;

pub mod format;
pub mod test_harness;
pub mod tracing_setup;

pub use block_validation::BlockValidationError;
pub use crypto::*;
pub use invariants::InvariantViolation;
pub use process::*;
pub use state_tracking::{PendingVotes, StateIndex};
pub use types::*;
