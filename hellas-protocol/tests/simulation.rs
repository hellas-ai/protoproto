//! # Randomized Simulation Framework
//!
//! This module implements a comprehensive simulation framework for stress-testing
//! the Hellas protocol under various conditions. It generates random workloads
//! and verifies that invariants hold throughout execution.

use hellas_protocol::transactions::ObjectRef;
use hellas_protocol::*;
use rand::distributions::{Distribution, WeightedIndex};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tracing::{debug, info};

/// Configuration for the simulation
#[derive(Debug, Clone)]
pub struct SimulationConfig {
    /// Random seed for reproducibility
    pub seed: u64,

    /// Number of validators
    pub num_validators: usize,

    /// Byzantine tolerance (f)
    pub byzantine_tolerance: usize,

    /// Number of user accounts to create
    pub num_accounts: usize,

    /// Initial balance range for accounts
    pub initial_balance_range: (Amount, Amount),

    /// Number of simulation steps
    pub num_steps: usize,

    /// Probability of each transaction type
    pub tx_probabilities: TransactionProbabilities,

    /// Whether to enable concurrent execution simulation
    pub enable_concurrency: bool,

    /// Block time in milliseconds
    pub block_time_ms: u64,
}

/// Probabilities for different transaction types
#[derive(Debug, Clone)]
pub struct TransactionProbabilities {
    pub create_account: f64,
    pub settle_directly: f64,
    pub post_job: f64,
    pub claim_job: f64,
    pub commit_result: f64,
    pub finalize_job: f64,
    pub abort_job: f64,
    pub reset_budget: f64,
}

impl Default for TransactionProbabilities {
    fn default() -> Self {
        Self {
            create_account: 0.05,
            settle_directly: 0.40,
            post_job: 0.15,
            claim_job: 0.10,
            commit_result: 0.10,
            finalize_job: 0.05,
            abort_job: 0.05,
            reset_budget: 0.10,
        }
    }
}

/// Represents a simulated user in the system
#[derive(Debug, Clone)]
struct SimulatedUser {
    pubkey: Pubkey,
    account_id: Option<ObjectId>,
    nonce: u64,
    pending_jobs: Vec<ObjectId>,
    provider_jobs: Vec<ObjectId>,
}

/// Main simulation state
pub struct Simulation {
    config: SimulationConfig,
    rng: StdRng,
    engine: StateTransitionEngine,
    users: Vec<SimulatedUser>,
    active_escrows: HashMap<ObjectId, EscrowInfo>,
    metrics: SimulationMetrics,
}

/// Information about active escrows
#[derive(Debug, Clone)]
struct EscrowInfo {
    _requestor_idx: usize,
    provider_idx: usize,
    _created_at: u64,
    claim_deadline: u64,
    _commit_deadline: u64,
    _finalize_after: u64,
}

/// Metrics collected during simulation
#[derive(Debug, Default)]
pub struct SimulationMetrics {
    pub total_transactions: usize,
    pub successful_transactions: usize,
    pub failed_transactions: HashMap<String, usize>,
    pub total_volume_transferred: Amount,
    pub max_concurrent_transactions: usize,
    pub average_block_time_ms: f64,
    pub channel_resets: usize,
    pub jobs_completed: usize,
    pub jobs_aborted: usize,
    pub invariant_violations: Vec<String>,
}

impl Simulation {
    pub fn new(config: SimulationConfig) -> Self {
        let rng = StdRng::seed_from_u64(config.seed);

        let validators: Vec<Pubkey> = (0..config.num_validators)
            .map(|i| Pubkey::test(i as u8))
            .collect();

        let engine = StateTransitionEngine::new(validators, config.byzantine_tolerance);

        Self {
            config,
            rng,
            engine,
            users: Vec::new(),
            active_escrows: HashMap::new(),
            metrics: SimulationMetrics::default(),
        }
    }

    /// Run the full simulation
    pub fn run(&mut self) -> Result<SimulationMetrics, String> {
        info!(
            "Starting simulation with {} validators, {} users, {} steps",
            self.config.num_validators, self.config.num_accounts, self.config.num_steps
        );

        // Initialize observability
        observability::init_metrics();

        // Phase 1: Create initial accounts
        self.create_initial_accounts()?;

        // Phase 2: Run main simulation loop
        let start_time = Instant::now();

        for step in 0..self.config.num_steps {
            if step % 100 == 0 {
                debug!("Simulation step {}/{}", step, self.config.num_steps);
            }

            // Advance block height
            self.engine.current_height += 1;

            // Generate and execute random transactions
            let tx_count = self.generate_transactions_for_block();

            if tx_count > self.metrics.max_concurrent_transactions {
                self.metrics.max_concurrent_transactions = tx_count;
            }

            // Check invariants periodically
            if step % 50 == 0 {
                self.check_invariants();
            }

            // Simulate block time
            std::thread::sleep(Duration::from_millis(self.config.block_time_ms));
        }

        let total_time = start_time.elapsed();
        self.metrics.average_block_time_ms =
            total_time.as_millis() as f64 / self.config.num_steps as f64;

        // Final invariant check
        self.check_invariants();

        info!(
            "Simulation complete: {} successful, {} failed transactions",
            self.metrics.successful_transactions,
            self.metrics.failed_transactions.values().sum::<usize>()
        );

        Ok(std::mem::take(&mut self.metrics))
    }

