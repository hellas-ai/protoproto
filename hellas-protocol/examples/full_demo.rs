//! # Hellas Protocol Full Demo
//!
//! This example demonstrates both the interactive (fast) and marketplace (escrow)
//! flows of the Hellas protocol, including:
//!
//! 1. Off-chain negotiation
//! 2. Channelized execution for concurrent payments
//! 3. Budget reconciliation
//! 4. Job escrow lifecycle

use hellas_protocol::crypto::SigningKey;
use hellas_protocol::negotiation::*;
use hellas_protocol::transactions::ObjectRef;
use hellas_protocol::*;

fn main() {
    println!("=== Hellas Protocol Demo ===\n");

    // Setup: Create validators and engine
    let validators = vec![
        Pubkey::test(1),
        Pubkey::test(2),
        Pubkey::test(3),
        Pubkey::test(4),
    ];

    let mut engine = StateTransitionEngine::new(validators.clone(), 1); // f=1

    // Create accounts
    println!("1. Creating accounts...");

    // Requestor with 10,000 HELL tokens
    let requestor = Pubkey::test(10);
    let create_requestor = Transaction::CreateAccount {
        initial_balance: Amount::from_units(10_000),
    };
    let signed_tx = SignedTransaction::new_single_signer(requestor, create_requestor, vec![], 0);
    let effects = engine
        .execute_transaction(&signed_tx, validators[0])
        .unwrap();
    let requestor_account_id = effects.created_objects[0];
    println!(
        "  ✓ Created requestor account with ID: {}",
        requestor_account_id
    );

    // Provider with 1,000 HELL tokens
    let provider = Pubkey::test(20);
    let create_provider = Transaction::CreateAccount {
        initial_balance: Amount::from_units(1_000),
    };
    let signed_tx = SignedTransaction::new_single_signer(provider, create_provider, vec![], 0);
    let effects = engine
        .execute_transaction(&signed_tx, validators[0])
        .unwrap();
    let provider_account_id = effects.created_objects[0];
    println!(
        "  ✓ Created provider account with ID: {}",
        provider_account_id
    );

    println!("\n2. Off-chain negotiation phase...");

    // Requestor broadcasts job specification
    let job_spec = JobSpec {
        catgrad_graph_hash: Hash::compute(b"llama-3.2-graph"),
        input_hashes: vec![
            Hash::compute(b"model-weights"),
            Hash::compute(b"user-prompt"),
        ],
        requirements: JobRequirements {
            min_gpu_memory: Some(80),
            max_latency_ms: Some(100),
            is_streaming: true,
            estimated_flops: Some(1_000_000_000_000),
        },
        security_params: SecurityParams {
            min_stake_ratio: 0.5,
            challenge_period: 50,
            allow_early_finalization: true,
        },
        max_price: 1000,
        nonce: 1,
    };

    println!("  ✓ Requestor broadcasts job spec (streaming LLM inference)");

    // Provider responds with quote
    let provider_quote = ProviderQuote {
        job_spec_hash: Hash::compute(&bincode::serialize(&job_spec).unwrap()),
        provider,
        price: 800,
        estimated_latency_ms: 50,
        stake_amount: 400,
        capabilities: ProviderCapabilities {
            gpu_memory_gb: 80,
            gpu_model: "A100".to_string(),
            gpu_count: 1,
            has_model_cached: true,
            bandwidth_mbps: 10_000,
        },
        valid_until: 1000,
        signature: Signature::dummy(),
    };

    println!("  ✓ Provider offers: 800 HELL, 50ms latency, model cached");

    // Create agreement
    let agreement = JobAgreement {
        job_spec: job_spec.clone(),
        selected_quote: provider_quote,
        requestor_signature: Signature::dummy(),
        agreed_at_block: engine.current_height,
    };

    println!("  ✓ Agreement reached!");

    // Check if this should use interactive flow
    if agreement.is_interactive() {
        println!("\n3. Using INTERACTIVE FLOW (streaming job)...");

        // For interactive jobs, use SettleDirectly
        let settle_tx = Transaction::SettleDirectly {
            provider,
            job_spec_hash: job_spec.catgrad_graph_hash,
            result_hash: Hash::compute(b"streaming-result"),
            payment: Amount::from_units(800),
        };

        let signed_tx = SignedTransaction::new_multi_party(
            requestor,
            vec![provider],
            settle_tx,
            vec![
                ObjectRef::new(requestor_account_id, 0),
                ObjectRef::new(provider_account_id, 0),
            ],
            1, // nonce
        );

        // Simulate multiple validators processing this concurrently
        println!("  ✓ Transaction sent to validator 0 (channelized execution)");
        engine
            .execute_transaction(&signed_tx, validators[0])
            .unwrap();

        // Check balances (note: requestor balance hasn't changed yet!)
        let requestor_obj = engine.state.get(&requestor_account_id).unwrap();
        if let Object::Account(acc) = &requestor_obj.object {
            println!("  ✓ Requestor balance still shows: {} HELL", acc.balance);
            println!("    (Payment deducted from validator 0's channel only)");
        }

        // Provider balance is immediately updated
        let provider_obj = engine.state.get(&provider_account_id).unwrap();
        if let Object::Account(acc) = &provider_obj.object {
            println!("  ✓ Provider balance updated to: {} HELL", acc.balance);
        }

        println!("\n4. Budget reconciliation (ResetBudget)...");

        // Collect spending certificates from validators
        let cert = engine
            .validator_local_state
            .create_certificate(
                requestor_account_id,
                &SigningKey::from_pubkey(validators[0]),
            )
            .unwrap();

        let reset_tx = Transaction::ResetBudget {
            budget_certificates: vec![cert],
        };

        let signed_tx = SignedTransaction::new_single_signer(
            requestor,
            reset_tx,
            vec![ObjectRef::new(requestor_account_id, 1)], // Version 1 after SettleDirectly
            2,                                             // nonce
        );

        engine
            .execute_transaction(&signed_tx, validators[0])
            .unwrap();

        // Now check reconciled balance
        let requestor_obj = engine.state.get(&requestor_account_id).unwrap();
        if let Object::Account(acc) = &requestor_obj.object {
            println!(
                "  ✓ Requestor balance after reconciliation: {} HELL",
                acc.balance
            );
        }
    } else {
        println!("\n3. Using MARKETPLACE FLOW (batch job)...");

        // For batch jobs, use the escrow flow
        let post_job_tx = Transaction::PostJob {
            provider: Some(provider),
            agreement_hash: agreement.hash(),
            job_spec_hash: job_spec.catgrad_graph_hash,
            payment: Amount::from_units(800),
            provider_bond_required: Amount::from_units(400),
            claim_deadline_delta: 100,
            commit_deadline_delta: 200,
            finalization_delay: 50,
        };

        let signed_tx = SignedTransaction::new_single_signer(
            requestor,
            post_job_tx,
            vec![ObjectRef::new(requestor_account_id, 0)],
            1,
        );

        let effects = engine
            .execute_transaction(&signed_tx, validators[0])
            .unwrap();
        let escrow_id = effects.created_objects[1]; // First is updated account, second is escrow

        println!("  ✓ Job posted with escrow");

        // Provider claims the job
        engine.current_height = 10;
        let claim_tx = Transaction::ClaimJob { escrow_id };
        let _signed_tx = SignedTransaction::new_single_signer(
            provider,
            claim_tx,
            vec![
                ObjectRef::new(escrow_id, 0),
                ObjectRef::new(provider_account_id, 0),
            ],
            1,
        );

        println!("  ✓ Provider claiming job (would stake bond)...");
        // engine.execute_transaction(&signed_tx, validators[0]).unwrap();
        // (Skipped as ClaimJob implementation is TODO)
    }

    println!("\n5. Parallel execution demonstration...");

    // Show how multiple small payments can be processed concurrently
    println!("  Simulating 3 concurrent micropayments from requestor...");

    // In a real system, these would be processed by different validators
    // in parallel without coordination
    for i in 0..3 {
        let _payment_tx = Transaction::SettleDirectly {
            provider,
            job_spec_hash: Hash::compute(format!("job-{}", i).as_bytes()),
            result_hash: Hash::compute(format!("result-{}", i).as_bytes()),
            payment: Amount::from_units(10),
        };

        // Each payment uses a different validator's channel
        let validator_idx = i % validators.len();
        println!("    - Payment {} via validator {}", i + 1, validator_idx);

        // In production, each validator would process independently
        // Here we simulate by switching validator context
        // (Real implementation would have separate ValidatorLocalState per validator)
    }

    println!("\n=== Demo Complete ===");
    println!("\nKey takeaways:");
    println!("- Off-chain negotiation keeps the chain lean");
    println!("- Interactive jobs use fast single-transaction settlement");
    println!("- Batch jobs use escrow for security");
    println!("- Channelized execution enables massive parallelism");
    println!("- Periodic budget resets maintain consistency");
}
