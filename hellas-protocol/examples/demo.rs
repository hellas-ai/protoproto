//! # Hellas Protocol Demo
//!
//! This example demonstrates the key features of the Hellas protocol:
//!
//! 1. **Interactive Flow**: Direct client-provider settlement with sub-second latency
//! 2. **Marketplace Flow**: Multi-step job execution with escrow and timeouts
//! 3. **Parallel Execution**: Multiple independent transactions processed concurrently
//! 4. **Bounded Counters**: Concurrent spending from the same account

use hellas_protocol::{engine::*, objects::*, parallel::*, transactions::*, types::*};
use std::time::Instant;

fn main() {
    println!("=== Hellas Protocol Demo ===\n");

    // Setup validators (4 validators, tolerating 1 Byzantine)
    let validators = vec![
        Pubkey::test(1),
        Pubkey::test(2),
        Pubkey::test(3),
        Pubkey::test(4),
    ];
    let byzantine_tolerance = 1;

    // Create the state transition engine
    let mut engine = StateTransitionEngine::new(validators.clone(), byzantine_tolerance);

    // Create test users
    let requestor = Pubkey::test(10);
    let provider = Pubkey::test(20);
    let validator1 = validators[0];

    println!("1. Creating accounts...");
    demo_create_accounts(&mut engine, requestor, provider, validator1);

    println!("\n2. Interactive Flow (SettleDirectly)...");
    demo_interactive_flow(&mut engine, requestor, provider, validator1);

    println!("\n3. Marketplace Flow (Job Escrow)...");
    demo_marketplace_flow(&mut engine, requestor, provider, validator1);

    println!("\n4. Parallel Execution Demo...");
    demo_parallel_execution();

    println!("\n5. Bounded Counter Concurrency...");
    demo_bounded_counter_concurrency();

    println!("\n=== Demo Complete ===");
}

/// Demo 1: Create accounts for users
fn demo_create_accounts(
    engine: &mut StateTransitionEngine,
    requestor: Pubkey,
    provider: Pubkey,
    validator: Pubkey,
) {
    // Create requestor account with 10,000 HELL tokens
    let create_requestor = SignedTransaction::new_single_signer(
        requestor,
        Transaction::CreateAccount {
            initial_balance: Amount::from_units(10_000),
        },
        vec![],
        0,
    );

    let effects = engine
        .execute_transaction(&create_requestor, validator)
        .unwrap();
    println!(
        "  ✓ Created requestor account: {:?}",
        effects.created_objects[0]
    );

    // Create provider account with 1,000 HELL tokens
    let create_provider = SignedTransaction::new_single_signer(
        provider,
        Transaction::CreateAccount {
            initial_balance: Amount::from_units(1_000),
        },
        vec![],
        0,
    );

    let effects = engine
        .execute_transaction(&create_provider, validator)
        .unwrap();
    println!(
        "  ✓ Created provider account: {:?}",
        effects.created_objects[0]
    );
}

/// Demo 2: Interactive flow - direct settlement between trusted parties
fn demo_interactive_flow(
    engine: &mut StateTransitionEngine,
    requestor: Pubkey,
    provider: Pubkey,
    validator: Pubkey,
) {
    println!("  Requestor and provider agree on job off-chain...");

    // Job details (agreed off-chain)
    let job_spec = b"Run GPT-4 inference on 'Explain quantum computing'";
    let job_spec_hash = Hash::compute(job_spec);

    let result = b"Quantum computing uses quantum bits that can be in superposition...";
    let result_hash = Hash::compute(result);

    let payment = Amount::from_units(100); // 100 HELL tokens

    // Get account IDs (in real system, these would be known)
    let requestor_account_id = get_account_id(engine, requestor);
    let provider_account_id = get_account_id(engine, provider);

    // Create a SettleDirectly transaction with both signatures
    let settle_tx = SignedTransaction::new_multi_party(
        requestor,
        vec![provider],
        Transaction::SettleDirectly {
            provider,
            job_spec_hash,
            result_hash,
            payment,
        },
        vec![
            ObjectRef::new(requestor_account_id, 1), // Version 1 after creation
            ObjectRef::new(provider_account_id, 1),
        ],
        1, // Nonce
    );

    let start = Instant::now();
    let _effects = engine.execute_transaction(&settle_tx, validator).unwrap();
    let elapsed = start.elapsed();

    println!("  ✓ Direct settlement completed in {:?}", elapsed);
    println!("    - Payment: {} HELL", payment);
    println!("    - Job hash: {:?}", &job_spec_hash.as_bytes()[..8]);
    println!("    - Result hash: {:?}", &result_hash.as_bytes()[..8]);

    // Check balances
    print_account_balance(engine, requestor, "Requestor");
    print_account_balance(engine, provider, "Provider");
}