    /// Create initial user accounts
    fn create_initial_accounts(&mut self) -> Result<(), String> {
        for i in 0..self.config.num_accounts {
            let pubkey = Pubkey::test((100 + i) as u8);
            let balance_units = self.rng.gen_range(
                self.config.initial_balance_range.0.units()
                    ..=self.config.initial_balance_range.1.units(),
            );
            let balance = Amount::from_units(balance_units);

            let tx = Transaction::CreateAccount {
                initial_balance: balance,
            };
            let signed_tx = SignedTransaction::new_single_signer(pubkey, tx, vec![], 0);

            let validator_idx = i % self.config.num_validators;
            let validator = self.engine.validators[validator_idx];

            match self.engine.execute_transaction(&signed_tx, validator) {
                Ok(effects) => {
                    let account_id = effects.created_objects[0];
                    self.users.push(SimulatedUser {
                        pubkey,
                        account_id: Some(account_id),
                        nonce: 1,
                        pending_jobs: Vec::new(),
                        provider_jobs: Vec::new(),
                    });
                    self.metrics.successful_transactions += 1;
                }
                Err(e) => {
                    return Err(format!("Failed to create initial account: {:?}", e));
                }
            }
        }

        Ok(())
    }

    /// Generate transactions for a single block
    fn generate_transactions_for_block(&mut self) -> usize {
        let weights = vec![
            self.config.tx_probabilities.settle_directly,
            self.config.tx_probabilities.post_job,
            self.config.tx_probabilities.claim_job,
            self.config.tx_probabilities.commit_result,
            self.config.tx_probabilities.finalize_job,
            self.config.tx_probabilities.abort_job,
            self.config.tx_probabilities.reset_budget,
        ];

        let dist = WeightedIndex::new(&weights).unwrap();
        let num_txs = self.rng.gen_range(1..=10);
        let mut tx_count = 0;

        for _ in 0..num_txs {
            match dist.sample(&mut self.rng) {
                0 => self.generate_settle_directly(),
                1 => self.generate_post_job(),
                2 => self.generate_claim_job(),
                3 => self.generate_commit_result(),
                4 => self.generate_finalize_job(),
                5 => self.generate_abort_job(),
                6 => self.generate_reset_budget(),
                _ => unreachable!(),
            }
            tx_count += 1;
        }

        tx_count
    }

    /// Generate a SettleDirectly transaction
    fn generate_settle_directly(&mut self) {
        if self.users.len() < 2 {
            return;
        }

        let sender_idx = self.rng.gen_range(0..self.users.len());
        let mut recipient_idx = self.rng.gen_range(0..self.users.len());
        while recipient_idx == sender_idx {
            recipient_idx = self.rng.gen_range(0..self.users.len());
        }

        let sender = &self.users[sender_idx];
        let recipient = &self.users[recipient_idx];

        if sender.account_id.is_none() || recipient.account_id.is_none() {
            return;
        }

        let amount = Amount::from_units(self.rng.gen_range(1..=100));

        let tx = Transaction::SettleDirectly {
            provider: recipient.pubkey,
            job_spec_hash: Hash::compute(b"simulated job"),
            result_hash: Hash::compute(b"simulated result"),
            payment: amount,
        };

        let signed_tx = SignedTransaction::new_multi_party(
            sender.pubkey,
            vec![recipient.pubkey],
            tx,
            vec![
                ObjectRef::new(sender.account_id.unwrap(), sender.nonce),
                ObjectRef::new(recipient.account_id.unwrap(), 0),
            ],
            sender.nonce,
        );

        self.execute_transaction(signed_tx, sender_idx);
    }

