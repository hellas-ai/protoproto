//! # Core Type Definitions
//!
//! This module defines the fundamental types used throughout the Hellas protocol.
//! These types are carefully designed to be:
//!
//! - **Small and fixed-size** where possible for efficient storage
//! - **Copy types** when appropriate for performance
//! - **Strongly typed** to prevent mixing up different kinds of identifiers
//!
//! ## Design Philosophy
//!
//! Rather than using raw byte arrays everywhere, we wrap them in newtype structs.
//! This provides type safety (can't accidentally use a Pubkey as an ObjectId) and
//! enables us to implement useful traits and methods on each type.

use fixed::types::U64F64;
use serde::{Deserialize, Serialize};
use std::fmt;

/// A 32-byte public key that serves as a user's identity on the network.
///
/// In a real implementation, this would be an Ed25519 or BLS public key.
/// For now, it's stubbed as a simple byte array.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct Pubkey([u8; 32]);

impl std::fmt::Debug for Pubkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Pubkey({})", self.0[0])
    }
}

impl Pubkey {
    /// Create a new pubkey from bytes
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Get the underlying bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Create a test pubkey from a single byte (for testing)
    pub fn test(id: u8) -> Self {
        let mut bytes = [0u8; 32];
        bytes[0] = id;
        Self(bytes)
    }

    /// Convert this pubkey to an ObjectId (for account lookup)
    pub fn to_object_id(&self) -> ObjectId {
        // For simplicity, use the same bytes
        // In production, this would be a proper derivation
        ObjectId(self.0)
    }
}

impl fmt::Display for Pubkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Show first 4 bytes in hex for readable logs
        write!(f, "Pubkey({:02x}{:02x}...)", self.0[0], self.0[1])
    }
}

/// A unique identifier for any object in the system.
///
/// ObjectIds are derived from the hash of the transaction that created the object
/// plus a nonce to ensure uniqueness. This makes them deterministic and
/// content-addressable.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ObjectId([u8; 32]);

impl std::fmt::Debug for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ObjectId({:02x}{:02x}...)", self.0[0], self.0[1])
    }
}

impl ObjectId {
    /// Create a new ObjectId from bytes
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Create a random ObjectId (for testing only)
    #[cfg(test)]
    pub fn new_random() -> Self {
        use rand::Rng;
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill(&mut bytes);
        Self(bytes)
    }

    /// Derive an ObjectId from transaction hash and index
    pub fn derive(tx_hash: &Hash, index: u32) -> Self {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(tx_hash.as_bytes());
        hasher.update(index.to_le_bytes());
        let result = hasher.finalize();
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&result);
        Self(bytes)
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Object({:02x}{:02x}...)", self.0[0], self.0[1])
    }
}

/// A 32-byte hash, typically SHA-256.
///
/// Used for content-addressing off-chain data like job specifications
/// and computation results. By storing only hashes on-chain, we keep
/// the blockchain lean while maintaining cryptographic guarantees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Hash([u8; 32]);

impl Hash {
    /// Create a new hash from bytes
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Get the underlying bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Compute hash of some data
    pub fn compute(data: &[u8]) -> Self {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(data);
        let result = hasher.finalize();
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&result);
        Self(bytes)
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hash({:02x}{:02x}...)", self.0[0], self.0[1])
    }
}

/// A monotonic version number for objects.
///
/// Every time an object is modified, its version increments. This enables:
/// - Optimistic concurrency control
/// - Protection against double-spending
/// - Efficient caching (version number is a cache key)
pub type Version = u64;

/// The current block height in the blockchain.
///
/// Used for time-based logic like deadlines. We use block height instead
/// of wall-clock time because:
/// - It's deterministic across all nodes
/// - It can't be manipulated by validators
/// - It provides a natural "tick" for the protocol
pub type BlockHeight = u64;

/// A unique transaction identifier
pub type TransactionDigest = Hash;

/// Result of signature verification (stubbed for now)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerificationResult(pub bool);

/// Amount type using fixed-point arithmetic for precise calculations
///
/// Uses 64.64 fixed-point representation (64 integer bits, 64 fractional bits)
/// This provides a range of ±9.2×10^18 with precision of ~5.4×10^-20
/// Perfect for cryptocurrency amounts without floating point errors
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Amount(U64F64);

impl Amount {
    /// Zero amount
    pub const ZERO: Self = Self(U64F64::ZERO);

    /// One unit
    pub const ONE: Self = Self(U64F64::ONE);

    /// Create amount from integer units
    pub fn from_units(units: u64) -> Self {
        Self(U64F64::from_num(units))
    }

    /// Create amount from rational (numerator/denominator)
    pub fn from_rational(num: u64, denom: u64) -> Option<Self> {
        if denom == 0 {
            return None;
        }
        Some(Self(U64F64::from_num(num) / U64F64::from_num(denom)))
    }

    /// Get the integer part (whole units)
    pub fn units(&self) -> u64 {
        self.0.to_num::<u64>()
    }

    /// Multiply by a rational (num/denom), useful for budget calculations
    pub fn mul_rational(&self, num: u64, denom: u64) -> Option<Self> {
        if denom == 0 {
            return None;
        }
        let result = self.0 * U64F64::from_num(num) / U64F64::from_num(denom);
        Some(Self(result))
    }

    /// Checked addition, returns None on overflow
    pub fn checked_add(&self, other: Self) -> Option<Self> {
        self.0.checked_add(other.0).map(Self)
    }

    /// Checked subtraction, returns None on underflow
    pub fn checked_sub(&self, other: Self) -> Option<Self> {
        self.0.checked_sub(other.0).map(Self)
    }

    /// Saturating subtraction (clamps to zero)
    pub fn saturating_sub(&self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }
}

impl fmt::Display for Amount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Default for Amount {
    fn default() -> Self {
        Self::ZERO
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_sizes() {
        // Ensure our types have expected sizes for efficient storage
        assert_eq!(std::mem::size_of::<Pubkey>(), 32);
        assert_eq!(std::mem::size_of::<ObjectId>(), 32);
        assert_eq!(std::mem::size_of::<Hash>(), 32);
        assert_eq!(std::mem::size_of::<Version>(), 8);
        assert_eq!(std::mem::size_of::<BlockHeight>(), 8);
    }

    #[test]
    fn test_object_id_derivation() {
        let tx_hash = Hash::compute(b"test transaction");
        let id1 = ObjectId::derive(&tx_hash, 0);
        let id2 = ObjectId::derive(&tx_hash, 1);

        // Same transaction but different indices should give different IDs
        assert_ne!(id1, id2);

        // Same inputs should give same ID (deterministic)
        let id1_again = ObjectId::derive(&tx_hash, 0);
        assert_eq!(id1, id1_again);
    }

    #[test]
    fn test_amount_arithmetic() {
        let a = Amount::from_units(100);
        let b = Amount::from_units(50);

        // Test addition
        assert_eq!(a.checked_add(b), Some(Amount::from_units(150)));

        // Test subtraction
        assert_eq!(a.checked_sub(b), Some(Amount::from_units(50)));
        assert_eq!(b.checked_sub(a), None); // Underflow

        // Test rational multiplication (for budget calculations)
        // 100 * 2/3 = 66.666...
        let budget = a.mul_rational(2, 3).unwrap();
        assert_eq!(budget.units(), 66); // Integer part

        // Test eta calculation for f=1: (f+1)/(2f+1) = 2/3
        let balance = Amount::from_units(1000);
        let eta_budget = balance.mul_rational(2, 3).unwrap();
        assert_eq!(eta_budget.units(), 666);
    }
}
