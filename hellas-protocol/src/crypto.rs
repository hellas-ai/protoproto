//! # Cryptographic Primitives (Stubbed)
//!
//! This module provides the cryptographic primitives used by the protocol.
//! For this prototype, all crypto operations are stubbed out to focus on
//! the protocol logic and data flow.
//!
//! ## Real Implementation Notes
//!
//! In production, this module would use:
//! - **Ed25519** for signatures (fast, secure, small signatures)
//! - **SHA-256** for hashing (or BLAKE3 for better performance)
//! - **BLS signatures** for aggregation (optional, for validator signatures)
//!
//! The stub implementation allows us to:
//! 1. Test the protocol logic without crypto dependencies
//! 2. Clearly separate concerns between protocol and crypto
//! 3. Easily swap in real implementations later

use crate::types::{Hash, Pubkey, VerificationResult};
use serde::{Deserialize, Serialize};

/// A digital signature (stubbed as empty struct)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signature;

impl Signature {
    /// Create a dummy signature for testing
    pub fn dummy() -> Self {
        Self
    }
}

/// A signing key that can create signatures (stubbed)
#[derive(Debug, Clone)]
pub struct SigningKey {
    pubkey: Pubkey,
}

impl SigningKey {
    /// Generate a new random signing key
    pub fn generate() -> Self {
        #[cfg(test)]
        {
            use rand::Rng;
            let mut bytes = [0u8; 32];
            rand::thread_rng().fill(&mut bytes);
            Self {
                pubkey: Pubkey::new(bytes),
            }
        }
        #[cfg(not(test))]
        {
            Self {
                pubkey: Pubkey::default(),
            }
        }
    }

    /// Create a signing key with a specific pubkey (for testing)
    pub fn from_pubkey(pubkey: Pubkey) -> Self {
        Self { pubkey }
    }

    /// Get the corresponding public key
    pub fn verifying_key(&self) -> VerifyingKey {
        VerifyingKey {
            pubkey: self.pubkey,
        }
    }

    /// Sign a message (stubbed - always returns dummy signature)
    /// TODO: Implement real Ed25519 signature generation
    pub fn sign(&self, _message: &[u8]) -> Signature {
        Signature::dummy()
    }
}

/// A verifying key that can verify signatures (stubbed)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifyingKey {
    pubkey: Pubkey,
}

impl VerifyingKey {
    /// Get the underlying public key
    pub fn to_pubkey(&self) -> Pubkey {
        self.pubkey
    }

    /// Verify a signature (stubbed - always returns true)
    /// TODO: Implement real Ed25519 signature verification
    pub fn verify(&self, _message: &[u8], _signature: &Signature) -> VerificationResult {
        VerificationResult(true)
    }
}

/// Multi-signature proof for transactions requiring multiple parties
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiSignatureProof {
    /// The public keys of all signers
    pub signers: Vec<Pubkey>,
    /// Their corresponding signatures (all stubbed)
    pub signatures: Vec<Signature>,
}

impl MultiSignatureProof {
    /// Create a new multi-sig proof
    pub fn new(signers: Vec<Pubkey>) -> Self {
        let signatures = vec![Signature::dummy(); signers.len()];
        Self {
            signers,
            signatures,
        }
    }

    /// Verify all signatures (stubbed - always returns true)
    /// TODO: Implement real multi-signature verification
    pub fn verify(&self, _message: &[u8]) -> VerificationResult {
        VerificationResult(true)
    }

    /// Check if a specific pubkey has signed
    pub fn has_signed(&self, pubkey: &Pubkey) -> bool {
        self.signers.contains(pubkey)
    }
}

/// Compute the digest of a transaction for signing
pub fn transaction_digest(tx_bytes: &[u8]) -> Hash {
    Hash::compute(tx_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signing_flow() {
        let key = SigningKey::generate();
        let message = b"test message";
        let signature = key.sign(message);

        let verifying_key = key.verifying_key();
        let result = verifying_key.verify(message, &signature);
        assert!(result.0); // Should always pass with stubs
    }

    #[test]
    fn test_multi_sig() {
        let key1 = SigningKey::generate();
        let key2 = SigningKey::generate();

        let proof = MultiSignatureProof::new(vec![
            key1.verifying_key().to_pubkey(),
            key2.verifying_key().to_pubkey(),
        ]);

        assert!(proof.has_signed(&key1.verifying_key().to_pubkey()));
        assert!(proof.has_signed(&key2.verifying_key().to_pubkey()));
        assert!(proof.verify(b"test").0);
    }
}
