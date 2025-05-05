//! State tracking module
//!
//! This module manages the protocol state (M_i and Q_i), including blocks, quorum certificates,
//! and pending voting state. Functionality is split into submodules for clarity.

mod index;
mod pending_votes;
mod voting;
mod qc;
mod block;
mod observes;
mod pending;

pub use index::StateIndex;
pub use pending_votes::PendingVotes;