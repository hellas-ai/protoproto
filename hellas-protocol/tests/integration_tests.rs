//! # Integration Tests
//!
//! These tests verify complete transaction flows and interactions between
//! different components of the Hellas protocol.

use hellas_protocol::crypto::SigningKey;
use hellas_protocol::negotiation::*;
use hellas_protocol::transactions::ObjectRef;
use hellas_protocol::*;

/// Helper struct to manage test users
struct TestUser {
    pubkey: Pubkey,
    account_id: Option<ObjectId>,
    nonce: u64,
    balance: u64,
    version: u64, // Track object version separately from nonce
}

impl TestUser {
    fn new(id: u8) -> Self {
        Self {
            pubkey: Pubkey::test(id),
            account_id: None,
            nonce: 0,
            balance: 0,
            version: 0,
        }
    }
}

/// Helper to set up a test environment
fn setup_test_env() -> (StateTransitionEngine, Vec<Pubkey>) {
    let validators = vec![
        Pubkey::test(1),
        Pubkey::test(2),
        Pubkey::test(3),
        Pubkey::test(4),
    ];

    let engine = StateTransitionEngine::new(validators.clone(), 1);
    (engine, validators)
}

/// Helper to create an account and return the user
fn create_test_account(
    engine: &mut StateTransitionEngine,
    validator: Pubkey,
    user_id: u8,
    initial_balance: u64,
) -> TestUser {
    let mut user = TestUser::new(user_id);
    user.balance = initial_balance;

    let tx = Transaction::CreateAccount {
        initial_balance: Amount::from_units(initial_balance),
    };
    let signed_tx = SignedTransaction::new_single_signer(user.pubkey, tx, vec![], 0);

    let effects = engine
        .execute_transaction(&signed_tx, validator)
        .expect("Account creation should succeed");

    user.account_id = Some(effects.created_objects[0]);
    user.nonce = 1;
    user.version = 0; // Newly created objects start at version 0

    user
}

#[test]
fn test_complete_interactive_flow() {
    let (mut engine, validators) = setup_test_env();

    // Create two users
    let mut alice = create_test_account(&mut engine, validators[0], 10, 10_000);
    let mut bob = create_test_account(&mut engine, validators[0], 20, 1_000);

    println!("=== Interactive Flow Test ===");
    println!("Alice (requestor) balance: {}", alice.balance);
    println!("Bob (provider) balance: {}", bob.balance);

    // Step 1: Off-chain negotiation (simulated)
    let job_spec = JobSpec {
        catgrad_graph_hash: Hash::compute(b"llama-3.2-graph"),
        input_hashes: vec![Hash::compute(b"prompt")],
        requirements: JobRequirements {
            min_gpu_memory: Some(80),
            max_latency_ms: Some(100),
            is_streaming: true,
            estimated_flops: Some(1_000_000_000_000),
        },
        security_params: SecurityParams {
            min_stake_ratio: 0.5,
            challenge_period: 0,
            allow_early_finalization: true,
        },
        max_price: 1000,
        nonce: 1,
    };

    assert!(
        job_spec.is_interactive(),
        "Job should be marked as interactive"
    );

    // Step 2: Execute SettleDirectly transaction
    let payment = Amount::from_units(800);
    let settle_tx = Transaction::SettleDirectly {
        provider: bob.pubkey,
        job_spec_hash: job_spec.catgrad_graph_hash,
        result_hash: Hash::compute(b"streaming-result"),
        payment,
    };

    let signed_tx = SignedTransaction::new_multi_party(
        alice.pubkey,
        vec![bob.pubkey],
        settle_tx,
        vec![
            ObjectRef::new(alice.account_id.unwrap(), 0), // Version 0 initially
            ObjectRef::new(bob.account_id.unwrap(), 0),
        ],
        alice.nonce,
    );

    let effects = engine
        .execute_transaction(&signed_tx, validators[0])
        .expect("SettleDirectly should succeed");

    assert!(effects.success);
    alice.nonce += 1;
    alice.version += 1; // Both accounts' versions increment
    bob.version += 1;

    // Step 3: Verify balances (note: Alice's balance not yet updated!)
    let alice_obj = engine.state.get(&alice.account_id.unwrap()).unwrap();
    if let Object::Account(acc) = &alice_obj.object {
        println!(
            "Alice balance after payment (not reconciled): {}",
            acc.balance
        );
        assert_eq!(acc.balance, Amount::from_units(alice.balance)); // Unchanged!
    }

    let bob_obj = engine.state.get(&bob.account_id.unwrap()).unwrap();
    if let Object::Account(acc) = &bob_obj.object {
        println!("Bob balance after payment: {}", acc.balance);
        assert_eq!(
            acc.balance,
            Amount::from_units(bob.balance)
                .checked_add(payment)
                .unwrap()
        );
        bob.balance = acc.balance.units();
    }

    // Step 4: Collect budget certificate and reset
    let cert = engine
        .validator_local_state
        .create_certificate(
            alice.account_id.unwrap(),
            &SigningKey::from_pubkey(validators[0]),
        )
        .expect("Should have spending to certify");

    let reset_tx = Transaction::ResetBudget {
        budget_certificates: vec![cert],
    };

    let signed_tx = SignedTransaction::new_single_signer(
        alice.pubkey,
        reset_tx,
        vec![ObjectRef::new(alice.account_id.unwrap(), 1)], // Version 1 after SettleDirectly
        alice.nonce,
    );

    let effects = engine
        .execute_transaction(&signed_tx, validators[0])
        .expect("ResetBudget should succeed");

    assert!(effects.success);
    alice.nonce += 1;
    alice.version += 1; // Version increments after ResetBudget

    // Step 5: Verify final balances
    let alice_obj = engine.state.get(&alice.account_id.unwrap()).unwrap();
    if let Object::Account(acc) = &alice_obj.object {
        println!("Alice balance after reconciliation: {}", acc.balance);
        assert_eq!(
            acc.balance,
            Amount::from_units(alice.balance)
                .checked_sub(payment)
                .unwrap()
        );
        alice.balance = acc.balance.units();
    }

    println!("\nFinal balances:");
    println!("  Alice: {} (spent {})", alice.balance, payment);
    println!("  Bob: {} (earned {})", bob.balance, payment);
}

