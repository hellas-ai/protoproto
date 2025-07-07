//! # FastUnlock Protocol
//!
//! This module implements the FastUnlock protocol from Stingray, which allows
//! quick resolution of conflicting transactions that would otherwise lock objects
//! for an entire epoch (day).
//!
//! ## How it Works
//!
//! 1. User creates an UnlockRequest for locked objects
//! 2. Validators sign UnlockVotes (with any existing certificates)
//! 3. User assembles votes into an UnlockCertificate
//! 4. Certificate goes through consensus for ordering
//! 5. Validators execute the unlock (either existing tx or no-op)

use crate::crypto::Signature;
use crate::transactions::{SignedTransaction, Transaction};
use crate::types::{ObjectId, Pubkey, TransactionDigest, Version};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Key identifying a specific version of an object
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ObjectKey {
    pub object_id: ObjectId,
    pub version: Version,
}

/// Request to unlock one or more objects
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnlockRequest {
    /// Objects to unlock
    pub object_keys: Vec<ObjectKey>,

    /// Transaction to execute if unlock succeeds (optional)
    pub new_transaction: Option<Transaction>,

    /// Proof of authorization (transaction signed by owners)
    pub auth: SignedTransaction,
}

/// Vote from a validator on an unlock request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnlockVote {
    /// The request being voted on
    pub request: UnlockRequest,

    /// Any existing certificates for these objects
    pub existing_certificates: Vec<TransactionCertificate>,

    /// Validator's signature
    pub validator: Pubkey,
    pub signature: Signature,
}

/// Certificate proving 2f+1 validators agree on unlock
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnlockCertificate {
    /// The original request
    pub request: UnlockRequest,

    /// Union of all certificates from votes
    pub certificates: Vec<TransactionCertificate>,

    /// Validator votes (at least 2f+1)
    pub votes: HashMap<Pubkey, UnlockVote>,
}

/// Certificate for a transaction (simplified)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionCertificate {
    pub transaction: SignedTransaction,
    pub signatures: HashMap<Pubkey, Signature>,
}

/// Database tracking object lock status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockStatus {
    /// Object is available for fast path
    Available,
    /// Object is locked, awaiting unlock
    Locked(Option<TransactionDigest>),
    /// Unlock in progress
    Unlocking,
    /// Unlock confirmed through consensus
    Confirmed,
}

/// Per-validator state for FastUnlock
#[derive(Debug)]
pub struct FastUnlockState {
    /// Lock status for each object version
    pub lock_db: HashMap<ObjectKey, LockStatus>,

    /// Unlock status for each object version
    pub unlock_db: HashMap<ObjectKey, UnlockStatus>,

    /// Certificates we've seen for objects
    pub cert_db: HashMap<ObjectKey, TransactionCertificate>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnlockStatus {
    None,
    Unlocked,
    Confirmed,
}

impl Default for FastUnlockState {
    fn default() -> Self {
        Self::new()
    }
}

impl FastUnlockState {
    pub fn new() -> Self {
        Self {
            lock_db: HashMap::new(),
            unlock_db: HashMap::new(),
            cert_db: HashMap::new(),
        }
    }

    /// Process an unlock request from a user
    pub fn process_unlock_request(
        &mut self,
        request: &UnlockRequest,
        validator: Pubkey,
    ) -> Result<UnlockVote, String> {
        // Check authorization
        if !self.verify_authorization(request)? {
            return Err("Invalid authorization".to_string());
        }

        // Collect any existing certificates
        let mut existing_certs = Vec::new();
        for key in &request.object_keys {
            if let Some(cert) = self.cert_db.get(key) {
                existing_certs.push(cert.clone());
            }
        }

        // Mark objects as being unlocked
        for key in &request.object_keys {
            self.unlock_db.insert(*key, UnlockStatus::Unlocked);
        }

        // Create vote
        Ok(UnlockVote {
            request: request.clone(),
            existing_certificates: existing_certs,
            validator,
            signature: Signature::dummy(),
        })
    }

