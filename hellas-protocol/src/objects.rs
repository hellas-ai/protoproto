//! # Core State Objects
//!
//! This module defines the objects that make up the blockchain state. The Hellas
//! protocol uses an object-centric model where:
//!
//! - All state is organized as discrete objects with unique IDs
//! - Objects have explicit owners who can authorize changes
//! - Objects are versioned to prevent double-spending
//! - Objects are never modified in-place; transactions consume old versions and create new ones
//!
//! This design enables massive parallelism: transactions touching different objects
//! can execute concurrently without coordination.

use crate::types::{Amount, BlockHeight, Hash, ObjectId, Pubkey, Version};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Metadata wrapper for all objects in the system.
///
/// This struct contains the "system" information about an object that the
/// protocol engine needs to track, separate from the object's actual data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectMetadata {
    /// Unique identifier for this object
    pub id: ObjectId,

    /// Current version number (increments on each change)
    pub version: Version,

    /// Set of public keys authorized to sign transactions for this object
    pub owner_set: HashSet<Pubkey>,

    /// The actual object data
    pub object: Object,
}

impl ObjectMetadata {
    /// Check if a given pubkey is authorized to modify this object
    pub fn is_authorized(&self, pubkey: &Pubkey) -> bool {
        self.owner_set.contains(pubkey)
    }

    /// Create a new metadata wrapper for an object
    pub fn new(id: ObjectId, owner: Pubkey, object: Object) -> Self {
        let mut owner_set = HashSet::new();
        owner_set.insert(owner);
        Self {
            id,
            version: 0,
            owner_set,
            object,
        }
    }

    /// Add an owner to the object
    pub fn add_owner(&mut self, owner: Pubkey) {
        self.owner_set.insert(owner);
    }
}

/// The enum of all possible object types in the system.
///
/// By having a fixed set of object types (rather than arbitrary smart contracts),
/// we can optimize the execution engine and provide stronger safety guarantees.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Object {
    /// A user account with balance and bounded counter state
    Account(HellasAccount),

    /// An escrow for a compute job
    JobEscrow(JobEscrow),

    /// A collective object that can be accessed by multiple owners
    CollectiveObject(CollectiveObject),

    /// A collective bounded counter accessible by multiple owners
    CollectiveBoundedCounter(CollectiveBoundedCounter),
}

impl Object {
    /// Try to get a reference to the inner HellasAccount
    pub fn as_account(&self) -> Option<&HellasAccount> {
        match self {
            Object::Account(account) => Some(account),
            _ => None,
        }
    }

    /// Try to get a mutable reference to the inner HellasAccount
    pub fn as_mut_account(&mut self) -> Option<&mut HellasAccount> {
        match self {
            Object::Account(account) => Some(account),
            _ => None,
        }
    }

    /// Try to get a reference to the inner JobEscrow
    pub fn as_job_escrow(&self) -> Option<&JobEscrow> {
        match self {
            Object::JobEscrow(escrow) => Some(escrow),
            _ => None,
        }
    }

    /// Try to get a mutable reference to the inner JobEscrow
    pub fn as_mut_job_escrow(&mut self) -> Option<&mut JobEscrow> {
        match self {
            Object::JobEscrow(escrow) => Some(escrow),
            _ => None,
        }
    }
}

/// # HellasAccount: User Account with Bounded Counter
///
/// This is the fundamental object for holding and transacting funds. It implements
/// the Bounded Counter pattern from Stingray, which enables massive concurrency:
///
/// ## How Bounded Counters Work
///
/// Instead of a simple balance that creates contention, the account distributes
/// "spending budgets" to each validator. A validator can approve transactions
/// that spend from its budget without coordinating with other validators.
///
/// Periodically, the account owner issues a `ResetBudget` transaction that:
/// 1. Collects all the spent amounts from validator budgets
/// 2. Updates the main balance
/// 3. Redistributes new budgets based on the updated balance
///
/// This allows many concurrent transactions while preventing double-spending.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HellasAccount {
    /// Total balance of HELL tokens in this account
    pub balance: Amount,

    /// Nonce for replay protection (increments with each transaction)
    pub nonce: u64,

    /// Version when budgets were last reset
    pub last_budget_reset_version: Version,

    /// Per-validator spending budgets
    /// Each validator can spend up to their budget without coordination
    pub validator_budgets: HashMap<Pubkey, Amount>,

    /// Maximum budget any single validator can have
    /// Computed as: (balance * eta) where eta is a safety factor
    pub max_budget_per_validator: Amount,
}