#[test]
fn test_complete_marketplace_flow() {
    let (mut engine, validators) = setup_test_env();

    // Create users
    let mut requestor = create_test_account(&mut engine, validators[0], 30, 5_000);
    let mut provider = create_test_account(&mut engine, validators[0], 40, 2_000);

    println!("\n=== Marketplace Flow Test ===");
    println!("Initial balances:");
    println!("  Requestor: {}", requestor.balance);
    println!("  Provider: {}", provider.balance);

    // Debug: Check the account versions after creation
    let requestor_obj = engine.state.get(&requestor.account_id.unwrap()).unwrap();
    println!(
        "  Requestor account version after creation: {}",
        requestor_obj.version
    );
    println!("  Requestor nonce: {}", requestor.nonce);

    // Step 1: Post job
    let payment = Amount::from_units(1_000);
    let bond_required = Amount::from_units(500);

    let post_job_tx = Transaction::PostJob {
        provider: Some(provider.pubkey),
        agreement_hash: Hash::compute(b"marketplace-agreement"),
        job_spec_hash: Hash::compute(b"batch-job"),
        payment,
        provider_bond_required: bond_required,
        claim_deadline_delta: 100,
        commit_deadline_delta: 200,
        finalization_delay: 50,
    };

    let signed_tx = SignedTransaction::new_single_signer(
        requestor.pubkey,
        post_job_tx,
        vec![ObjectRef::new(
            requestor.account_id.unwrap(),
            requestor.version,
        )],
        requestor.nonce,
    );

    let effects = engine
        .execute_transaction(&signed_tx, validators[0])
        .expect("PostJob should succeed");

    let escrow_id = effects.created_objects[1]; // First is account update, second is escrow
    requestor.nonce += 1;
    requestor.version += 1; // Version increments after transaction
    requestor.balance = Amount::from_units(requestor.balance)
        .checked_sub(payment)
        .unwrap()
        .units();

    println!("Job posted with escrow ID: {}", escrow_id);

    // Step 2: Provider claims job
    engine.current_height = 10;

    let claim_tx = Transaction::ClaimJob { escrow_id };
    let signed_tx = SignedTransaction::new_single_signer(
        provider.pubkey,
        claim_tx,
        vec![
            ObjectRef::new(escrow_id, 0),
            ObjectRef::new(provider.account_id.unwrap(), provider.version),
        ],
        0, // Provider's first transaction
    );

    let effects = engine
        .execute_transaction(&signed_tx, validators[0])
        .expect("ClaimJob should succeed");

    assert!(effects.success);
    provider.balance = Amount::from_units(provider.balance)
        .checked_sub(bond_required)
        .unwrap()
        .units();
    provider.version += 1; // Provider version increments after claim

    println!("Job claimed, provider bond locked: {}", bond_required);

    // Step 3: Provider commits result
    engine.current_height = 50;

    let commit_tx = Transaction::CommitResult {
        escrow_id,
        result_hash: Hash::compute(b"computation-result"),
    };

    let signed_tx = SignedTransaction::new_single_signer(
        provider.pubkey,
        commit_tx,
        vec![ObjectRef::new(escrow_id, 1)], // Version 1 after claim
        1,                                  // Provider's second transaction
    );

    let effects = engine
        .execute_transaction(&signed_tx, validators[0])
        .expect("CommitResult should succeed");

    assert!(effects.success);
    println!("Result committed, entering challenge period");

    // Step 4: Finalize after challenge period
    engine.current_height = 150; // After finalization delay

    let finalize_tx = Transaction::FinalizeJob { escrow_id };
    let signed_tx = SignedTransaction::new_single_signer(
        requestor.pubkey, // Anyone can finalize
        finalize_tx,
        vec![], // No input objects needed
        requestor.nonce,
    );

    let effects = engine
        .execute_transaction(&signed_tx, validators[0])
        .expect("FinalizeJob should succeed");

    assert!(effects.success);

    // Verify final balances
    let provider_obj = engine.state.get(&provider.account_id.unwrap()).unwrap();
    if let Object::Account(acc) = &provider_obj.object {
        println!("\nFinal provider balance: {}", acc.balance);
        assert_eq!(
            acc.balance,
            Amount::from_units(provider.balance)
                .checked_add(payment)
                .unwrap()
                .checked_add(bond_required)
                .unwrap()
        );
    }

    println!("\nMarketplace flow completed successfully!");
}