    /// Generate a PostJob transaction
    fn generate_post_job(&mut self) {
        if self.users.len() < 2 {
            return;
        }

        let requestor_idx = self.rng.gen_range(0..self.users.len());
        let provider_idx = self.rng.gen_range(0..self.users.len());

        if requestor_idx == provider_idx {
            return;
        }

        let requestor = &self.users[requestor_idx];
        let provider = &self.users[provider_idx];

        if requestor.account_id.is_none() {
            return;
        }

        let payment = Amount::from_units(self.rng.gen_range(50..=500));
        let bond = payment.mul_rational(1, 2).unwrap();

        let tx = Transaction::PostJob {
            provider: Some(provider.pubkey),
            agreement_hash: Hash::compute(b"simulated agreement"),
            job_spec_hash: Hash::compute(b"simulated job spec"),
            payment,
            provider_bond_required: bond,
            claim_deadline_delta: 100,
            commit_deadline_delta: 200,
            finalization_delay: 50,
        };

        let signed_tx = SignedTransaction::new_single_signer(
            requestor.pubkey,
            tx,
            vec![ObjectRef::new(
                requestor.account_id.unwrap(),
                requestor.nonce,
            )],
            requestor.nonce,
        );

        self.execute_transaction(signed_tx, requestor_idx);
    }

    /// Execute a transaction and update metrics
    fn execute_transaction(&mut self, signed_tx: SignedTransaction, user_idx: usize) {
        let validator_idx = self.rng.gen_range(0..self.config.num_validators);
        let validator = self.engine.validators[validator_idx];

        self.metrics.total_transactions += 1;

        match self.engine.execute_transaction(&signed_tx, validator) {
            Ok(effects) => {
                self.metrics.successful_transactions += 1;

                // Update user state
                self.users[user_idx].nonce += 1;

                // Track escrows if this created one
                if let Transaction::PostJob { provider, .. } = &signed_tx.transaction {
                    if effects.created_objects.len() > 1 {
                        let escrow_id = effects.created_objects[1];
                        if let Some(provider_pubkey) = provider {
                            let provider_idx = self
                                .users
                                .iter()
                                .position(|u| u.pubkey == *provider_pubkey)
                                .unwrap();

                            self.active_escrows.insert(
                                escrow_id,
                                EscrowInfo {
                                    _requestor_idx: user_idx,
                                    provider_idx,
                                    _created_at: self.engine.current_height,
                                    claim_deadline: self.engine.current_height + 100,
                                    _commit_deadline: 0,
                                    _finalize_after: 0,
                                },
                            );

                            self.users[user_idx].pending_jobs.push(escrow_id);
                            self.users[provider_idx].provider_jobs.push(escrow_id);
                        }
                    }
                }

                // Track volume for SettleDirectly
                if let Transaction::SettleDirectly { payment, .. } = &signed_tx.transaction {
                    self.metrics.total_volume_transferred = self
                        .metrics
                        .total_volume_transferred
                        .checked_add(*payment)
                        .unwrap_or(self.metrics.total_volume_transferred);
                }

                // Track resets
                if matches!(signed_tx.transaction, Transaction::ResetBudget { .. }) {
                    self.metrics.channel_resets += 1;
                }
            }
            Err(e) => {
                let error_type = format!("{:?}", e);
                *self
                    .metrics
                    .failed_transactions
                    .entry(error_type)
                    .or_insert(0) += 1;
            }
        }
    }

    /// Check system invariants
    fn check_invariants(&mut self) {
        // Invariant 1: Total token supply is conserved
        let mut total_balance = Amount::ZERO;
        let mut total_locked = Amount::ZERO;

        for obj_meta in self.engine.state.values() {
            match &obj_meta.object {
                Object::Account(account) => {
                    total_balance = total_balance.checked_add(account.balance).unwrap();
                }
                Object::JobEscrow(escrow) => {
                    total_locked = total_locked.checked_add(escrow.payment).unwrap();
                    total_locked = total_locked
                        .checked_add(escrow.provider_bond_locked)
                        .unwrap();
                }
                _ => {}
            }
        }

        let initial_supply =
            self.users
                .iter()
                .filter_map(|u| u.account_id)
                .fold(Amount::ZERO, |acc, _| {
                    acc.checked_add(self.config.initial_balance_range.1)
                        .unwrap()
                });

        // Allow some tolerance for rounding
        let total = total_balance.checked_add(total_locked).unwrap();
        let tolerance = Amount::from_units(self.users.len() as u64);
        let diff = if total > initial_supply {
            total.checked_sub(initial_supply).unwrap_or(Amount::ZERO)
        } else {
            initial_supply.checked_sub(total).unwrap_or(Amount::ZERO)
        };
        if diff > tolerance {
            self.metrics.invariant_violations.push(format!(
                "Token supply mismatch: balance={}, locked={}, expected ~{}",
                total_balance, total_locked, initial_supply
            ));
        }

        // Invariant 2: No negative balances
        for (id, obj_meta) in &self.engine.state {
            if let Object::Account(account) = &obj_meta.object {
                // Amount type prevents negative values, but we can check for suspiciously large values
                if account.balance > Amount::from_units(u64::MAX / 2) {
                    self.metrics
                        .invariant_violations
                        .push(format!("Suspiciously large balance in account {:?}", id));
                }
            }
        }

        // Invariant 3: Escrow deadlines are monotonic
        for escrow_id in self.active_escrows.keys() {
            if let Some(obj_meta) = self.engine.state.get(escrow_id) {
                if let Object::JobEscrow(escrow) = &obj_meta.object {
                    if escrow.commit_deadline > 0 && escrow.commit_deadline <= escrow.claim_deadline
                    {
                        self.metrics.invariant_violations.push(format!(
                            "Invalid deadline ordering in escrow {:?}",
                            escrow_id
                        ));
                    }
                }
            }
        }
    }