impl HellasAccount {
    /// Create a new account with an initial balance
    pub fn new(balance: Amount) -> Self {
        Self {
            balance,
            nonce: 0,
            last_budget_reset_version: 0,
            validator_budgets: HashMap::new(),
            max_budget_per_validator: Amount::ZERO,
        }
    }

    /// Initialize validator budgets based on current balance.
    ///
    /// This uses the formula from Stingray: each validator gets at most
    /// eta * balance, where eta = (f+1)/(2f+1) and f is the number of
    /// Byzantine validators the system can tolerate.
    pub fn initialize_budgets(&mut self, validators: &[Pubkey], f: usize) {
        let n = validators.len();
        assert!(n > 3 * f, "Need at least 3f+1 validators");

        // Stingray formula: eta = (f+1)/(2f+1)
        let eta_numerator = (f + 1) as u64;
        let eta_denominator = (2 * f + 1) as u64;

        // Each validator gets at most eta * balance
        self.max_budget_per_validator = self
            .balance
            .mul_rational(eta_numerator, eta_denominator)
            .expect("eta calculation should not fail");

        // Initialize all validator budgets to the maximum
        self.validator_budgets.clear();
        for validator in validators {
            self.validator_budgets
                .insert(*validator, self.max_budget_per_validator);
        }
    }

    /// Try to spend from a specific validator's budget
    pub fn try_spend_from_budget(
        &mut self,
        validator: &Pubkey,
        amount: Amount,
    ) -> Result<(), String> {
        let budget = self
            .validator_budgets
            .get_mut(validator)
            .ok_or_else(|| "Unknown validator".to_string())?;

        if *budget < amount {
            return Err("Insufficient budget".to_string());
        }

        *budget = budget.saturating_sub(amount);
        Ok(())
    }

    /// Check if there's sufficient budget for a validator
    pub fn has_budget(&self, validator: &Pubkey, amount: Amount) -> bool {
        self.validator_budgets
            .get(validator)
            .map(|budget| *budget >= amount)
            .unwrap_or(false)
    }

    /// Reset all budgets with a new balance
    pub fn reset_budgets(&mut self, new_balance: Amount, validators: &[Pubkey], f: usize) {
        self.balance = new_balance;
        self.last_budget_reset_version += 1;
        self.initialize_budgets(validators, f);
    }
}

/// Status of a job in the marketplace flow
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    /// Job posted, waiting for provider to claim
    Posted,

    /// Provider claimed job and locked bond, execution pending
    Claimed,

    /// Provider committed result hash, in challenge period
    Committed,

    /// Job completed successfully, funds distributed
    Finalized,

    /// Job aborted due to timeout or dispute
    Aborted,
}

/// # JobEscrow: On-Chain Job Contract
///
/// This object represents a compute job agreement between a requestor and provider.
/// It manages the lifecycle of a job from posting to completion, holding funds
/// in escrow to ensure proper execution.
///
/// ## State Machine
///
/// Posted -> Claimed -> Committed -> Finalized
///   |         |          |
///   +---------|----------+-----> Aborted (on timeout)
///
/// The escrow ensures:
/// - Requestor's payment is locked until job completes
/// - Provider must stake a bond to claim the job
/// - There's a challenge period before finalization
/// - Timeouts are enforced at each step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobEscrow {
    /// Hash of the off-chain JobAgreement that led to this escrow
    pub agreement_hash: Hash,

    /// Public key of the job requestor
    pub requestor: Pubkey,

    /// Public key of the provider (optional - can be selected later)
    pub provider: Option<Pubkey>,

    /// Hash of the off-chain job specification
    /// This includes the compute graph, inputs, and parameters
    pub job_spec_hash: Hash,

    /// Payment amount locked in escrow
    pub payment: Amount,

    /// Bond required from provider
    pub provider_bond_required: Amount,

    /// Bond actually locked (0 until claimed)
    pub provider_bond_locked: Amount,

    /// Current status of the job
    pub status: JobStatus,

    /// Hash of the computation result (if committed)
    pub result_hash: Option<Hash>,

    /// Block height by which provider must claim
    pub claim_deadline: BlockHeight,

    /// Block height by which provider must commit result
    pub commit_deadline: BlockHeight,

    /// Block height after which job can be finalized
    pub finalize_after: BlockHeight,

    /// Delay before finalization (blocks)
    pub finalization_delay: BlockHeight,

    /// Block when job was claimed
    pub claimed_at: Option<BlockHeight>,

    /// Block when result was committed
    pub committed_at: Option<BlockHeight>,

    /// Block when job was finalized
    pub finalized_at: Option<BlockHeight>,

    /// Block when job was aborted
    pub aborted_at: Option<BlockHeight>,
}

