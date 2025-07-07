//! # Transaction Types and Effects
//!
//! This module defines all the transaction types supported by the Hellas protocol.
//! Transactions are the only way to modify the blockchain state - they are the
//! "API" of the system.
//!
//! ## Design Principles
//!
//! 1. **Explicit inputs/outputs**: Each transaction declares which objects it reads
//!    and writes, enabling parallel execution
//! 2. **Atomic execution**: Transactions either fully succeed or fully fail
//! 3. **Authorization**: All object modifications require signatures from owners
//! 4. **Deterministic effects**: Given the same state and transaction, the result
//!    is always the same

use crate::crypto::{MultiSignatureProof, Signature};
use crate::types::{Amount, BlockHeight, Hash, ObjectId, Pubkey, TransactionDigest, Version};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Reference to an object at a specific version
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ObjectRef {
    pub object_id: ObjectId,
    pub version: Version,
}

impl ObjectRef {
    pub fn new(object_id: ObjectId, version: Version) -> Self {
        Self { object_id, version }
    }
}

/// A transaction with all necessary signatures and metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedTransaction {
    /// The primary signer (pays gas, initiates transaction)
    pub signer: Pubkey,

    /// Signature from the primary signer
    pub signature: Signature,

    /// Additional signatures for multi-party transactions
    pub additional_signatures: Option<MultiSignatureProof>,

    /// The actual transaction data
    pub transaction: Transaction,

    /// Objects this transaction will read/modify
    pub input_objects: Vec<ObjectRef>,

    /// Nonce for replay protection
    pub nonce: u64,
}

impl SignedTransaction {
    /// Create a simple single-signer transaction
    pub fn new_single_signer(
        signer: Pubkey,
        transaction: Transaction,
        input_objects: Vec<ObjectRef>,
        nonce: u64,
    ) -> Self {
        Self {
            signer,
            signature: Signature::dummy(),
            additional_signatures: None,
            transaction,
            input_objects,
            nonce,
        }
    }

    /// Create a multi-party transaction
    pub fn new_multi_party(
        signer: Pubkey,
        additional_signers: Vec<Pubkey>,
        transaction: Transaction,
        input_objects: Vec<ObjectRef>,
        nonce: u64,
    ) -> Self {
        Self {
            signer,
            signature: Signature::dummy(),
            additional_signatures: Some(MultiSignatureProof::new(additional_signers)),
            transaction,
            input_objects,
            nonce,
        }
    }

    /// Compute the digest of this transaction for signing
    pub fn digest(&self) -> TransactionDigest {
        let bytes = bincode::serialize(self).unwrap();
        Hash::compute(&bytes)
    }
}

/// All possible transaction types in the Hellas protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Transaction {
    // === Account Management ===
    /// Create a new account with initial balance
    CreateAccount { initial_balance: Amount },

    /// Reset bounded counter budgets for an account
    ResetBudget {
        /// Certificates proving all validator spends since last reset
        budget_certificates: Vec<BudgetCertificate>,
    },

    // === Marketplace Flow ===
    /// Post a new job (provider optional - marketplace flow)
    /// Provider can be pre-selected or left open for any provider to claim
    PostJob {
        /// The selected provider (optional - None for open bounty)
        provider: Option<Pubkey>,
        /// Hash of the complete JobAgreement
        agreement_hash: Hash,
        /// Job specification hash
        job_spec_hash: Hash,
        /// Payment amount
        payment: Amount,
        /// Provider bond required
        provider_bond_required: Amount,
        /// Deadline for provider to claim (accept)
        claim_deadline_delta: BlockHeight,
        /// Time allowed for computation after claim
        commit_deadline_delta: BlockHeight,
        /// Challenge period before finalization
        finalization_delay: BlockHeight,
    },

    /// Provider claims a posted job
    ClaimJob { escrow_id: ObjectId },

    /// Provider commits result hash
    CommitResult {
        escrow_id: ObjectId,
        result_hash: Hash,
    },

    /// Finalize job after challenge period
    FinalizeJob { escrow_id: ObjectId },

    /// Abort job on timeout
    AbortJob { escrow_id: ObjectId },

    // === Interactive Flow ===
    /// Direct settlement between client and provider
    SettleDirectly {
        provider: Pubkey,
        job_spec_hash: Hash,
        result_hash: Hash,
        payment: Amount,
    },
}