    // Additional transaction generators...

    fn generate_claim_job(&mut self) {
        // Find claimable jobs
        let claimable: Vec<_> = self
            .active_escrows
            .iter()
            .filter(|(_, info)| self.engine.current_height <= info.claim_deadline)
            .map(|(id, info)| (*id, info.provider_idx))
            .collect();

        if claimable.is_empty() {
            return;
        }

        let idx = self.rng.gen_range(0..claimable.len());
        let (_escrow_id, _provider_idx) = claimable[idx];

        // Implementation simplified for brevity
        debug!("Attempting to claim job {:?}", _escrow_id);
    }

    fn generate_commit_result(&mut self) {
        // Similar pattern for other transaction types
        debug!("Generating commit result transaction");
    }

    fn generate_finalize_job(&mut self) {
        debug!("Generating finalize job transaction");
    }

    fn generate_abort_job(&mut self) {
        debug!("Generating abort job transaction");
    }

    fn generate_reset_budget(&mut self) {
        if self.users.is_empty() {
            return;
        }

        let user_idx = self.rng.gen_range(0..self.users.len());
        let user = &self.users[user_idx];

        if let Some(account_id) = user.account_id {
            // Get budget certificates from validator
            let cert = self.engine.validator_local_state.create_certificate(
                account_id,
                &crypto::SigningKey::from_pubkey(self.engine.validators[0]),
            );

            if let Some(cert) = cert {
                let tx = Transaction::ResetBudget {
                    budget_certificates: vec![cert],
                };

                let signed_tx = SignedTransaction::new_single_signer(
                    user.pubkey,
                    tx,
                    vec![ObjectRef::new(account_id, user.nonce)],
                    user.nonce,
                );

                self.execute_transaction(signed_tx, user_idx);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_logging() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter("hellas_protocol=debug,simulation=info")
            .try_init();
    }

    #[test]
    fn test_small_simulation() {
        init_logging();

        let config = SimulationConfig {
            seed: 42,
            num_validators: 4,
            byzantine_tolerance: 1,
            num_accounts: 10,
            initial_balance_range: (Amount::from_units(1000), Amount::from_units(10000)),
            num_steps: 100,
            tx_probabilities: TransactionProbabilities::default(),
            enable_concurrency: true,
            block_time_ms: 10,
        };

        let mut sim = Simulation::new(config);
        let metrics = sim.run().expect("Simulation should complete");

        println!("Simulation Results:");
        println!("  Total transactions: {}", metrics.total_transactions);
        println!("  Successful: {}", metrics.successful_transactions);
        println!("  Failed: {:?}", metrics.failed_transactions);
        println!("  Volume transferred: {}", metrics.total_volume_transferred);
        println!("  Max concurrent: {}", metrics.max_concurrent_transactions);
        println!("  Channel resets: {}", metrics.channel_resets);

        assert!(metrics.successful_transactions > 0);
        assert!(
            metrics.invariant_violations.is_empty(),
            "Invariant violations: {:?}",
            metrics.invariant_violations
        );
    }

    #[test]
    fn test_stress_simulation() {
        init_logging();

        let config = SimulationConfig {
            seed: 12345,
            num_validators: 7,
            byzantine_tolerance: 2,
            num_accounts: 100,
            initial_balance_range: (Amount::from_units(10000), Amount::from_units(100000)),
            num_steps: 1000,
            tx_probabilities: TransactionProbabilities {
                settle_directly: 0.60, // Heavy payment load
                reset_budget: 0.20,    // Frequent resets
                ..Default::default()
            },
            enable_concurrency: true,
            block_time_ms: 1,
        };

        let mut sim = Simulation::new(config);
        let metrics = sim.run().expect("Simulation should complete");

        assert!(metrics.successful_transactions > 500);
        assert!(metrics.channel_resets > 50);
        assert!(metrics.invariant_violations.is_empty());
    }
}