impl JobEscrow {
    /// Create a new job escrow (provider optional)
    pub fn new(
        agreement_hash: Hash,
        requestor: Pubkey,
        provider: Option<Pubkey>,
        job_spec_hash: Hash,
        payment: Amount,
        provider_bond_required: Amount,
        claim_deadline: BlockHeight,
    ) -> Self {
        Self {
            agreement_hash,
            requestor,
            provider,
            job_spec_hash,
            payment,
            provider_bond_required,
            provider_bond_locked: Amount::ZERO,
            status: JobStatus::Posted,
            result_hash: None,
            claim_deadline,
            commit_deadline: 0,     // Set when claimed
            finalize_after: 0,      // Set when committed
            finalization_delay: 50, // Default delay
            claimed_at: None,
            committed_at: None,
            finalized_at: None,
            aborted_at: None,
        }
    }

    /// Check if the job can be claimed
    pub fn can_claim(&self, current_height: BlockHeight) -> bool {
        self.status == JobStatus::Posted && current_height <= self.claim_deadline
    }

    /// Check if the job can be committed
    pub fn can_commit(&self, current_height: BlockHeight) -> bool {
        self.status == JobStatus::Claimed && current_height <= self.commit_deadline
    }

    /// Check if the job can be finalized
    pub fn can_finalize(&self, current_height: BlockHeight) -> bool {
        self.status == JobStatus::Committed && current_height >= self.finalize_after
    }

    /// Check if the job can be aborted
    pub fn can_abort(&self, current_height: BlockHeight) -> bool {
        match self.status {
            JobStatus::Posted => current_height > self.claim_deadline,
            JobStatus::Claimed => current_height > self.commit_deadline,
            _ => false,
        }
    }
}

/// # CollectiveObject: Multi-Owner Object with Version Merges
///
/// CollectiveObjects can be accessed by multiple owners concurrently.
/// When conflicting updates occur, they are resolved through version merges.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectiveObject {
    /// Arbitrary data stored in the object
    pub data: Vec<u8>,

    /// Hash of the data for integrity
    pub data_hash: Hash,

    /// Version history tracking merges
    pub version_info: CollectiveVersionInfo,
}

/// Version tracking for collective objects
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectiveVersionInfo {
    /// Current version
    pub version: Version,

    /// Parent versions (single for update, multiple for merge)
    pub parents: HashSet<Version>,

    /// Transactions that created this version
    pub creating_txs: Vec<Hash>,
}

impl CollectiveObject {
    /// Create a new collective object
    pub fn new(data: Vec<u8>) -> Self {
        let data_hash = Hash::compute(&data);
        Self {
            data,
            data_hash,
            version_info: CollectiveVersionInfo {
                version: 0,
                parents: HashSet::new(),
                creating_txs: Vec::new(),
            },
        }
    }

    /// Update the object data
    pub fn update_data(&mut self, new_data: Vec<u8>, tx_hash: Hash) {
        self.data = new_data;
        self.data_hash = Hash::compute(&self.data);
        self.version_info.version += 1;
        self.version_info.creating_txs = vec![tx_hash];
    }

    /// Merge multiple versions of the object
    pub fn merge_versions(
        &mut self,
        other_versions: Vec<&CollectiveObject>,
        merge_tx: Hash,
    ) -> Result<(), String> {
        // Collect all parent versions
        let mut parents = HashSet::new();
        parents.insert(self.version_info.version);
        for other in &other_versions {
            parents.insert(other.version_info.version);
        }

        // For now, use a simple merge strategy: take the data from the highest version
        let mut highest_version = self.version_info.version;
        let mut best_data = &self.data;

        for other in &other_versions {
            if other.version_info.version > highest_version {
                highest_version = other.version_info.version;
                best_data = &other.data;
            }
        }

        // Update to merged state
        self.data = best_data.clone();
        self.data_hash = Hash::compute(&self.data);
        self.version_info.version = highest_version + 1;
        self.version_info.parents = parents;
        self.version_info.creating_txs = vec![merge_tx];

        Ok(())
    }
}

/// # CollectiveBoundedCounter: Multi-Owner Bounded Counter
///
/// This extends the bounded counter concept to support multiple owners.
/// Each owner can have their own spending budgets from validators.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectiveBoundedCounter {
    /// Total balance in the counter
    pub balance: Amount,

    /// Per-owner, per-validator budgets
    /// Map from owner -> validator -> budget
    pub owner_validator_budgets: HashMap<Pubkey, HashMap<Pubkey, Amount>>,

    /// Maximum budget any validator can give to any owner
    pub max_budget_per_validator: Amount,

    /// Version tracking for merges
    pub version_info: CollectiveVersionInfo,

    /// Last reset version for each owner
    pub last_reset_versions: HashMap<Pubkey, Version>,
}