/// Certificate proving a validator's spending from a bounded counter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetCertificate {
    pub validator: Pubkey,
    pub total_spent: Amount,
    pub transactions: Vec<TransactionDigest>,
    /// Signature from the validator attesting to this spending
    pub validator_signature: Signature,
}

/// Represents a validator's channel state for a bounded counter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorChannelState {
    pub validator: Pubkey,
    pub account_id: ObjectId,
    pub remaining_budget: Amount,
    pub spent_amount: Amount,
    pub processed_transactions: Vec<TransactionDigest>,
}

/// The effects of executing a transaction - what changed in the state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionEffects {
    /// The transaction that was executed
    pub transaction_digest: TransactionDigest,

    /// Objects that were consumed (at specific versions)
    pub consumed_objects: Vec<ObjectRef>,

    /// Objects that were created or modified
    pub created_objects: Vec<ObjectId>,

    /// Whether the transaction succeeded
    pub success: bool,

    /// Error message if failed
    pub error: Option<String>,

    /// Gas/fee consumed
    pub gas_used: u64,
}

impl TransactionEffects {
    /// Create effects for a successful transaction
    pub fn success(
        transaction_digest: TransactionDigest,
        consumed_objects: Vec<ObjectRef>,
        created_objects: Vec<ObjectId>,
        gas_used: u64,
    ) -> Self {
        Self {
            transaction_digest,
            consumed_objects,
            created_objects,
            success: true,
            error: None,
            gas_used,
        }
    }

    /// Create effects for a failed transaction
    pub fn failure(transaction_digest: TransactionDigest, error: String, gas_used: u64) -> Self {
        Self {
            transaction_digest,
            consumed_objects: vec![],
            created_objects: vec![],
            success: false,
            error: Some(error),
            gas_used,
        }
    }
}

/// Helper functions for transactions
impl Transaction {
    /// Get the set of object IDs this transaction needs to read
    pub fn input_objects(&self) -> HashSet<ObjectId> {
        match self {
            Transaction::CreateAccount { .. } => HashSet::new(),
            Transaction::ResetBudget { .. } => {
                // Needs the account object (inferred from signer)
                HashSet::new()
            }
            Transaction::PostJob { .. } => {
                // Needs requestor's account
                HashSet::new()
            }
            Transaction::ClaimJob { escrow_id } => {
                // Needs escrow and provider's account
                let mut set = HashSet::new();
                set.insert(*escrow_id);
                set
            }
            Transaction::CommitResult { escrow_id, .. }
            | Transaction::FinalizeJob { escrow_id }
            | Transaction::AbortJob { escrow_id } => {
                let mut set = HashSet::new();
                set.insert(*escrow_id);
                set
            }
            Transaction::SettleDirectly { .. } => {
                // Needs both accounts (inferred from signer and provider)
                HashSet::new()
            }
        }
    }

    /// Check if this transaction type requires multiple signatures
    pub fn requires_multi_sig(&self) -> bool {
        matches!(self, Transaction::SettleDirectly { .. })
    }

    /// Get required additional signers for multi-sig transactions
    pub fn required_signers(&self) -> Vec<Pubkey> {
        match self {
            Transaction::SettleDirectly { provider, .. } => vec![*provider],
            _ => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_creation() {
        let signer = Pubkey::test(1);
        let provider = Pubkey::test(2);

        // Test single-signer transaction
        let tx = Transaction::CreateAccount {
            initial_balance: Amount::from_units(1000),
        };
        let signed_tx = SignedTransaction::new_single_signer(signer, tx, vec![], 0);
        assert!(signed_tx.additional_signatures.is_none());

        // Test multi-party transaction
        let tx = Transaction::SettleDirectly {
            provider,
            job_spec_hash: Hash::compute(b"job"),
            result_hash: Hash::compute(b"result"),
            payment: Amount::from_units(100),
        };
        let signed_tx = SignedTransaction::new_multi_party(signer, vec![provider], tx, vec![], 1);
        assert!(signed_tx.additional_signatures.is_some());
    }

    #[test]
    fn test_transaction_effects() {
        let tx_digest = Hash::compute(b"transaction");
        let obj_ref = ObjectRef::new(ObjectId::new([1; 32]), 0);
        let new_obj = ObjectId::new([2; 32]);

        let effects = TransactionEffects::success(tx_digest, vec![obj_ref], vec![new_obj], 100);

        assert!(effects.success);
        assert_eq!(effects.consumed_objects.len(), 1);
        assert_eq!(effects.created_objects.len(), 1);
        assert_eq!(effects.gas_used, 100);
    }
}
