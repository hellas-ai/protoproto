//! # Collective Bounded Counter
//!
//! This module implements the collective bounded counter from Stingray,
//! which allows multiple owners to concurrently update a bounded counter.
//! The key addition is support for version merges to resolve conflicts.

use crate::types::{TransactionDigest, Version};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Version update request for bounded counters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionUpdate {
    /// Previous version this updates from
    pub prev_version: Version,

    /// All transactions included in this version
    pub prev_txs: Vec<TransactionDigest>,
}

/// Version merge request for collective bounded counters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionMerge {
    /// Set of versions to merge (for resolving conflicts)
    pub prev_versions: HashSet<Version>,

    /// Union of all transactions from merged versions
    pub prev_txs: Vec<TransactionDigest>,
}

/// Tracks the version history of a bounded counter
#[derive(Debug, Clone)]
pub struct VersionHistory {
    /// Current version
    pub version: Version,

    /// Parent version(s) - single for update, multiple for merge
    pub parents: HashSet<Version>,

    /// Transactions included in this version
    pub transactions: Vec<TransactionDigest>,

    /// History of all transactions up to this version
    pub cumulative_history: std::collections::HashSet<TransactionDigest>,
}

impl Default for VersionHistory {
    fn default() -> Self {
        Self::new()
    }
}

impl VersionHistory {
    /// Create a new version history
    pub fn new() -> Self {
        Self {
            version: 0,
            parents: HashSet::new(),
            transactions: Vec::new(),
            cumulative_history: HashSet::new(),
        }
    }

    /// Apply a version update
    pub fn apply_update(&mut self, update: &VersionUpdate) -> Result<(), String> {
        if self.version != update.prev_version {
            return Err("Version mismatch".to_string());
        }

        self.version += 1;
        self.parents.clear();
        self.parents.insert(update.prev_version);
        self.transactions = update.prev_txs.clone();

        // Add to cumulative history
        for tx in &update.prev_txs {
            self.cumulative_history.insert(*tx);
        }

        Ok(())
    }

    /// Apply a version merge
    pub fn apply_merge(&mut self, merge: &VersionMerge) -> Result<(), String> {
        if !merge.prev_versions.contains(&self.version) {
            return Err("Current version not in merge set".to_string());
        }

        self.version += 1;
        self.parents = merge.prev_versions.clone();
        self.transactions = merge.prev_txs.clone();

        // Add all transactions to cumulative history
        for tx in &merge.prev_txs {
            self.cumulative_history.insert(*tx);
        }

        Ok(())
    }
}

/// Collective bounded counter state for a validator
#[derive(Debug, Clone)]
pub struct CollectiveBoundedCounterState {
    /// Version history
    pub history: VersionHistory,

    /// Current budget for this validator
    pub budget: u64,

    /// Transactions signed by this validator
    pub signed_transactions: HashMap<TransactionDigest, u64>,
}

impl CollectiveBoundedCounterState {
    /// Create new state with initial budget
    pub fn new(initial_budget: u64) -> Self {
        Self {
            history: VersionHistory::new(),
            budget: initial_budget,
            signed_transactions: HashMap::new(),
        }
    }

    /// Process a version update request
    pub fn process_version_update(
        &mut self,
        update: &VersionUpdate,
        certified_txs: &HashMap<TransactionDigest, i64>, // tx -> delta
        eta: f64,
    ) -> Result<(), String> {
        self.history.apply_update(update)?;

        // Update budget based on certified transactions
        let mut new_budget = self.budget;
        for (tx, delta) in certified_txs {
            if update.prev_txs.contains(tx) {
                new_budget = ((new_budget as f64) + (eta * (*delta as f64))) as u64;

                // Reclaim budget for transactions we signed
                if let Some(spent) = self.signed_transactions.get(tx) {
                    if *delta < 0 {
                        new_budget += spent;
                    }
                }
            }
        }

        self.budget = new_budget;
        Ok(())
    }

    /// Process a version merge request
    pub fn process_version_merge(
        &mut self,
        merge: &VersionMerge,
        certified_txs: &HashMap<TransactionDigest, i64>,
        eta: f64,
    ) -> Result<(), String> {
        self.history.apply_merge(merge)?;

        // Calculate pending transactions (not yet in our history)
        let mut pending_txs = Vec::new();
        for tx in &merge.prev_txs {
            if !self.history.cumulative_history.contains(tx) {
                pending_txs.push(*tx);
            }
        }

        // Update budget based on pending transactions
        let mut new_budget = self.budget;
        for tx in pending_txs {
            if let Some(delta) = certified_txs.get(&tx) {
                new_budget = ((new_budget as f64) + (eta * (*delta as f64))) as u64;

                // Reclaim budget for transactions we signed
                if let Some(spent) = self.signed_transactions.get(&tx) {
                    if *delta < 0 {
                        new_budget += spent;
                    }
                }
            }
        }

        self.budget = new_budget;
        Ok(())
    }
}

/// Determines if a set of versions forms a valid chain
pub fn verify_version_chain(versions: &[Version]) -> bool {
    if versions.is_empty() {
        return true;
    }

    // Check that versions are consecutive
    for i in 1..versions.len() {
        if versions[i] != versions[i - 1] + 1 {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Hash;

    #[test]
    fn test_version_update() {
        let mut state = CollectiveBoundedCounterState::new(1000);

        let update = VersionUpdate {
            prev_version: 0,
            prev_txs: vec![Hash::compute(b"tx1"), Hash::compute(b"tx2")],
        };

        let mut certified_txs = HashMap::new();
        certified_txs.insert(Hash::compute(b"tx1"), -100);
        certified_txs.insert(Hash::compute(b"tx2"), -200);

        assert!(state
            .process_version_update(&update, &certified_txs, 0.66)
            .is_ok());
        assert_eq!(state.history.version, 1);

        // Budget should be updated: 1000 + 0.66 * (-300) = 802
        assert_eq!(state.budget, 802);
    }

    #[test]
    fn test_version_merge() {
        let mut state = CollectiveBoundedCounterState::new(1000);
        state.history.version = 1; // Simulate being on version 1

        let mut prev_versions = std::collections::HashSet::new();
        prev_versions.insert(1);
        prev_versions.insert(2);

        let merge = VersionMerge {
            prev_versions,
            prev_txs: vec![
                Hash::compute(b"tx1"),
                Hash::compute(b"tx2"),
                Hash::compute(b"tx3"),
            ],
        };

        let mut certified_txs = HashMap::new();
        certified_txs.insert(Hash::compute(b"tx1"), -100);
        certified_txs.insert(Hash::compute(b"tx2"), -100);
        certified_txs.insert(Hash::compute(b"tx3"), -100);

        assert!(state
            .process_version_merge(&merge, &certified_txs, 0.66)
            .is_ok());
        assert_eq!(state.history.version, 2);
    }
}
