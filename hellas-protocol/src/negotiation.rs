//! # Off-Chain Negotiation Types
//!
//! This module defines the data structures used during the off-chain negotiation
//! phase between requestors and providers. These types are used for:
//!
//! 1. Broadcasting job requirements
//! 2. Collecting and evaluating provider quotes
//! 3. Selecting providers based on various criteria
//!
//! None of these structures go on-chain directly; only their hashes and the
//! final agreements are recorded in the blockchain.

use crate::types::{BlockHeight, Hash, Pubkey};
use serde::{Deserialize, Serialize};

/// Security parameters that requestors can specify for their jobs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityParams {
    /// Minimum stake required from provider (as ratio of payment)
    pub min_stake_ratio: f64,

    /// Length of challenge period in blocks
    pub challenge_period: BlockHeight,

    /// Whether early finalization is allowed (for trusted relationships)
    pub allow_early_finalization: bool,
}

impl Default for SecurityParams {
    fn default() -> Self {
        Self {
            min_stake_ratio: 1.0,  // Provider must stake equal to payment
            challenge_period: 100, // ~10 minutes at 6s blocks
            allow_early_finalization: false,
        }
    }
}

/// Complete job specification broadcast by requestors
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobSpec {
    /// Hash of the catgrad computation graph
    pub catgrad_graph_hash: Hash,

    /// Hashes of input data (weights, inputs, etc.)
    pub input_hashes: Vec<Hash>,

    /// Execution requirements
    pub requirements: JobRequirements,

    /// Security parameters
    pub security_params: SecurityParams,

    /// Maximum price requestor is willing to pay
    pub max_price: u64,

    /// Unique nonce to prevent replay
    pub nonce: u64,
}

impl JobSpec {
    /// Check if this job is interactive (streaming with low latency)
    pub fn is_interactive(&self) -> bool {
        self.requirements.is_streaming
            && self
                .requirements
                .max_latency_ms
                .is_some_and(|ms| ms <= 1000)
    }
}

/// Hardware and performance requirements for a job
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRequirements {
    /// Minimum GPU memory required (in GB)
    pub min_gpu_memory: Option<u32>,

    /// Maximum acceptable latency (in milliseconds)
    pub max_latency_ms: Option<u64>,

    /// Whether the job is streaming (e.g., LLM chat) or batch
    pub is_streaming: bool,

    /// Estimated FLOPs for the computation
    pub estimated_flops: Option<u64>,
}

/// Quote from a provider in response to a job specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderQuote {
    /// The job this quote is for
    pub job_spec_hash: Hash,

    /// Provider's identity
    pub provider: Pubkey,

    /// Price in HELL tokens
    pub price: u64,

    /// Estimated latency in milliseconds
    pub estimated_latency_ms: u64,

    /// Provider's stake offer (may exceed minimum)
    pub stake_amount: u64,

    /// Provider's hardware capabilities
    pub capabilities: ProviderCapabilities,

    /// Quote expiration (block height)
    pub valid_until: BlockHeight,

    /// Provider's signature over the quote
    pub signature: crate::crypto::Signature,
}

/// Provider's hardware and software capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    /// Available GPU memory in GB
    pub gpu_memory_gb: u32,

    /// GPU model (e.g., "A100", "H100")
    pub gpu_model: String,

    /// Number of GPUs available
    pub gpu_count: u32,

    /// Whether provider has the model weights cached
    pub has_model_cached: bool,

    /// Network bandwidth in Mbps
    pub bandwidth_mbps: u32,
}

/// The final agreement between requestor and provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobAgreement {
    /// The original job specification
    pub job_spec: JobSpec,

    /// The selected quote
    pub selected_quote: ProviderQuote,

    /// Requestor's signature accepting the quote
    pub requestor_signature: crate::crypto::Signature,

    /// Agreement timestamp (block height)
    pub agreed_at_block: BlockHeight,
}

impl JobAgreement {
    /// Compute the hash of this agreement for on-chain reference
    pub fn hash(&self) -> Hash {
        let bytes = bincode::serialize(self).unwrap();
        Hash::compute(&bytes)
    }

    /// Check if this is a streaming/interactive job
    pub fn is_interactive(&self) -> bool {
        self.job_spec.requirements.is_streaming
    }
}

/// Result of off-chain job matching
#[derive(Debug, Clone)]
pub enum MatchingResult {
    /// Found a suitable provider
    Matched(Box<JobAgreement>),

    /// No providers met requirements
    NoMatch { reason: String },

    /// Matching timed out
    Timeout,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_spec_creation() {
        let spec = JobSpec {
            catgrad_graph_hash: Hash::compute(b"llama-3.2-graph"),
            input_hashes: vec![Hash::compute(b"weights"), Hash::compute(b"prompt")],
            requirements: JobRequirements {
                min_gpu_memory: Some(80),  // A100 memory
                max_latency_ms: Some(100), // Interactive latency
                is_streaming: true,
                estimated_flops: Some(1_000_000_000_000), // 1 TFLOP
            },
            security_params: SecurityParams {
                min_stake_ratio: 0.5, // Lower stake for trusted provider
                challenge_period: 50,
                allow_early_finalization: true,
            },
            max_price: 1000,
            nonce: 12345,
        };

        assert!(spec.requirements.is_streaming);
        assert_eq!(spec.security_params.min_stake_ratio, 0.5);
    }

    #[test]
    fn test_provider_quote() {
        let quote = ProviderQuote {
            job_spec_hash: Hash::compute(b"job123"),
            provider: Pubkey::test(1),
            price: 800,
            estimated_latency_ms: 50,
            stake_amount: 400,
            capabilities: ProviderCapabilities {
                gpu_memory_gb: 80,
                gpu_model: "A100".to_string(),
                gpu_count: 1,
                has_model_cached: true,
                bandwidth_mbps: 10000,
            },
            valid_until: 1000,
            signature: crate::crypto::Signature::dummy(),
        };

        assert!(quote.capabilities.has_model_cached);
        assert_eq!(quote.price, 800);
    }
}