impl CollectiveBoundedCounter {
    /// Create a new collective bounded counter
    pub fn new(initial_balance: Amount, owners: Vec<Pubkey>) -> Self {
        let mut last_reset_versions = HashMap::new();
        for owner in &owners {
            last_reset_versions.insert(*owner, 0);
        }

        Self {
            balance: initial_balance,
            owner_validator_budgets: HashMap::new(),
            max_budget_per_validator: Amount::ZERO,
            version_info: CollectiveVersionInfo {
                version: 0,
                parents: HashSet::new(),
                creating_txs: Vec::new(),
            },
            last_reset_versions,
        }
    }

    /// Initialize budgets for a specific owner
    pub fn initialize_owner_budgets(&mut self, owner: &Pubkey, validators: &[Pubkey], f: usize) {
        let n = validators.len();
        assert!(n > 3 * f, "Need at least 3f+1 validators");

        // Use same formula as single-owner bounded counter
        let eta_numerator = (f + 1) as u64;
        let eta_denominator = (2 * f + 1) as u64;

        // Each validator can allocate at most eta * balance to this owner
        let owner_max_budget = self
            .balance
            .mul_rational(eta_numerator, eta_denominator)
            .expect("eta calculation should not fail");
        self.max_budget_per_validator = owner_max_budget;

        // Initialize validator budgets for this owner
        let mut validator_budgets = HashMap::new();
        for validator in validators {
            validator_budgets.insert(*validator, owner_max_budget);
        }
        self.owner_validator_budgets
            .insert(*owner, validator_budgets);
    }

    /// Try to spend from a specific owner's validator budget
    pub fn try_spend_from_owner_budget(
        &mut self,
        owner: &Pubkey,
        validator: &Pubkey,
        amount: Amount,
    ) -> Result<(), String> {
        let validator_budgets = self
            .owner_validator_budgets
            .get_mut(owner)
            .ok_or_else(|| "Unknown owner".to_string())?;

        let budget = validator_budgets
            .get_mut(validator)
            .ok_or_else(|| "Unknown validator for owner".to_string())?;

        if *budget < amount {
            return Err("Insufficient budget".to_string());
        }

        *budget = budget.saturating_sub(amount);
        Ok(())
    }

    /// Reset budgets for a specific owner
    /// Reset all budgets for an owner with a new balance
    pub fn reset_owner_budgets(
        &mut self,
        owner: &Pubkey,
        new_balance: Amount,
        validators: &[Pubkey],
        f: usize,
    ) {
        self.balance = new_balance;
        self.last_reset_versions
            .insert(*owner, self.version_info.version);
        self.initialize_owner_budgets(owner, validators, f);
    }