    /// Process an unlock certificate after consensus
    pub fn process_unlock_certificate(
        &mut self,
        cert: &UnlockCertificate,
    ) -> Result<Vec<TransactionEffects>, String> {
        let mut effects = Vec::new();

        // Check if any objects are already confirmed
        for key in &cert.request.object_keys {
            if self.unlock_db.get(key) == Some(&UnlockStatus::Confirmed) {
                return Err("Object already confirmed".to_string());
            }
        }

        // If there are existing certificates, execute them
        if !cert.certificates.is_empty() {
            for _tx_cert in &cert.certificates {
                // Execute the transaction
                // (In real implementation, this would update state)
                effects.push(TransactionEffects::dummy_success());
            }
        } else {
            // No existing certificates - execute no-op or new transaction
            if let Some(_new_tx) = &cert.request.new_transaction {
                // Execute new transaction
                effects.push(TransactionEffects::dummy_success());
            } else {
                // Execute no-op (increment version)
                effects.push(TransactionEffects::dummy_no_op());
            }
        }

        // Mark all objects as confirmed
        for key in &cert.request.object_keys {
            self.unlock_db.insert(*key, UnlockStatus::Confirmed);
        }

        Ok(effects)
    }

    /// Verify that the unlock request is properly authorized
    fn verify_authorization(&self, request: &UnlockRequest) -> Result<bool, String> {
        // Check that auth transaction references all objects
        let auth_objects: HashSet<ObjectId> = request
            .auth
            .input_objects
            .iter()
            .map(|r| r.object_id)
            .collect();

        for key in &request.object_keys {
            if !auth_objects.contains(&key.object_id) {
                return Ok(false);
            }
        }

        // Check signatures (stubbed)
        Ok(true)
    }
}

/// Dummy transaction effects for testing
pub struct TransactionEffects;

impl TransactionEffects {
    fn dummy_success() -> Self {
        Self
    }

    fn dummy_no_op() -> Self {
        Self
    }
}

/// Check if a quorum of votes forms a valid unlock certificate
pub fn verify_unlock_certificate(
    cert: &UnlockCertificate,
    validators: &[Pubkey],
    f: usize,
) -> Result<(), String> {
    // Need at least 2f+1 votes
    if cert.votes.len() < 2 * f + 1 {
        return Err("Insufficient votes".to_string());
    }

    // All votes must be for the same request
    for vote in cert.votes.values() {
        if vote.request.object_keys != cert.request.object_keys {
            return Err("Inconsistent votes".to_string());
        }
    }

    // All voters must be validators
    for voter in cert.votes.keys() {
        if !validators.contains(voter) {
            return Err("Unknown validator".to_string());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transactions::ObjectRef;

    #[test]
    fn test_unlock_request_creation() {
        let object_key = ObjectKey {
            object_id: ObjectId::new([1; 32]),
            version: 0,
        };

        let auth = SignedTransaction::new_single_signer(
            Pubkey::test(1),
            Transaction::CreateAccount {
                initial_balance: crate::types::Amount::from_units(100),
            },
            vec![ObjectRef::new(object_key.object_id, object_key.version)],
            0,
        );

        let request = UnlockRequest {
            object_keys: vec![object_key],
            new_transaction: None,
            auth,
        };

        assert_eq!(request.object_keys.len(), 1);
    }

    #[test]
    fn test_unlock_vote_processing() {
        let mut state = FastUnlockState::new();
        let validator = Pubkey::test(1);

        let object_key = ObjectKey {
            object_id: ObjectId::new([1; 32]),
            version: 0,
        };

        let auth = SignedTransaction::new_single_signer(
            Pubkey::test(2),
            Transaction::CreateAccount {
                initial_balance: crate::types::Amount::from_units(100),
            },
            vec![ObjectRef::new(object_key.object_id, object_key.version)],
            0,
        );

        let request = UnlockRequest {
            object_keys: vec![object_key],
            new_transaction: None,
            auth,
        };

        let vote = state.process_unlock_request(&request, validator).unwrap();
        assert_eq!(vote.validator, validator);
        assert_eq!(state.unlock_db[&object_key], UnlockStatus::Unlocked);
    }
}
