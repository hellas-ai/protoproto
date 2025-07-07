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
//! - `process.rs`: Defines the core `MorpheusProcess` struct with component-based architecture
//! - `processor.rs`: Implements pure functional action processing
//! - `effects.rs`: Defines state mutation effects
//! - `actions.rs`: Defines external action types
//! - `state_tracking.rs`: Manages protocol state (blocks, QCs, DAG structure)
//! - `types.rs`: Defines protocol data types
//! - `test_harness.rs`: Testing framework for the protocol
//! - `tracing_setup.rs`: Structured logging with tracing-rs

mod actions;
mod block_producer;
mod block_validation;
mod crypto;
mod dag_index;
mod effects;
mod event_log;
mod process;
mod processor;
mod qc_index;
mod serialization;
mod state_tracking;
mod timeout_manager;
mod types;
mod view_index;
mod view_manager;
mod vote_manager;
mod config;

pub mod format;
pub mod test_harness;
pub mod tracing_setup;

use std::{fmt::Debug, hash::Hash};

use ark_serialize::{CanonicalDeserialize, CanonicalSerialize, Valid};

pub use actions::Action;
pub use block_validation::BlockValidationError;
pub use crypto::*;
pub use effects::Effect;
pub use event_log::{EventLog, LogEntry, default_snapshots_table as snapshots_table_default};
pub use process::*;
pub use state_tracking::{PendingVotes, StateIndex};
pub use types::*;
pub use processor::{ActionProcessor, ProcessState};

pub trait Transaction:
    Sync + Clone + Default + Eq + Ord + Hash + Valid + CanonicalDeserialize + CanonicalSerialize + Debug + serde::Serialize + for<'de> serde::Deserialize<'de> + 'static
{
}