#[test]
fn test_job_abort_scenarios() {
    let (mut engine, validators) = setup_test_env();

    println!("\n=== Job Abort Scenarios Test ===");

    // Scenario 1: Requestor aborts before claim
    {
        let mut requestor = create_test_account(&mut engine, validators[0], 50, 3_000);
        let provider = create_test_account(&mut engine, validators[0], 51, 1_000);

        // Post job
        let post_tx = Transaction::PostJob {
            provider: Some(provider.pubkey),
            agreement_hash: Hash::compute(b"abort-test-1"),
            job_spec_hash: Hash::compute(b"job"),
            payment: Amount::from_units(500),
            provider_bond_required: Amount::from_units(250),
            claim_deadline_delta: 100,
            commit_deadline_delta: 200,
            finalization_delay: 50,
        };

        let signed_tx = SignedTransaction::new_single_signer(
            requestor.pubkey,
            post_tx,
            vec![ObjectRef::new(
                requestor.account_id.unwrap(),
                requestor.version,
            )],
            requestor.nonce,
        );

        let effects = engine
            .execute_transaction(&signed_tx, validators[0])
            .unwrap();
        let escrow_id = effects.created_objects[1];
        requestor.nonce += 1;
        requestor.version += 1; // Version increments after posting job

        // Advance past claim deadline
        engine.current_height = 200;

        // Requestor aborts
        let abort_tx = Transaction::AbortJob { escrow_id };
        let signed_tx = SignedTransaction::new_single_signer(
            requestor.pubkey,
            abort_tx,
            vec![ObjectRef::new(escrow_id, 0)],
            requestor.nonce,
        );

        let effects = engine
            .execute_transaction(&signed_tx, validators[0])
            .unwrap();
        assert!(effects.success);

        // Verify refund
        let requestor_obj = engine.state.get(&requestor.account_id.unwrap()).unwrap();
        if let Object::Account(acc) = &requestor_obj.object {
            assert_eq!(acc.balance, Amount::from_units(3_000)); // Full refund
            println!("Scenario 1 passed: Requestor got full refund");
        }
    }

    // Scenario 2: Provider misses deadline
    {
        engine.current_height = 300;

        let mut requestor = create_test_account(&mut engine, validators[0], 60, 3_000);
        let provider = create_test_account(&mut engine, validators[0], 61, 1_000);

        // Post and claim job
        let post_tx = Transaction::PostJob {
            provider: Some(provider.pubkey),
            agreement_hash: Hash::compute(b"abort-test-2"),
            job_spec_hash: Hash::compute(b"job"),
            payment: Amount::from_units(500),
            provider_bond_required: Amount::from_units(250),
            claim_deadline_delta: 100,
            commit_deadline_delta: 200,
            finalization_delay: 50,
        };

        let signed_tx = SignedTransaction::new_single_signer(
            requestor.pubkey,
            post_tx,
            vec![ObjectRef::new(
                requestor.account_id.unwrap(),
                requestor.version,
            )],
            requestor.nonce,
        );

        let effects = engine
            .execute_transaction(&signed_tx, validators[0])
            .unwrap();
        let escrow_id = effects.created_objects[1];
        requestor.nonce += 1;
        requestor.version += 1; // Version increments after posting job

        // Provider claims
        engine.current_height = 310;
        let claim_tx = Transaction::ClaimJob { escrow_id };
        let signed_tx = SignedTransaction::new_single_signer(
            provider.pubkey,
            claim_tx,
            vec![
                ObjectRef::new(escrow_id, 0),
                ObjectRef::new(provider.account_id.unwrap(), 0),
            ],
            0,
        );

        engine
            .execute_transaction(&signed_tx, validators[0])
            .unwrap();

        // Advance past commit deadline
        engine.current_height = 600;

        // Anyone can abort now
        let abort_tx = Transaction::AbortJob { escrow_id };
        let signed_tx = SignedTransaction::new_single_signer(
            requestor.pubkey,
            abort_tx,
            vec![ObjectRef::new(escrow_id, 1)],
            requestor.nonce,
        );

        let effects = engine
            .execute_transaction(&signed_tx, validators[0])
            .unwrap();
        assert!(effects.success);

        // Verify requestor got refund, provider was slashed
        let requestor_obj = engine.state.get(&requestor.account_id.unwrap()).unwrap();
        if let Object::Account(acc) = &requestor_obj.object {
            assert_eq!(acc.balance, Amount::from_units(3_000)); // Full refund
        }

        let provider_obj = engine.state.get(&provider.account_id.unwrap()).unwrap();
        if let Object::Account(acc) = &provider_obj.object {
            assert_eq!(acc.balance, Amount::from_units(750)); // Lost bond
            println!("Scenario 2 passed: Provider slashed for missing deadline");
        }
    }
}