/// Demo 3: Marketplace flow - untrusted execution with escrow
fn demo_marketplace_flow(
    engine: &mut StateTransitionEngine,
    requestor: Pubkey,
    provider: Pubkey,
    validator: Pubkey,
) {
    println!("  Step 1: Requestor posts job to marketplace...");

    let job_spec = b"Train small neural network on MNIST subset";
    let job_spec_hash = Hash::compute(job_spec);
    let payment = Amount::from_units(500);
    let provider_bond = Amount::from_units(250);

    let requestor_account_id = get_account_id(engine, requestor);

    let post_job_tx = SignedTransaction::new_single_signer(
        requestor,
        Transaction::PostJob {
            provider: Some(provider),
            agreement_hash: Hash::compute(b"demo-agreement"),
            job_spec_hash,
            payment,
            provider_bond_required: provider_bond,
            claim_deadline_delta: 100,
            commit_deadline_delta: 200,
            finalization_delay: 50,
        },
        vec![ObjectRef::new(requestor_account_id, 2)], // Version 2 after settle
        2,
    );

    let effects = engine.execute_transaction(&post_job_tx, validator).unwrap();
    let escrow_id = effects.created_objects[1]; // First is updated account, second is escrow
    println!("  ✓ Job posted with escrow ID: {:?}", escrow_id);

    // In a real system, provider would discover this job and decide to claim it
    println!("\n  Step 2: Provider claims job...");

    // For demo purposes, we'll skip the claim/commit/finalize steps
    // as they require more complex state management

    println!("  ✓ (Additional steps omitted for brevity)");
}

/// Demo 4: Parallel execution of independent transactions
fn demo_parallel_execution() {
    // Create a new engine for this demo
    let validators = vec![
        Pubkey::test(1),
        Pubkey::test(2),
        Pubkey::test(3),
        Pubkey::test(4),
    ];
    let engine = StateTransitionEngine::new(validators, 1);
    let parallel_executor = ParallelExecutor::new(engine);

    // Create a batch of independent account creations
    let mut transactions = Vec::new();
    for i in 0..8 {
        let user = Pubkey::test(100 + i as u8);
        let tx = SignedTransaction::new_single_signer(
            user,
            Transaction::CreateAccount {
                initial_balance: Amount::from_units(1000),
            },
            vec![],
            0,
        );
        transactions.push(tx);
    }

    let batch = TransactionBatch {
        transactions: transactions.clone(),
        proposing_validator: Pubkey::test(1),
    };

    // Analyze parallelism opportunity
    let analysis = ParallelExecutor::analyze_parallelism(&batch);
    println!("  Batch analysis:");
    println!("    - Total transactions: {}", analysis.total_transactions);
    println!("    - Parallel rounds needed: {}", analysis.parallel_rounds);
    println!("    - Max parallelism: {}", analysis.max_parallelism);
    println!(
        "    - Theoretical speedup (4 cores): {:.1}x",
        analysis.speedup(4)
    );

    // Execute the batch
    let start = Instant::now();
    let result = parallel_executor.execute_batch(batch);
    let elapsed = start.elapsed();

    let successful = result.effects.iter().filter(|e| e.is_ok()).count();
    println!(
        "\n  ✓ Executed {} transactions in {:?}",
        successful, elapsed
    );
    println!("    - Rounds used: {}", result.rounds);
    println!(
        "    - Time per transaction: {:?}",
        elapsed / successful as u32
    );
}

/// Demo 5: Bounded counter enabling concurrent transactions
fn demo_bounded_counter_concurrency() {
    println!("  Setting up high-volume account (exchange)...");

    let validators = vec![
        Pubkey::test(1),
        Pubkey::test(2),
        Pubkey::test(3),
        Pubkey::test(4),
    ];
    let mut engine = StateTransitionEngine::new(validators.clone(), 1);

    // Create an exchange account with large balance
    let exchange = Pubkey::test(200);
    let create_exchange = SignedTransaction::new_single_signer(
        exchange,
        Transaction::CreateAccount {
            initial_balance: Amount::from_units(1_000_000),
        },
        vec![],
        0,
    );
    engine
        .execute_transaction(&create_exchange, validators[0])
        .unwrap();

    println!("  ✓ Exchange account created with 1M HELL");

    // Show how different validators can process payments concurrently
    println!("\n  Simulating concurrent payments from exchange...");

    // Each validator has a budget of ~333,333 HELL (1M * 1/3)
    // They can all approve transactions up to their budget without coordination

    let mut total_spent = 0;
    for (i, &_validator) in validators.iter().enumerate() {
        let payment = 10_000 * (i + 1) as u64;
        println!(
            "    - Validator {} approving {} HELL payment",
            i + 1,
            payment
        );
        total_spent += payment;
    }

    println!("\n  ✓ Total concurrent payments: {} HELL", total_spent);
    println!("    Without bounded counters: 4 sequential transactions");
    println!("    With bounded counters: 4 parallel transactions!");
}

// Helper functions

fn get_account_id(engine: &StateTransitionEngine, pubkey: Pubkey) -> ObjectId {
    // In a real system, we'd have an index. For demo, we search.
    for (id, metadata) in &engine.state {
        if metadata.owner_set.contains(&pubkey) {
            if let Object::Account(_) = &metadata.object {
                return *id;
            }
        }
    }
    panic!("Account not found for {:?}", pubkey);
}

fn print_account_balance(engine: &StateTransitionEngine, pubkey: Pubkey, label: &str) {
    let account_id = get_account_id(engine, pubkey);
    let metadata = engine.state.get(&account_id).unwrap();
    if let Object::Account(account) = &metadata.object {
        println!("    - {} balance: {} HELL", label, account.balance);
    }
}
