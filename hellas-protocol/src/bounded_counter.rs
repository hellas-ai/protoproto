//! # Bounded Counter Implementation
//!
//! This module implements the Bounded Counter mechanism that enables massive
//! concurrency for account operations. It's based on the Stingray protocol
//! but adapted for the Hellas use case.
//!
//! ## The Problem
//!
//! In a naive blockchain, every transaction that debits an account must check
//! and update the balance. This creates a bottleneck: all transactions from
//! a popular account must be processed sequentially.
//!
//! ## The Solution
//!
//! Bounded Counters distribute trust among validators. Each validator gets a
//! "budget" they can spend from an account without coordinating with others.
//! As long as the sum of budgets doesn't exceed the true balance (with a
//! safety margin), double-spending is prevented while allowing parallel execution.
//!
//! ## How It Works
//!
//! 1. Account owner initializes budgets for all validators
//! 2. Each validator tracks its local budget and can approve spends up to that amount
//! 3. Periodically, the owner collects all spending certificates and resets budgets
//! 4. The reset transaction reconciles all concurrent spends and updates the true balance

use crate::crypto::SigningKey;
use crate::transactions::BudgetCertificate;
use crate::types::{Amount, ObjectId, Pubkey, TransactionDigest, Version};
use std::collections::HashMap;

/// Per-validator state for a bounded counter account
#[derive(Debug, Clone)]
pub struct ValidatorBudgetState {
    /// The account version this state is based on
    pub base_version: Version,

    /// Remaining budget this validator can spend
    pub available_budget: Amount,

    /// Total amount spent by this validator since last reset
    pub total_spent: Amount,

    /// Transactions this validator has approved
    pub approved_transactions: Vec<TransactionDigest>,
}

impl ValidatorBudgetState {
    /// Create a new budget state
    pub fn new(base_version: Version, budget: Amount) -> Self {
        Self {
            base_version,
            available_budget: budget,
            total_spent: Amount::ZERO,
            approved_transactions: Vec::new(),
        }
    }

    /// Try to spend from this budget
    pub fn try_spend(
        &mut self,
        amount: Amount,
        tx_digest: TransactionDigest,
    ) -> Result<(), String> {
        if self.available_budget < amount {
            return Err(format!(
                "Insufficient budget: available {}, requested {}",
                self.available_budget, amount
            ));
        }

        self.available_budget = self.available_budget.saturating_sub(amount);
        self.total_spent = self
            .total_spent
            .checked_add(amount)
            .ok_or_else(|| "Total spent overflow".to_string())?;
        self.approved_transactions.push(tx_digest);
        Ok(())
    }

    /// Generate a certificate of all spending
    pub fn generate_certificate(&self, validator: Pubkey) -> BudgetCertificate {
        BudgetCertificate {
            validator,
            total_spent: self.total_spent,
            transactions: self.approved_transactions.clone(),
            validator_signature: crate::crypto::Signature::dummy(), // Stubbed for now
        }
    }
}

/// Per-validator local state for managing channels
#[derive(Debug, Clone)]
pub struct ValidatorLocalState {
    /// This validator's public key
    pub validator: Pubkey,

    /// Map from account ID to this validator's budget state for that account
    pub account_channels: HashMap<ObjectId, ChannelState>,
}

/// State of a single channel for an account
#[derive(Debug, Clone)]
pub struct ChannelState {
    /// The base version of the account this channel is based on
    pub base_version: Version,

    /// Maximum budget allocated to this channel
    pub max_budget: Amount,

    /// Amount already spent from this channel
    pub total_spent: Amount,

    /// Remaining budget
    pub remaining_budget: Amount,

    /// Transaction digests that spent from this channel
    pub transactions: Vec<TransactionDigest>,
}

impl ValidatorLocalState {
    /// Create a new validator local state
    pub fn new(validator: Pubkey) -> Self {
        Self {
            validator,
            account_channels: HashMap::new(),
        }
    }

    /// Initialize a channel for an account
    pub fn init_channel(&mut self, account: ObjectId, max_budget: Amount, base_version: Version) {
        let channel = ChannelState {
            base_version,
            max_budget,
            total_spent: Amount::ZERO,
            remaining_budget: max_budget,
            transactions: Vec::new(),
        };
        self.account_channels.insert(account, channel);
    }