    /// Merge multiple versions of the counter
    pub fn merge_versions(
        &mut self,
        other_versions: Vec<&CollectiveBoundedCounter>,
        merge_tx: Hash,
    ) -> Result<(), String> {
        // Collect all parent versions
        let mut parents = HashSet::new();
        parents.insert(self.version_info.version);
        for other in &other_versions {
            parents.insert(other.version_info.version);
        }

        // Merge strategy: take minimum available balance
        let mut min_balance = self.balance;
        for other in &other_versions {
            if other.balance < min_balance {
                min_balance = other.balance;
            }
        }

        // For budgets, take the minimum available for each owner-validator pair
        let mut merged_budgets: HashMap<Pubkey, HashMap<Pubkey, Amount>> = HashMap::new();

        // Process all owners across all versions
        let mut all_owners = HashSet::new();
        all_owners.extend(self.owner_validator_budgets.keys());
        for other in &other_versions {
            all_owners.extend(other.owner_validator_budgets.keys());
        }

        for owner in all_owners {
            let mut owner_budgets = HashMap::new();

            // Get all validators for this owner
            let mut all_validators = HashSet::new();
            if let Some(budgets) = self.owner_validator_budgets.get(&owner) {
                all_validators.extend(budgets.keys());
            }
            for other in &other_versions {
                if let Some(budgets) = other.owner_validator_budgets.get(&owner) {
                    all_validators.extend(budgets.keys());
                }
            }

            // Take minimum budget for each validator
            for validator in all_validators {
                let mut min_budget: Option<Amount> = None;

                // Check this version
                if let Some(budgets) = self.owner_validator_budgets.get(&owner) {
                    if let Some(budget) = budgets.get(&validator) {
                        min_budget = Some(min_budget.map_or(*budget, |b| b.min(*budget)));
                    }
                }

                // Check other versions
                for other in &other_versions {
                    if let Some(budgets) = other.owner_validator_budgets.get(&owner) {
                        if let Some(budget) = budgets.get(&validator) {
                            min_budget = Some(min_budget.map_or(*budget, |b| b.min(*budget)));
                        }
                    }
                }

                if let Some(budget) = min_budget {
                    owner_budgets.insert(validator, budget);
                }
            }

            if !owner_budgets.is_empty() {
                merged_budgets.insert(owner, owner_budgets);
            }
        }

        // Update to merged state
        self.balance = min_balance;
        self.owner_validator_budgets = merged_budgets;
        self.version_info.version += 1;
        self.version_info.parents = parents;
        self.version_info.creating_txs = vec![merge_tx];

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bounded_counter_budgets() {
        let mut account = HellasAccount::new(Amount::from_units(1000));
        let validators = vec![
            Pubkey::test(1),
            Pubkey::test(2),
            Pubkey::test(3),
            Pubkey::test(4),
        ];

        // Initialize with f=1 (tolerate 1 Byzantine validator)
        account.initialize_budgets(&validators, 1);

        // eta = 2/3, so each validator gets 666.666...
        // Check that the value is approximately correct (within 1 unit)
        assert!(account.max_budget_per_validator >= Amount::from_units(666));
        assert!(account.max_budget_per_validator <= Amount::from_units(667));

        // Try to spend from a validator's budget
        assert!(account
            .try_spend_from_budget(&validators[0], Amount::from_units(100))
            .is_ok());
        assert_eq!(
            account.validator_budgets[&validators[0]],
            Amount::from_rational(566 * 3 + 2, 3).unwrap()
        );

        // Can't spend more than budget
        assert!(account
            .try_spend_from_budget(&validators[0], Amount::from_units(600))
            .is_err());
    }

    #[test]
    fn test_job_escrow_lifecycle() {
        let requestor = Pubkey::test(1);
        let provider = Pubkey::test(2);
        let agreement_hash = Hash::compute(b"agreement");
        let job_hash = Hash::compute(b"job spec");

        let escrow = JobEscrow::new(
            agreement_hash,
            requestor,
            Some(provider),
            job_hash,
            Amount::from_units(1000),
            Amount::from_units(50),
            1000,
        );

        assert_eq!(escrow.status, JobStatus::Posted);
        assert_eq!(escrow.provider, Some(provider));
        assert!(escrow.can_claim(500));
        assert!(!escrow.can_claim(1001));
        assert!(escrow.can_abort(1001));
    }

    #[test]
    fn test_collective_object() {
        let data1 = b"initial data".to_vec();
        let mut obj = CollectiveObject::new(data1.clone());

        assert_eq!(obj.data, data1);
        assert_eq!(obj.version_info.version, 0);

        // Update the object
        let data2 = b"updated data".to_vec();
        let tx_hash = Hash::compute(b"tx1");
        obj.update_data(data2.clone(), tx_hash);

        assert_eq!(obj.data, data2);
        assert_eq!(obj.version_info.version, 1);
        assert_eq!(obj.version_info.creating_txs, vec![tx_hash]);
    }

    #[test]
    fn test_collective_bounded_counter() {
        let owners = vec![Pubkey::test(1), Pubkey::test(2)];
        let validators = vec![
            Pubkey::test(10),
            Pubkey::test(11),
            Pubkey::test(12),
            Pubkey::test(13),
        ];

        let mut counter = CollectiveBoundedCounter::new(Amount::from_units(3000), owners.clone());

        // Initialize budgets for owner 1
        counter.initialize_owner_budgets(&owners[0], &validators, 1);

        // Each validator gets 2000 for owner 1 (3000 * 2/3)
        assert_eq!(counter.max_budget_per_validator, Amount::from_units(2000));

        // Try to spend from owner 1's budget
        assert!(counter
            .try_spend_from_owner_budget(&owners[0], &validators[0], Amount::from_units(100))
            .is_ok());
        assert_eq!(
            counter.owner_validator_budgets[&owners[0]][&validators[0]],
            Amount::from_units(1900)
        );

        // Can't spend from uninitialized owner 2
        assert!(counter
            .try_spend_from_owner_budget(&owners[1], &validators[0], Amount::from_units(100))
            .is_err());
    }
}
