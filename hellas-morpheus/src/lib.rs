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
mod message_handling;
mod process;
mod snapshot;
mod state_tracking;
mod types;
mod view_management;
mod voting;

pub mod format;
pub mod test_harness;
pub mod tracing_setup;

use std::{fmt::Debug, hash::Hash};

use ark_serialize::{CanonicalDeserialize, CanonicalSerialize, Valid};
use serde::{Deserialize, Serialize};

pub use block_validation::BlockValidationError;
pub use crypto::*;
pub use invariants::InvariantViolation;
pub use process::*;
pub use state_tracking::{PendingVotes, StateIndex};
pub use types::*;
pub use voting::*;

pub trait Transaction:
    Sync + Clone + Eq + Ord + Hash + Valid + CanonicalDeserialize + CanonicalSerialize + Debug + 'static
{
}

#[derive(Debug)]
pub struct ArkSerialize<T>(pub T);

impl<T> redb::Value for ArkSerialize<T>
where
    T: Debug + CanonicalDeserialize + CanonicalSerialize,
{
    type SelfType<'a>
        = T
    where
        Self: 'a;

    type AsBytes<'a>
        = Vec<u8>
    where
        Self: 'a;

    fn fixed_width() -> Option<usize> {
        None
    }

    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
    where
        Self: 'a,
    {
        T::deserialize_compressed(data).unwrap()
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
    where
        Self: 'a,
        Self: 'b,
    {
        let mut writer = Vec::new();
        T::serialize_compressed(value, &mut writer).unwrap();
        writer
    }

    fn type_name() -> redb::TypeName {
        redb::TypeName::new(&format!("ArkSerialize<{}>", std::any::type_name::<T>()))
    }
}

impl<T> redb::Key for ArkSerialize<T>
where
    T: Debug + CanonicalDeserialize + CanonicalSerialize + Ord,
{
    fn compare(data1: &[u8], data2: &[u8]) -> std::cmp::Ordering {
        use redb::Value;
        Self::from_bytes(data1).cmp(&Self::from_bytes(data2))
    }
}

#[derive(Debug)]
pub struct Postcard<T>(pub T);

impl<T> redb::Value for Postcard<T>
where
    T: Debug + Serialize + for<'a> Deserialize<'a>,
{
    type SelfType<'a>
        = T
    where
        Self: 'a;

    type AsBytes<'a>
        = Vec<u8>
    where
        Self: 'a;

    fn fixed_width() -> Option<usize> {
        None
    }

    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
    where
        Self: 'a,
    {
        postcard::from_bytes(data).unwrap()
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
    where
        Self: 'a,
        Self: 'b,
    {
        postcard::to_stdvec(value).unwrap()
    }

    fn type_name() -> redb::TypeName {
        redb::TypeName::new(&format!("Postcard<{}>", std::any::type_name::<T>()))
    }
}

impl<T> redb::Key for Postcard<T>
where
    T: Debug + Serialize + for<'a> Deserialize<'a> + Ord,
{
    fn compare(data1: &[u8], data2: &[u8]) -> std::cmp::Ordering {
        use redb::Value;
        Self::from_bytes(data1).cmp(&Self::from_bytes(data2))
    }
}