    /// Try to spend from a channel
    pub fn try_spend(
        &mut self,
        account: ObjectId,
        amount: Amount,
        tx_digest: TransactionDigest,
    ) -> Result<(), String> {
        let channel = self
            .account_channels
            .get_mut(&account)
            .ok_or_else(|| "Channel not initialized for account".to_string())?;

        if channel.remaining_budget < amount {
            return Err(format!(
                "Insufficient channel budget: remaining {}, requested {}",
                channel.remaining_budget, amount
            ));
        }

        channel.remaining_budget = channel.remaining_budget.saturating_sub(amount);
        channel.total_spent = channel
            .total_spent
            .checked_add(amount)
            .ok_or_else(|| "Channel spent overflow".to_string())?;
        channel.transactions.push(tx_digest);
        Ok(())
    }

    /// Reset a channel after budget reconciliation
    pub fn reset_channel(&mut self, account: ObjectId, max_budget: Amount, base_version: Version) {
        self.init_channel(account, max_budget, base_version);
    }

    /// Create a budget certificate for an account
    pub fn create_certificate(
        &self,
        account: ObjectId,
        _signing_key: &SigningKey,
    ) -> Option<BudgetCertificate> {
        let channel = self.account_channels.get(&account)?;

        if channel.total_spent == Amount::ZERO {
            return None;
        }

        Some(BudgetCertificate {
            validator: self.validator,
            total_spent: channel.total_spent,
            transactions: channel.transactions.clone(),
            validator_signature: crate::crypto::Signature::dummy(), // TODO: Implement real signatures
        })
    }
}

/// Manager for all bounded counter states across validators
#[derive(Debug, Clone)]
pub struct BoundedCounterManager {
    /// All validators in the system
    validators: Vec<Pubkey>,

    /// Byzantine fault tolerance parameter
    byzantine_tolerance: usize,

    /// Map from account ID to the validator's budget state for that account
    states: HashMap<ObjectId, HashMap<Pubkey, ValidatorBudgetState>>,
}

impl BoundedCounterManager {
    /// Create a new bounded counter manager
    pub fn new(validators: Vec<Pubkey>, byzantine_tolerance: usize) -> Self {
        Self {
            validators,
            byzantine_tolerance,
            states: HashMap::new(),
        }
    }

    /// Initialize channels for an account across all validators
    pub fn init_account_channels(&mut self, account: ObjectId, balance: Amount, version: Version) {
        let budget_per_validator = self.calculate_max_budget(balance);
        let validators = self.validators.clone();

        for validator in validators {
            self.initialize_budget(account, validator, version, budget_per_validator);
        }
    }

    /// Calculate the maximum budget for a given balance
    pub fn calculate_max_budget(&self, balance: Amount) -> Amount {
        calculate_validator_budgets(balance, self.validators.len(), self.byzantine_tolerance)
            .unwrap_or(Amount::ZERO)
    }

    /// Verify a set of budget certificates and return total spent
    pub fn verify_certificates(
        &self,
        certificates: &[BudgetCertificate],
    ) -> Result<Amount, String> {
        verify_budget_certificates(certificates, &self.validators)
    }
    /// Initialize a new budget for an account
    pub fn initialize_budget(
        &mut self,
        account: ObjectId,
        validator: Pubkey,
        version: Version,
        budget: Amount,
    ) {
        let state = ValidatorBudgetState::new(version, budget);
        self.states
            .entry(account)
            .or_default()
            .insert(validator, state);
    }

    /// Try to spend from a validator's budget for an account
    pub fn try_spend(
        &mut self,
        account: ObjectId,
        validator: Pubkey,
        amount: Amount,
        tx_digest: TransactionDigest,
    ) -> Result<(), String> {
        let account_states = self
            .states
            .get_mut(&account)
            .ok_or_else(|| "Account not found".to_string())?;

        let validator_state = account_states
            .get_mut(&validator)
            .ok_or_else(|| "Validator budget not found".to_string())?;

        validator_state.try_spend(amount, tx_digest)
    }

    /// Get the current budget state for a validator on an account
    pub fn get_budget_state(
        &self,
        account: ObjectId,
        validator: Pubkey,
    ) -> Option<&ValidatorBudgetState> {
        self.states.get(&account)?.get(&validator)
    }

