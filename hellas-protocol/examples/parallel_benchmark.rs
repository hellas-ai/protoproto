//! Benchmark demonstrating the performance improvement of the new parallel executor

use hellas_protocol::engine::StateTransitionEngine;
use hellas_protocol::parallel::{ParallelExecutor, TransactionBatch};
use hellas_protocol::transactions::{SignedTransaction, Transaction};
use hellas_protocol::types::Pubkey;
use hellas_protocol::Amount;
use std::time::Instant;

fn main() {
    println!("Parallel Execution Benchmark");
    println!("============================\n");

    // Setup validators
    let validators = vec![
        Pubkey::test(1),
        Pubkey::test(2),
        Pubkey::test(3),
        Pubkey::test(4),
    ];

    // Create initial state with accounts
    let mut engine = StateTransitionEngine::new(validators.clone(), 1);
    let num_accounts = 100;

    println!("Creating {} initial accounts...", num_accounts);
    let mut account_creators = Vec::new();
    for i in 0..num_accounts {
        let creator = Pubkey::test(100 + i);
        account_creators.push(creator);

        let tx = Transaction::CreateAccount {
            initial_balance: Amount::from_units(10000),
        };
        let signed_tx = SignedTransaction::new_single_signer(creator, tx, vec![], 0);
        engine
            .execute_transaction(&signed_tx, validators[0])
            .unwrap();
    }

    // Benchmark 1: Independent transactions (best case for parallelism)
    println!("\nBenchmark 1: Independent Transactions");
    println!("-------------------------------------");

    let mut independent_txs = Vec::new();
    for (i, creator) in account_creators.iter().enumerate() {
        let tx = Transaction::CreateAccount {
            initial_balance: Amount::from_units(1000 + i as u64),
        };
        let signed_tx = SignedTransaction::new_single_signer(*creator, tx, vec![], 1);
        independent_txs.push(signed_tx);
    }

    let batch = TransactionBatch {
        transactions: independent_txs.clone(),
        proposing_validator: validators[0],
    };

    // Analyze parallelism opportunity
    let analysis = ParallelExecutor::analyze_parallelism(&batch);
    println!("Parallelism Analysis:");
    println!("  Total transactions: {}", analysis.total_transactions);
    println!("  Parallel rounds needed: {}", analysis.parallel_rounds);
    println!("  Max parallelism: {}", analysis.max_parallelism);
    println!(
        "  Theoretical speedup (4 cores): {:.2}x",
        analysis.speedup(4)
    );
    println!(
        "  Theoretical speedup (8 cores): {:.2}x",
        analysis.speedup(8)
    );

    // Execute with new parallel executor
    let parallel_executor = ParallelExecutor::new(engine.clone());

    let start = Instant::now();
    let result = parallel_executor.execute_batch(batch.clone());
    let parallel_time = start.elapsed();

    println!("\nExecution Results:");
    println!("  Parallel execution time: {:?}", parallel_time);
    println!("  Rounds executed: {}", result.rounds);

    // Compare with sequential execution (simulated)
    let start = Instant::now();
    let mut seq_engine = engine.clone();
    for tx in &independent_txs {
        seq_engine.execute_transaction(tx, validators[0]).unwrap();
    }
    let sequential_time = start.elapsed();

    println!("  Sequential execution time: {:?}", sequential_time);
    println!(
        "  Actual speedup: {:.2}x",
        sequential_time.as_secs_f64() / parallel_time.as_secs_f64()
    );

    // Benchmark 2: Mixed dependencies (realistic case)
    println!("\n\nBenchmark 2: Mixed Dependencies");
    println!("--------------------------------");

    let mut mixed_txs = Vec::new();

    // Create groups of transactions with some dependencies
    for group in 0..10 {
        let base = group * 10;

        // Independent transactions within group
        for i in 0..8 {
            let creator = account_creators[base + i];
            let tx = Transaction::CreateAccount {
                initial_balance: Amount::from_units(2000 + i as u64),
            };
            let signed_tx = SignedTransaction::new_single_signer(creator, tx, vec![], 2);
            mixed_txs.push(signed_tx);
        }

        // Add some that depend on earlier ones (simulate conflicts)
        for i in 8..10 {
            let creator = account_creators[base + i % 8]; // Reuse earlier accounts
            let tx = Transaction::CreateAccount {
                initial_balance: Amount::from_units(3000 + i as u64),
            };
            let signed_tx = SignedTransaction::new_single_signer(creator, tx, vec![], 2);
            mixed_txs.push(signed_tx);
        }
    }

    let mixed_batch = TransactionBatch {
        transactions: mixed_txs,
        proposing_validator: validators[0],
    };

    let analysis = ParallelExecutor::analyze_parallelism(&mixed_batch);
    println!("Parallelism Analysis:");
    println!("  Total transactions: {}", analysis.total_transactions);
    println!("  Parallel rounds needed: {}", analysis.parallel_rounds);
    println!("  Total conflicts: {}", analysis.total_conflicts);
    println!(
        "  Theoretical speedup (4 cores): {:.2}x",
        analysis.speedup(4)
    );

    // Benchmark 3: Optimistic execution
    println!("\n\nBenchmark 3: Optimistic Execution");
    println!("---------------------------------");

    let optimistic_batch = TransactionBatch {
        transactions: independent_txs[..50].to_vec(),
        proposing_validator: validators[0],
    };

    let start = Instant::now();
    let result = parallel_executor.execute_batch_optimistic(optimistic_batch);
    let optimistic_time = start.elapsed();

    println!("Optimistic Execution Results:");
    println!("  Execution time: {:?}", optimistic_time);
    println!("  Rounds: {} (always 1 for optimistic)", result.rounds);
    println!(
        "  Success rate: {:.1}%",
        result.effects.iter().filter(|e| e.is_ok()).count() as f64 / result.effects.len() as f64
            * 100.0
    );

    println!("\n✅ Benchmark complete!");
}