#[test]
fn test_concurrent_payments() {
    let (mut engine, validators) = setup_test_env();

    println!("\n=== Concurrent Payments Test ===");

    // Create one sender and multiple recipients
    let mut alice = create_test_account(&mut engine, validators[0], 70, 10_000);
    let recipients: Vec<TestUser> = (0..4)
        .map(|i| create_test_account(&mut engine, validators[0], 80 + i, 0))
        .collect();

    // Debug: Check Alice's account version after creation
    let alice_obj = engine.state.get(&alice.account_id.unwrap()).unwrap();
    println!(
        "  Alice account version after creation: {}",
        alice_obj.version
    );
    println!("  Alice nonce: {}", alice.nonce);

    // Execute multiple payments without reconciliation
    for (i, recipient) in recipients.iter().enumerate() {
        let payment = Amount::from_units(100 * (i as u64 + 1));

        let settle_tx = Transaction::SettleDirectly {
            provider: recipient.pubkey,
            job_spec_hash: Hash::compute(format!("job-{}", i).as_bytes()),
            result_hash: Hash::compute(format!("result-{}", i).as_bytes()),
            payment,
        };

        let signed_tx = SignedTransaction::new_multi_party(
            alice.pubkey,
            vec![recipient.pubkey],
            settle_tx,
            vec![
                ObjectRef::new(alice.account_id.unwrap(), alice.version),
                ObjectRef::new(recipient.account_id.unwrap(), 0),
            ],
            alice.nonce,
        );

        // Use different validators to simulate parallel execution
        let validator = validators[i % validators.len()];
        let effects = engine
            .execute_transaction(&signed_tx, validator)
            .expect("Payment should succeed");

        assert!(effects.success);
        alice.nonce += 1;
        alice.version += 1; // Version increments after each payment

        println!(
            "Payment {} of {} sent via validator {}",
            i + 1,
            payment,
            validator
        );
    }

    // Alice's balance should still show original amount
    let alice_obj = engine.state.get(&alice.account_id.unwrap()).unwrap();
    if let Object::Account(acc) = &alice_obj.object {
        assert_eq!(acc.balance, Amount::from_units(10_000));
        println!("\nAlice balance before reconciliation: {}", acc.balance);
    }

    // Now reconcile with budget reset
    let cert = engine
        .validator_local_state
        .create_certificate(
            alice.account_id.unwrap(),
            &SigningKey::from_pubkey(validators[0]),
        )
        .expect("Should have certificates");

    let reset_tx = Transaction::ResetBudget {
        budget_certificates: vec![cert],
    };

    let signed_tx = SignedTransaction::new_single_signer(
        alice.pubkey,
        reset_tx,
        vec![ObjectRef::new(alice.account_id.unwrap(), alice.version)],
        alice.nonce,
    );

    engine
        .execute_transaction(&signed_tx, validators[0])
        .expect("ResetBudget should succeed");

    // Verify final balance
    let alice_obj = engine.state.get(&alice.account_id.unwrap()).unwrap();
    if let Object::Account(acc) = &alice_obj.object {
        let total_spent = 100 + 200 + 300 + 400; // 1000 total
        assert_eq!(acc.balance, Amount::from_units(10_000 - total_spent));
        println!(
            "Alice balance after reconciliation: {} (spent {})",
            acc.balance, total_spent
        );
    }

    println!("\nConcurrent payments test passed!");
}

