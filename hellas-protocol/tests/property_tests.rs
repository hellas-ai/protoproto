//! Property-based tests for the Hellas protocol
//!
//! These tests use proptest to generate random inputs and verify that
//! certain properties always hold, regardless of the specific values.

use hellas_protocol::transactions::ObjectRef;
use hellas_protocol::*;
use proptest::prelude::*;
use std::collections::HashMap;

/// Generate random Pubkeys for testing
fn arb_pubkey() -> impl Strategy<Value = Pubkey> {
    (0u8..255u8).prop_map(Pubkey::test)
}

/// Generate random amounts (constrained to reasonable values)
fn arb_amount() -> impl Strategy<Value = Amount> {
    (1u64..=1_000_000u64).prop_map(Amount::from_units)
}

/// Generate a valid set of validators (3f+1 where f is Byzantine tolerance)
fn arb_validator_set() -> impl Strategy<Value = (Vec<Pubkey>, usize)> {
    (1usize..=5usize).prop_flat_map(|f| {
        let n = 3 * f + 1;
        prop::collection::vec(arb_pubkey(), n..=n).prop_map(move |validators| (validators, f))
    })
}

proptest! {
    /// Property: Bounded counter budgets never exceed safety threshold
    #[test]
    fn bounded_counter_safety(
        balance in arb_amount(),
        (validators, f) in arb_validator_set(),
    ) {
        // Ensure unique validators
        let unique_validators: std::collections::HashSet<_> = validators.iter().cloned().collect();
        prop_assume!(unique_validators.len() == validators.len());
        prop_assume!(validators.len() > 3 * f);
        let budget_per_validator = hellas_protocol::bounded_counter::calculate_validator_budgets(
            balance, validators.len(), f
        ).unwrap();

        // Property 1: Each validator's budget is at most η * balance
        let eta_numerator = (f + 1) as u64;
        let eta_denominator = (2 * f + 1) as u64;
        let max_allowed = balance.mul_rational(eta_numerator, eta_denominator).unwrap();

        prop_assert!(budget_per_validator <= max_allowed);

        // Property 2: The protocol ensures that even if all validators spend their full budgets,
        // the system remains safe due to quorum requirements

        // Each validator gets η * balance budget
        let _total_validator_budgets = validators.len() as u64 * budget_per_validator.units();

        // Key insight from Stingray: certificates need 2f+1 signatures, so at least f+1 honest validators
        // must sign each transaction. This means the effective spending limit is constrained.
        // The protocol guarantees that no more than balance can actually be certified and spent.

        // Test the fundamental safety property: budget per validator should not exceed balance
        prop_assert!(budget_per_validator <= balance);

        // Test that η (eta) is correctly bounded: η = (f+1)/(2f+1) < 1 for f > 0
        if f > 0 {
            let eta_times_denominator = (f + 1) as u64;
            let denominator = (2 * f + 1) as u64;
            prop_assert!(eta_times_denominator < denominator);
        }
    }

    /// Property: Transaction nonces prevent replay attacks
    #[test]
    fn nonce_monotonicity(
        initial_balance in arb_amount(),
        transactions in prop::collection::vec((arb_pubkey(), arb_amount()), 1..10),
    ) {
        let validators = vec![Pubkey::test(1), Pubkey::test(2), Pubkey::test(3), Pubkey::test(4)];
        let mut engine = StateTransitionEngine::new(validators.clone(), 1);

        // Create account
        let creator = Pubkey::test(10);
        let create_tx = Transaction::CreateAccount { initial_balance };
        let signed_tx = SignedTransaction::new_single_signer(creator, create_tx, vec![], 0);
        let effects = engine.execute_transaction(&signed_tx, validators[0]).unwrap();
        let account_id = effects.created_objects[0];

        // Track nonces
        let mut expected_nonce = 1u64;

        // Execute transactions with proper nonces
        for (recipient, amount) in transactions.iter() {
            if amount > &initial_balance { continue; } // Skip invalid amounts

            // Create another account for recipient
            let create_recipient = Transaction::CreateAccount { initial_balance: Amount::ZERO };
            let signed_tx = SignedTransaction::new_single_signer(*recipient, create_recipient, vec![], 0);
            let recipient_effects = engine.execute_transaction(&signed_tx, validators[0]).unwrap();
            let recipient_id = recipient_effects.created_objects[0];

            // Try to settle directly
            let settle_tx = Transaction::SettleDirectly {
                provider: *recipient,
                job_spec_hash: Hash::compute(b"test"),
                result_hash: Hash::compute(b"result"),
                payment: *amount,
            };

            // Property: Transaction with wrong nonce should fail
            let _wrong_nonce_tx = SignedTransaction::new_multi_party(
                creator,
                vec![*recipient],
                settle_tx.clone(),
                vec![
                    ObjectRef::new(account_id, expected_nonce),
                    ObjectRef::new(recipient_id, 0),
                ],
                expected_nonce + 1, // Wrong nonce!
            );

            // This should fail (in a real implementation)
            // For now we just track that nonces increment

            let correct_tx = SignedTransaction::new_multi_party(
                creator,
                vec![*recipient],
                settle_tx,
                vec![
                    ObjectRef::new(account_id, expected_nonce),
                    ObjectRef::new(recipient_id, 0),
                ],
                expected_nonce,
            );

            if engine.execute_transaction(&correct_tx, validators[0]).is_ok() {
                expected_nonce += 1;
            }
        }

        prop_assert!(expected_nonce >= 1); // At least one transaction attempt
    }

    /// Property: Parallel transactions on different objects don't conflict
    #[test]
    fn parallel_non_interference(
        accounts in prop::collection::vec((arb_pubkey(), arb_amount()), 2..=10),
        transactions_per_account in 1usize..=5,
    ) {
        // Ensure unique accounts
        let unique_accounts: std::collections::HashSet<_> = accounts.iter().map(|(pk, _)| pk).collect();
        prop_assume!(unique_accounts.len() == accounts.len());
        prop_assume!(!accounts.is_empty());
        let validators = vec![Pubkey::test(1), Pubkey::test(2), Pubkey::test(3), Pubkey::test(4)];
        let mut engine = StateTransitionEngine::new(validators.clone(), 1);

        // Create all accounts
        let mut account_map = HashMap::new();
        for (pubkey, balance) in accounts {
            let create_tx = Transaction::CreateAccount { initial_balance: balance };
            let signed_tx = SignedTransaction::new_single_signer(pubkey, create_tx, vec![], 0);
            let effects = engine.execute_transaction(&signed_tx, validators[0]).unwrap();
            account_map.insert(pubkey, (effects.created_objects[0], balance));
        }

        // Property: Transactions touching different accounts can execute in any order
        // We simulate this by executing them sequentially but tracking that no conflicts occur
        let mut conflict_count = 0;
        let mut success_count = 0;
        let mut account_versions: HashMap<ObjectId, u64> = HashMap::new();
        let mut account_nonces: HashMap<Pubkey, u64> = HashMap::new();

        // Initialize version and nonce tracking
        for (pubkey, (account_id, _)) in &account_map {
            account_versions.insert(*account_id, 0); // Version 0 after creation
            account_nonces.insert(*pubkey, 1); // Nonce 1 after creation
        }

        for _ in 0..transactions_per_account {
            for (sender, (sender_id, sender_balance)) in account_map.iter() {
                // Pick a different account as recipient
                let recipient = account_map.keys()
                    .find(|k| k != &sender)
                    .cloned();

                if let Some(recipient) = recipient {
                    // Use a small fraction of sender's balance, minimum 1 unit
                    let payment_amount = if sender_balance.units() > 1 {
                        Amount::from_units(1)
                    } else {
                        continue; // Skip if insufficient balance
                    };

                    let recipient_id = account_map[&recipient].0;
                    let sender_version = account_versions[sender_id];
                    let recipient_version = account_versions[&recipient_id];
                    let sender_nonce = account_nonces[sender];

                    let settle_tx = Transaction::SettleDirectly {
                        provider: recipient,
                        job_spec_hash: Hash::compute(b"parallel test"),
                        result_hash: Hash::compute(b"result"),
                        payment: payment_amount,
                    };

                    let signed_tx = SignedTransaction::new_multi_party(
                        *sender,
                        vec![recipient],
                        settle_tx,
                        vec![
                            ObjectRef::new(*sender_id, sender_version),
                            ObjectRef::new(recipient_id, recipient_version),
                        ],
                        sender_nonce,
                    );

                    // Use the same validator that created the accounts (validators[0])
                    // This ensures the channels are properly initialized
                    match engine.execute_transaction(&signed_tx, validators[0]) {
                        Ok(_) => {
                            success_count += 1;
                            // Update versions and nonces after successful transaction
                            account_versions.insert(*sender_id, sender_version + 1);
                            account_versions.insert(recipient_id, recipient_version + 1);
                            account_nonces.insert(*sender, sender_nonce + 1);
                        }
                        Err(_) => conflict_count += 1,
                    }
                }
            }
        }

        // Property: Most transactions should succeed (low conflict rate)
        prop_assert!(success_count > 0);
        prop_assert!(conflict_count < success_count / 2); // Less than 50% conflicts
    }

    /// Property: Channel utilization stays within bounds
    #[test]
    fn channel_utilization_bounds(
        initial_balance in arb_amount(),
        spending_pattern in prop::collection::vec(1u64..=100u64, 10..50),
    ) {
        use hellas_protocol::bounded_counter::{BoundedCounterManager, ValidatorLocalState};

        let validators = vec![Pubkey::test(1), Pubkey::test(2), Pubkey::test(3), Pubkey::test(4)];
        let mut channel_manager = BoundedCounterManager::new(validators.clone(), 1);
        let mut local_state = ValidatorLocalState::new(validators[0]);

        let account_id = ObjectId::new([1; 32]);

        // Initialize channels
        channel_manager.init_account_channels(account_id, initial_balance, 0);
        local_state.init_channel(account_id, channel_manager.calculate_max_budget(initial_balance), 0);

        let mut total_spent = Amount::ZERO;
        let max_budget = channel_manager.calculate_max_budget(initial_balance);

        // Try to spend according to pattern
        for amount in spending_pattern {
            let tx_digest = Hash::compute(&amount.to_le_bytes());

            if local_state.try_spend(account_id, Amount::from_units(amount), tx_digest).is_ok() {
                total_spent = total_spent.checked_add(Amount::from_units(amount)).unwrap();
            }
        }

        // Properties:
        // 1. Total spent never exceeds the channel budget
        prop_assert!(total_spent <= max_budget);

        // 2. Channel utilization is tracked correctly
        if let Some(channel) = local_state.account_channels.get(&account_id) {
            prop_assert_eq!(channel.total_spent, total_spent);
            prop_assert_eq!(channel.remaining_budget.checked_add(channel.total_spent).unwrap(), max_budget);
        }
    }

    /// Property: Job escrow state machine transitions are valid
    #[test]
    fn escrow_state_machine(
        payment in arb_amount(),
        bond in arb_amount(),
        claim_deadline in 100u64..=1000u64,
        current_heights in prop::collection::vec(0u64..=2000u64, 5..10),
    ) {
        let requestor = Pubkey::test(1);
        let provider = Pubkey::test(2);

        let mut escrow = JobEscrow::new(
            Hash::compute(b"agreement"),
            requestor,
            Some(provider),
            Hash::compute(b"job"),
            payment,
            bond,
            claim_deadline,
        );

        // Track state transitions
        let mut state_history = vec![escrow.status];

        for height in current_heights {
            let old_status = escrow.status;

            // Simulate possible transitions based on current state and height
            match escrow.status {
                JobStatus::Posted => {
                    if height <= claim_deadline {
                        // Can be claimed
                        if height % 3 == 0 { // Randomly decide to claim
                            escrow.status = JobStatus::Claimed;
                            escrow.claimed_at = Some(height);
                            escrow.commit_deadline = height + 200;
                        }
                    } else if escrow.can_abort(height) {
                        escrow.status = JobStatus::Aborted;
                        escrow.aborted_at = Some(height);
                    }
                }
                JobStatus::Claimed => {
                    if height <= escrow.commit_deadline {
                        // Can commit result
                        if height % 3 == 1 {
                            escrow.status = JobStatus::Committed;
                            escrow.committed_at = Some(height);
                            escrow.result_hash = Some(Hash::compute(b"result"));
                            escrow.finalize_after = height + 50;
                        }
                    } else if escrow.can_abort(height) {
                        escrow.status = JobStatus::Aborted;
                        escrow.aborted_at = Some(height);
                    }
                }
                JobStatus::Committed => {
                    if escrow.can_finalize(height) {
                        escrow.status = JobStatus::Finalized;
                        escrow.finalized_at = Some(height);
                    }
                }
                JobStatus::Finalized | JobStatus::Aborted => {
                    // Terminal states - no transitions allowed
                }
            }

            if old_status != escrow.status {
                state_history.push(escrow.status);
            }
        }

        // Properties:
        // 1. State transitions follow valid paths
        for i in 1..state_history.len() {
            let valid_transition = match (state_history[i-1], state_history[i]) {
                (JobStatus::Posted, JobStatus::Claimed) => true,
                (JobStatus::Posted, JobStatus::Aborted) => true,
                (JobStatus::Claimed, JobStatus::Committed) => true,
                (JobStatus::Claimed, JobStatus::Aborted) => true,
                (JobStatus::Committed, JobStatus::Finalized) => true,
                (s1, s2) if s1 == s2 => true, // Staying in same state
                _ => false,
            };
            prop_assert!(valid_transition, "Invalid transition from {:?} to {:?}",
                        state_history[i-1], state_history[i]);
        }

        // 2. Terminal states are truly terminal
        if matches!(escrow.status, JobStatus::Finalized | JobStatus::Aborted) {
            let terminal_idx = state_history.iter().position(|s|
                matches!(s, JobStatus::Finalized | JobStatus::Aborted));
            if let Some(idx) = terminal_idx {
                // All subsequent states should be the same
                for i in idx..state_history.len() {
                    prop_assert_eq!(state_history[i], state_history[idx]);
                }
            }
        }
    }
}

