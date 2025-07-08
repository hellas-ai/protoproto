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

mod config;
mod crypto;
mod logic;
mod process;
mod storage;
mod state;
mod types;


pub mod format;
pub mod test_harness;
pub mod tracing_setup;

use std::{fmt::Debug, hash::Hash};

use ark_serialize::{CanonicalDeserialize, CanonicalSerialize, Valid};

pub use crypto::*;
pub use logic::*;
pub use process::*;
pub use state::{ProcessState, PendingVotes};
pub use storage::*;
pub use types::*;

pub trait Transaction:
    Send
    + Sync
    + Clone
    + Default
    + Eq
    + Ord
    + Hash
    + Valid
    + CanonicalDeserialize
    + CanonicalSerialize
    + Debug
    + serde::Serialize
    + for<'de> serde::Deserialize<'de>
    + 'static
{
}