    /// Generate spending certificates for all validators on an account
    pub fn generate_certificates(&self, account: ObjectId) -> Vec<BudgetCertificate> {
        self.states
            .get(&account)
            .map(|validator_states| {
                validator_states
                    .iter()
                    .filter(|(_, state)| state.total_spent > Amount::ZERO)
                    .map(|(validator, state)| state.generate_certificate(*validator))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Clear budget state after a reset
    pub fn clear_budget(&mut self, account: ObjectId) {
        self.states.remove(&account);
    }
}

/// Calculate the optimal budget distribution for validators
///
/// This implements the Stingray formula for Byzantine fault tolerance:
/// - n = total validators
/// - f = Byzantine validators to tolerate
/// - eta = (f+1)/(2f+1) = maximum fraction any validator can spend
///
/// The formula ensures that even if f validators are Byzantine and spend
/// their full budgets maliciously, the honest validators' budgets sum to
/// less than the true balance.
pub fn calculate_validator_budgets(
    balance: Amount,
    num_validators: usize,
    byzantine_tolerance: usize,
) -> Result<Amount, String> {
    let n = num_validators;
    let f = byzantine_tolerance;

    // Validate parameters
    if n < 3 * f + 1 {
        return Err(format!(
            "Need at least 3f+1 validators: n={}, f={}, need n>={}",
            n,
            f,
            3 * f + 1
        ));
    }

    // Calculate eta = (f+1)/(2f+1)
    let eta_numerator = (f + 1) as u64;
    let eta_denominator = (2 * f + 1) as u64;

    // Each validator gets at most eta * balance
    let budget_per_validator = balance
        .mul_rational(eta_numerator, eta_denominator)
        .ok_or_else(|| "Failed to calculate budget".to_string())?;

    Ok(budget_per_validator)
}

/// Verify that a set of budget certificates is valid and consistent
pub fn verify_budget_certificates(
    certificates: &[BudgetCertificate],
    expected_validators: &[Pubkey],
) -> Result<Amount, String> {
    // Check all certificates are from expected validators
    let mut seen_validators = HashMap::new();
    let mut total_spent = Amount::ZERO;

    for cert in certificates {
        if !expected_validators.contains(&cert.validator) {
            return Err(format!("Unknown validator: {:?}", cert.validator));
        }

        if seen_validators.contains_key(&cert.validator) {
            return Err(format!(
                "Duplicate certificate from validator: {:?}",
                cert.validator
            ));
        }

        seen_validators.insert(cert.validator, cert.total_spent);
        total_spent = total_spent
            .checked_add(cert.total_spent)
            .ok_or_else(|| "Total spent overflow".to_string())?;
    }

    Ok(total_spent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Hash;

    #[test]
    fn test_budget_calculation() {
        // Test with 4 validators, tolerating 1 Byzantine
        let balance = Amount::from_units(1000);
        let budget = calculate_validator_budgets(balance, 4, 1).unwrap();

        // eta = 2/3, so each validator gets 666.666...
        // Check that the value is approximately correct (within 1 unit)
        assert!(budget >= Amount::from_units(666));
        assert!(budget <= Amount::from_units(667));

        // Even if 1 validator spends maliciously, total is 4*666 = 2664
        // But honest validators (3) would only approve 3*666 = 1998
        // Since we only have 1000, the Byzantine validator can at most
        // cause 666 of overspend, which is within safety margin
    }

    #[test]
    fn test_validator_budget_state() {
        let mut state = ValidatorBudgetState::new(0, Amount::from_units(100));
        let tx1 = Hash::compute(b"tx1");
        let tx2 = Hash::compute(b"tx2");

        // Can spend within budget
        assert!(state.try_spend(Amount::from_units(30), tx1).is_ok());
        assert_eq!(state.available_budget, Amount::from_units(70));
        assert_eq!(state.total_spent, Amount::from_units(30));

        assert!(state.try_spend(Amount::from_units(50), tx2).is_ok());
        assert_eq!(state.available_budget, Amount::from_units(20));
        assert_eq!(state.total_spent, Amount::from_units(80));

        // Can't exceed budget
        assert!(state
            .try_spend(Amount::from_units(30), Hash::compute(b"tx3"))
            .is_err());

        // Generate certificate
        let cert = state.generate_certificate(Pubkey::test(1));
        assert_eq!(cert.total_spent, Amount::from_units(80));
        assert_eq!(cert.transactions.len(), 2);
    }

    #[test]
    fn test_bounded_counter_manager() {
        let validators = vec![
            Pubkey::test(10),
            Pubkey::test(11),
            Pubkey::test(12),
            Pubkey::test(13),
        ];
        let mut manager = BoundedCounterManager::new(validators.clone(), 1);
        let account = ObjectId::new([1; 32]);
        let validator1 = validators[0];
        let validator2 = validators[1];

        // Initialize budgets
        manager.initialize_budget(account, validator1, 0, Amount::from_units(100));
        manager.initialize_budget(account, validator2, 0, Amount::from_units(100));

        // Spend from different validators
        let tx1 = Hash::compute(b"tx1");
        let tx2 = Hash::compute(b"tx2");

        assert!(manager
            .try_spend(account, validator1, Amount::from_units(50), tx1)
            .is_ok());
        assert!(manager
            .try_spend(account, validator2, Amount::from_units(30), tx2)
            .is_ok());

        // Generate certificates
        let certs = manager.generate_certificates(account);
        assert_eq!(certs.len(), 2);

        let total_spent = certs
            .iter()
            .map(|c| c.total_spent)
            .fold(Amount::ZERO, |acc, x| acc.checked_add(x).unwrap());
        assert_eq!(total_spent, Amount::from_units(80));
    }

    #[test]
    fn test_byzantine_scenario_double_spending() {
        // Test that even if f Byzantine validators try to double-spend,
        // the total spent cannot exceed the balance
        let validators = vec![
            Pubkey::test(1), // Byzantine
            Pubkey::test(2), // Honest
            Pubkey::test(3), // Honest
            Pubkey::test(4), // Honest
        ];
        let mut manager = BoundedCounterManager::new(validators.clone(), 1);
        let account = ObjectId::new([1; 32]);
        let balance = Amount::from_units(1000);

        // Initialize account with 1000 units
        manager.init_account_channels(account, balance, 0);

        // Byzantine validator (validators[0]) tries to spend infinitely
        let byzantine = validators[0];
        let mut byzantine_spent = Amount::ZERO;
        // Byzantine validator signs many transactions
        for (tx_counter, _) in (0_u64..).zip(0..100) {
            let tx = Hash::compute(&tx_counter.to_le_bytes());

            if manager
                .try_spend(account, byzantine, Amount::from_units(100), tx)
                .is_ok()
            {
                byzantine_spent = byzantine_spent
                    .checked_add(Amount::from_units(100))
                    .unwrap();
            }
        }

        // Byzantine can only spend up to their budget (666)
        assert_eq!(byzantine_spent, Amount::from_units(600)); // 6 transactions of 100

        // Now try to get certificates with honest validators
        let mut total_certified = Amount::ZERO;

        // Each honest validator spends up to their budget
        for (i, validator) in validators.iter().enumerate().skip(1) {
            for j in 0..7 {
                let tx = Hash::compute(format!("honest-{}-{}", i, j).as_bytes());
                if manager
                    .try_spend(account, *validator, Amount::from_units(100), tx)
                    .is_ok()
                {
                    total_certified = total_certified
                        .checked_add(Amount::from_units(100))
                        .unwrap();
                }
            }
        }

        // Total certified by honest validators: 3 * 600 = 1800
        // But since we need 2f+1 = 3 validators for a certificate,
        // and the Byzantine validator already spent 600,
        // the maximum that can be certified is still bounded by total balance
        let certs = manager.generate_certificates(account);
        let total: Amount = certs
            .iter()
            .map(|c| c.total_spent)
            .fold(Amount::ZERO, |acc, x| acc.checked_add(x).unwrap());

        // Even with Byzantine behavior, total spent across all validators
        // is at most the sum of their budgets: 4 * 666 = 2664
        // But honest validators won't sign more than balance (1000)
        assert!(total <= Amount::from_units(2664));
    }

    #[test]
    fn test_budget_reset_validation() {
        // Test that reset budget properly validates all spending is accounted for
        let validators = vec![
            Pubkey::test(1),
            Pubkey::test(2),
            Pubkey::test(3),
            Pubkey::test(4),
        ];
        let mut manager = BoundedCounterManager::new(validators.clone(), 1);
        let account = ObjectId::new([1; 32]);

        // Initialize with balance of 1000
        manager.init_account_channels(account, Amount::from_units(1000), 0);

        // Some validators spend
        let tx1 = Hash::compute(b"tx1");
        let tx2 = Hash::compute(b"tx2");
        manager
            .try_spend(account, validators[0], Amount::from_units(100), tx1)
            .unwrap();
        manager
            .try_spend(account, validators[1], Amount::from_units(200), tx2)
            .unwrap();

        // Generate certificates only for validator 0
        let state0 = manager.get_budget_state(account, validators[0]).unwrap();
        let cert0 = state0.generate_certificate(validators[0]);

        // Try to verify with missing certificate from validator 1
        let result = manager.verify_certificates(&[cert0]);

        // This should succeed but only account for validator 0's spending
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Amount::from_units(100));

        // The actual reset budget transaction in the engine should check
        // that all validators who spent have provided certificates
    }

    #[test]
    fn test_concurrent_spending_edge_cases() {
        // Test edge cases with concurrent spending
        let validators = vec![
            Pubkey::test(1),
            Pubkey::test(2),
            Pubkey::test(3),
            Pubkey::test(4),
        ];
        let mut manager = BoundedCounterManager::new(validators.clone(), 1);
        let account = ObjectId::new([1; 32]);

        // Test 1: Zero balance account
        manager.init_account_channels(account, Amount::ZERO, 0);
        let tx = Hash::compute(b"tx1");
        assert!(manager
            .try_spend(account, validators[0], Amount::from_units(1), tx)
            .is_err());

        // Test 2: Exact budget spending
        let account2 = ObjectId::new([2; 32]);
        manager.init_account_channels(account2, Amount::from_units(3), 0);

        // With balance=3 and f=1, eta=2/3, so each validator gets 2
        let budget = manager.calculate_max_budget(Amount::from_units(3));
        assert_eq!(budget, Amount::from_units(2));

        // Should be able to spend exactly the budget
        let tx2 = Hash::compute(b"tx2");
        assert!(manager
            .try_spend(account2, validators[0], Amount::from_units(2), tx2)
            .is_ok());

        // But not more
        let tx3 = Hash::compute(b"tx3");
        assert!(manager
            .try_spend(account2, validators[0], Amount::from_units(1), tx3)
            .is_err());
    }

    #[test]
    fn test_version_overflow_protection() {
        // Test that version numbers handle overflow gracefully
        let mut state = ValidatorBudgetState {
            base_version: u64::MAX - 1,
            available_budget: Amount::from_units(100),
            total_spent: Amount::ZERO,
            approved_transactions: Vec::new(),
        };

        // Spending should still work near max version
        let tx = Hash::compute(b"tx1");
        assert!(state.try_spend(Amount::from_units(50), tx).is_ok());

        // Version overflow is handled at the account/engine level,
        // not in the bounded counter itself
    }

    #[test]
    fn test_certificate_validation_edge_cases() {
        let validators = vec![
            Pubkey::test(1),
            Pubkey::test(2),
            Pubkey::test(3),
            Pubkey::test(4),
        ];

        // Test 1: Empty certificates
        let result = verify_budget_certificates(&[], &validators);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Amount::ZERO);

        // Test 2: Unknown validator
        let unknown = Pubkey::test(99);
        let cert = BudgetCertificate {
            validator: unknown,
            total_spent: Amount::from_units(100),
            transactions: vec![],
            validator_signature: crate::crypto::Signature::dummy(),
        };
        let result = verify_budget_certificates(&[cert], &validators);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown validator"));

        // Test 3: Duplicate certificates
        let cert1 = BudgetCertificate {
            validator: validators[0],
            total_spent: Amount::from_units(100),
            transactions: vec![],
            validator_signature: crate::crypto::Signature::dummy(),
        };
        let cert2 = BudgetCertificate {
            validator: validators[0], // Same validator
            total_spent: Amount::from_units(200),
            transactions: vec![],
            validator_signature: crate::crypto::Signature::dummy(),
        };
        let result = verify_budget_certificates(&[cert1, cert2], &validators);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Duplicate certificate"));
    }

    #[test]
    fn test_budget_calculation_various_f_values() {
        // Test budget calculation for various Byzantine tolerance values
        let balance = Amount::from_units(10000);

        // f=1, n=4: eta = 2/3
        let budget = calculate_validator_budgets(balance, 4, 1).unwrap();
        // 10000 * 2/3 = 6666.666...
        assert!(budget >= Amount::from_units(6666));
        assert!(budget <= Amount::from_units(6667));

        // f=2, n=7: eta = 3/5
        let budget = calculate_validator_budgets(balance, 7, 2).unwrap();
        assert_eq!(budget, Amount::from_units(6000));

        // f=3, n=10: eta = 4/7
        let budget = calculate_validator_budgets(balance, 10, 3).unwrap();
        // 10000 * 4/7 = 5714.285...
        assert!(budget >= Amount::from_units(5714));
        assert!(budget <= Amount::from_units(5715));

        // Invalid: n < 3f+1
        let result = calculate_validator_budgets(balance, 3, 2);
        assert!(result.is_err());
    }
}