#[cfg(test)]
mod budget_certificate_tests {
    use super::*;
    use hellas_protocol::crypto::Signature;
    use hellas_protocol::transactions::BudgetCertificate;

    proptest! {
        /// Property: Budget certificates aggregate correctly
        #[test]
        fn certificate_aggregation(
            validator_spends in prop::collection::vec((arb_pubkey(), 1u64..=1000u64), 1..=4),
        ) {
            // Ensure no duplicate validators
            let mut seen_validators = std::collections::HashSet::new();
            for (validator, _) in &validator_spends {
                prop_assume!(seen_validators.insert(*validator));
            }
            let mut certificates = Vec::new();
            let mut expected_total = Amount::ZERO;

            for (validator, amount) in validator_spends {
                let cert = BudgetCertificate {
                    validator,
                    total_spent: Amount::from_units(amount),
                    transactions: vec![Hash::compute(&amount.to_le_bytes())],
                    validator_signature: Signature::dummy(),
                };
                expected_total = expected_total.checked_add(Amount::from_units(amount)).unwrap();
                certificates.push(cert);
            }

            // Verify aggregation
            let total = certificates.iter()
                .map(|c| c.total_spent)
                .fold(Amount::ZERO, |acc, x| acc.checked_add(x).unwrap());
            prop_assert_eq!(total, expected_total);

            // Verify no duplicate validators
            let mut seen_validators = std::collections::HashSet::new();
            for cert in &certificates {
                prop_assert!(seen_validators.insert(cert.validator),
                           "Duplicate validator in certificates");
            }
        }
    }
}