#[test]
fn test_channel_budget_exhaustion() {
    let (mut engine, validators) = setup_test_env();

    println!("\n=== Channel Budget Exhaustion Test ===");

    let alice = create_test_account(&mut engine, validators[0], 90, 1_000);
    let bob = create_test_account(&mut engine, validators[0], 91, 0);

    // Calculate max budget per validator
    let max_budget = engine
        .channel_manager
        .calculate_max_budget(Amount::from_units(alice.balance));
    println!("Alice balance: {}", alice.balance);
    println!("Max budget per validator: {}", max_budget);

    // Try to spend more than the budget allows
    let oversized_payment = max_budget.checked_add(Amount::from_units(100)).unwrap();

    let settle_tx = Transaction::SettleDirectly {
        provider: bob.pubkey,
        job_spec_hash: Hash::compute(b"oversized"),
        result_hash: Hash::compute(b"result"),
        payment: oversized_payment,
    };

    let signed_tx = SignedTransaction::new_multi_party(
        alice.pubkey,
        vec![bob.pubkey],
        settle_tx,
        vec![
            ObjectRef::new(alice.account_id.unwrap(), 0), // Version 0 initially
            ObjectRef::new(bob.account_id.unwrap(), 0),
        ],
        alice.nonce,
    );

    let result = engine.execute_transaction(&signed_tx, validators[0]);

    assert!(result.is_err());
    match result {
        Err(ExecutionError::InsufficientBudget) => {
            println!("✓ Payment correctly rejected due to insufficient budget");
        }
        Err(e) => panic!("Wrong error type: {:?}", e),
        Ok(_) => panic!("Payment should have failed"),
    }

    // Now try with a valid amount
    let valid_payment = max_budget.mul_rational(1, 2).unwrap();

    let settle_tx = Transaction::SettleDirectly {
        provider: bob.pubkey,
        job_spec_hash: Hash::compute(b"valid"),
        result_hash: Hash::compute(b"result"),
        payment: valid_payment,
    };

    let signed_tx = SignedTransaction::new_multi_party(
        alice.pubkey,
        vec![bob.pubkey],
        settle_tx,
        vec![
            ObjectRef::new(alice.account_id.unwrap(), 0), // Version 0 initially
            ObjectRef::new(bob.account_id.unwrap(), 0),
        ],
        alice.nonce,
    );

    let result = engine.execute_transaction(&signed_tx, validators[0]);
    assert!(result.is_ok());

    println!("✓ Valid payment within budget succeeded");
}
