//! # The Hellas Protocol: A High-Throughput Decentralized Compute Network
//!
//! This crate implements the Hellas protocol, a blockchain designed specifically for
//! decentralized AI compute marketplaces. It achieves seemingly contradictory goals:
//!
//! 1. **Ultra-low latency** for interactive workloads (e.g., streaming LLM responses)
//! 2. **Robust execution** for batch compute jobs with fraud protection
//!
//! The key insight is that both flows use the same on-chain primitives, just in
//! different patterns. This unified design enables massive throughput through:
//!
//! - **Object-centric state model** (inspired by Sui) for parallel execution
//! - **Bounded counter accounts** (from Stingray) for concurrent spending
//! - **Minimal on-chain data** - only cryptographic commitments stored
//! - **Smart clients, dumb protocol** - complexity lives off-chain
//!
//! ## Architecture Overview
//!
//! The protocol is structured as a state machine with fixed transaction types,
//! not a general-purpose VM. This enables aggressive optimization and formal
//! verification while keeping the implementation simple.

// Re-export core types for ergonomic API
pub use bounded_counter::{ValidatorBudgetState, ValidatorLocalState};
pub use crypto::{Signature, SigningKey, VerifyingKey};
pub use engine::{ExecutionError, StateTransitionEngine};
pub use negotiation::{JobAgreement, JobSpec, ProviderQuote};
pub use objects::{HellasAccount, JobEscrow, JobStatus, Object, ObjectMetadata};
pub use transactions::{SignedTransaction, Transaction, TransactionEffects};
pub use types::{Amount, BlockHeight, Hash, ObjectId, Pubkey, Version};

// Core modules that implement the protocol
pub mod bounded_counter;
pub mod collective_counter;
pub mod crypto;
pub mod engine;
pub mod fast_unlock;
pub mod negotiation;
pub mod objects;
pub mod observability;
pub mod parallel;
pub mod transactions;
pub mod types;

/// Configuration constants for the protocol
pub mod constants {
    use super::BlockHeight;

    /// Default challenge period for job finalization (in blocks)
    pub const DEFAULT_CHALLENGE_PERIOD: BlockHeight = 100;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_basics() {
        // Verify the protocol compiles and basic types work
        let _pubkey = Pubkey::default();
        let _object_id = ObjectId::new_random();
        assert_eq!(std::mem::size_of::<Pubkey>(), 32);
    }
}
