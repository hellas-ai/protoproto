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
//! Core Protocol Modules:
//! - `types.rs`: Core data structures and protocol types
//! - `process.rs`: Core MorpheusProcess struct definition
//! - `message_handling.rs`: Protocol message processing logic
//! - `block_production.rs`: Block creation logic
//! - `state_tracking.rs`: DAG management and state tracking
//! - `voting.rs`: Vote collection and quorum formation
//! - `finalization.rs`: Block finalization logic
//! - `view_management.rs`: View changes and phase transitions
//! - `block_validation.rs`: Block validation rules
//!
//! Supporting Modules:
//! - `crypto.rs`: Cryptographic primitives for the protocol
//! - `invariants.rs`: Invariant checking for protocol safety
//! - `format.rs`: String formatting for protocol structures
//! - `tracing_setup.rs`: Structured logging with tracing-rs
//! - `test_harness.rs`: Testing framework for the protocol
//!
//! ## Key Protocol Concepts
//!
//! - **Quorum Certificates (QCs)**: Proofs that n-f processes have voted for a block
//! - **z-votes**: Votes at different levels (0, 1, 2) for blocks
//! - **Observes relation**: Defines the DAG structure and block ordering
//! - **View changes**: Allow progress when a leader is faulty

// Core protocol modules
mod block_production;
mod block_validation;
mod crypto;
mod finalization;
mod invariants;
mod message_handling;
mod process;
mod state_tracking;
mod types;
mod view_management;
mod voting;

// Public modules
pub mod format;
pub mod test_harness;
pub mod tracing_setup;

// Public re-exports
pub use block_validation::BlockValidationError;
pub use crypto::*;
pub use invariants::InvariantViolation;
pub use process::*;
pub use state_tracking::{PendingVotes, StateIndex};
pub use types::*;
pub use voting::{Duplicate, QuorumTrack};

// Constants shared across modules
/// Complaint timeout (6Δ)
pub(crate) const COMPLAIN_TIMEOUT: u128 = 6;

/// End view timeout (12Δ)
pub(crate) const END_VIEW_TIMEOUT: u128 = 12;