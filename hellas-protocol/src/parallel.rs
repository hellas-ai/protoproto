//! # Parallel Transaction Execution
//!
//! This module implements parallel execution of transactions using copy-on-write
//! snapshots to avoid the mutex bottleneck. Each parallel thread works on its own
//! transactional snapshot, and changes are validated and atomically committed after
//! execution.
//!
//! ## The Algorithm
//!
//! 1. **Dependency Analysis**: Build a graph of transaction dependencies based
//!    on which objects they read/write
//! 2. **Snapshot Creation**: Create copy-on-write snapshots for each parallel group
//! 3. **Parallel Execution**: Run independent transactions on separate snapshots
//! 4. **Conflict Detection**: Validate that no conflicts occurred between parallel executions
//! 5. **Atomic Commit**: Apply all validated changes to the main state
//!
//! ## Performance Benefits
//!
//! On a machine with N cores, we can achieve near N× speedup for workloads
//! with many independent transactions. The snapshot-based approach eliminates
//! the global lock bottleneck and enables true parallel execution.

use crate::engine::{ExecutionError, StateTransitionEngine};
use crate::objects::ObjectMetadata;
use crate::transactions::{SignedTransaction, TransactionEffects};
use crate::types::{ObjectId, Pubkey, Version};
use parking_lot::RwLock;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// A batch of transactions to execute in parallel
#[derive(Debug, Clone)]
pub struct TransactionBatch {
    pub transactions: Vec<SignedTransaction>,
    pub proposing_validator: Pubkey,
}

/// Result of parallel execution
pub struct ParallelExecutionResult {
    /// Effects for each transaction (in order)
    pub effects: Vec<Result<TransactionEffects, ExecutionError>>,

    /// Total time taken
    pub execution_time_ms: u64,

    /// Number of parallel rounds needed
    pub rounds: usize,
}

/// A snapshot of the state at a specific point in time
#[derive(Clone)]
struct StateSnapshot {
    /// Copy-on-write state: all objects in the system
    state: HashMap<ObjectId, ObjectMetadata>,

    /// Current block height
    current_height: u64,

    /// Version of this snapshot for conflict detection
    _snapshot_version: u64,
}

impl StateSnapshot {
    /// Create a new snapshot from the current state
    fn from_engine(engine: &StateTransitionEngine, version: u64) -> Self {
        Self {
            state: engine.state.clone(),
            current_height: engine.current_height,
            _snapshot_version: version,
        }
    }
}

/// Changes made during transaction execution
#[derive(Debug, Clone)]
struct StateChanges {
    /// Objects that were consumed (with their versions before consumption)
    consumed: HashMap<ObjectId, Version>,

    /// Objects that were created or modified
    modified: HashMap<ObjectId, ObjectMetadata>,

    /// New objects that were created
    created: HashSet<ObjectId>,
}

impl StateChanges {
    fn new() -> Self {
        Self {
            consumed: HashMap::new(),
            modified: HashMap::new(),
            created: HashSet::new(),
        }
    }

    /// Record that an object was consumed
    fn consume_object(&mut self, id: ObjectId, version: Version) {
        self.consumed.insert(id, version);
    }

    /// Record that an object was modified
    fn modify_object(&mut self, metadata: ObjectMetadata) {
        let id = metadata.id;
        self.modified.insert(id, metadata);
        if !self.consumed.contains_key(&id) {
            // This is a new object
            self.created.insert(id);
        }
    }

    /// Get the write set (all objects that were modified or created)
    fn write_set(&self) -> HashSet<ObjectId> {
        self.modified.keys().cloned().collect()
    }
}

/// Execution context for a single transaction
struct ExecutionContext {
    /// The snapshot this execution is working on
    snapshot: StateSnapshot,

    /// Changes made during execution
    changes: StateChanges,

    /// The transaction being executed
    transaction: SignedTransaction,

    /// The proposing validator
    proposing_validator: Pubkey,
}

impl ExecutionContext {
    fn new(
        snapshot: StateSnapshot,
        transaction: SignedTransaction,
        proposing_validator: Pubkey,
    ) -> Self {
        Self {
            snapshot,
            changes: StateChanges::new(),
            transaction,
            proposing_validator,
        }
    }

    /// Execute the transaction in this context
    fn execute(
        &mut self,
        base_engine: &StateTransitionEngine,
    ) -> Result<TransactionEffects, ExecutionError> {
        // Create a temporary engine with our snapshot state
        let mut temp_engine = StateTransitionEngine {
            state: self.snapshot.state.clone(),
            current_height: self.snapshot.current_height,
            validators: base_engine.validators.clone(),
            byzantine_tolerance: base_engine.byzantine_tolerance,
            channel_manager: base_engine.channel_manager.clone(),
            validator_local_state: base_engine.validator_local_state.clone(),
            object_counter: base_engine.object_counter,
            account_lookup: base_engine.account_lookup.clone(),
        };

        // Execute the transaction
        let effects =
            temp_engine.execute_transaction(&self.transaction, self.proposing_validator)?;

        // Record all changes
        for consumed_ref in &effects.consumed_objects {
            self.changes
                .consume_object(consumed_ref.object_id, consumed_ref.version);
        }

        // Find all modified/created objects by comparing states
        for (id, metadata) in temp_engine.state {
            if let Some(original) = self.snapshot.state.get(&id) {
                if original.version != metadata.version {
                    // Object was modified
                    self.changes.modify_object(metadata);
                }
            } else {
                // Object was created
                self.changes.modify_object(metadata);
            }
        }

        Ok(effects)
    }
}

/// Dependency graph for transactions
#[derive(Debug)]
struct DependencyGraph {
    /// Number of transactions
    num_transactions: usize,

    /// Adjacency list: transaction i depends on transactions in dependencies[i]
    dependencies: Vec<Vec<usize>>,

    /// Reverse dependencies: transactions that depend on transaction i
    dependents: Vec<Vec<usize>>,
}

impl DependencyGraph {
    /// Build a dependency graph from a batch of transactions
    fn build(transactions: &[SignedTransaction]) -> Self {
        let n = transactions.len();
        let mut dependencies = vec![vec![]; n];
        let mut dependents = vec![vec![]; n];

        // Extract read/write sets for each transaction
        let mut read_sets = Vec::with_capacity(n);
        let mut write_sets = Vec::with_capacity(n);

        for tx in transactions {
            let mut reads = HashSet::new();
            let mut writes = HashSet::new();

            // All input objects are read
            for input in &tx.input_objects {
                reads.insert(input.object_id);
            }

            // For a conservative estimate, assume all read objects are also written
            // In practice, we'd analyze each transaction type separately
            writes.extend(&reads);

            read_sets.push(reads);
            write_sets.push(writes);
        }

        // Build dependency edges
        for i in 0..n {
            for j in 0..i {
                // Transaction i depends on j if:
                // - j writes an object that i reads or writes
                let conflict = write_sets[j]
                    .iter()
                    .any(|obj| read_sets[i].contains(obj) || write_sets[i].contains(obj));

                if conflict {
                    dependencies[i].push(j);
                    dependents[j].push(i);
                }
            }
        }

        Self {
            num_transactions: n,
            dependencies,
            dependents,
        }
    }

    /// Find groups of transactions that can execute in parallel
    fn find_parallel_groups(&self) -> Vec<Vec<usize>> {
        let mut groups = Vec::new();
        let mut executed = vec![false; self.num_transactions];
        let mut in_degree = vec![0; self.num_transactions];

        // Calculate in-degrees
        for (i, deps) in self.dependencies.iter().enumerate() {
            in_degree[i] = deps.len();
        }

        // Repeatedly find transactions with no unexecuted dependencies
        loop {
            let mut current_group = Vec::new();

            // Find all transactions with in-degree 0
            for i in 0..self.num_transactions {
                if !executed[i] && in_degree[i] == 0 {
                    current_group.push(i);
                }
            }

            if current_group.is_empty() {
                break;
            }

            // Mark as executed and update in-degrees
            for &tx_idx in &current_group {
                executed[tx_idx] = true;
                for &dependent in &self.dependents[tx_idx] {
                    if !executed[dependent] {
                        in_degree[dependent] -= 1;
                    }
                }
            }

            groups.push(current_group);
        }

        groups
    }
}

/// Conflict detector for validating parallel executions
struct ConflictDetector {
    /// Track which objects were read at which versions
    read_versions: HashMap<ObjectId, HashSet<(usize, Version)>>,

    /// Track which objects were written by which transactions
    write_sets: HashMap<ObjectId, Vec<usize>>,
}

impl ConflictDetector {
    fn new() -> Self {
        Self {
            read_versions: HashMap::new(),
            write_sets: HashMap::new(),
        }
    }

    /// Record reads and writes from an execution context
    fn record_execution(&mut self, tx_idx: usize, changes: &StateChanges) {
        // Record reads
        for (obj_id, version) in &changes.consumed {
            self.read_versions
                .entry(*obj_id)
                .or_default()
                .insert((tx_idx, *version));
        }

        // Record writes
        for obj_id in changes.write_set() {
            self.write_sets.entry(obj_id).or_default().push(tx_idx);
        }
    }

    /// Check if there are any conflicts between parallel executions
    fn has_conflicts(&self) -> bool {
        // Check write-write conflicts
        for writers in self.write_sets.values() {
            if writers.len() > 1 {
                return true; // Multiple writers to same object
            }
        }

        // Check read-write conflicts where read happened at wrong version
        for (obj_id, readers) in &self.read_versions {
            if let Some(writers) = self.write_sets.get(obj_id) {
                for (reader_idx, _read_version) in readers {
                    for writer_idx in writers {
                        if reader_idx > writer_idx {
                            // Reader should have seen the write, conflict!
                            return true;
                        }
                    }
                }
            }
        }

        false
    }
}

/// Parallel execution engine with snapshot-based isolation
pub struct ParallelExecutor {
    /// The underlying state transition engine (behind RwLock for safe concurrent access)
    engine: Arc<RwLock<StateTransitionEngine>>,

    /// Snapshot version counter
    snapshot_counter: Arc<RwLock<u64>>,
}

impl ParallelExecutor {
    /// Create a new parallel executor
    pub fn new(engine: StateTransitionEngine) -> Self {
        Self {
            engine: Arc::new(RwLock::new(engine)),
            snapshot_counter: Arc::new(RwLock::new(0)),
        }
    }

    /// Execute a batch of transactions with maximum parallelism
    pub fn execute_batch(&self, batch: TransactionBatch) -> ParallelExecutionResult {
        let start_time = std::time::Instant::now();
        let mut all_effects: Vec<Option<Result<TransactionEffects, ExecutionError>>> =
            (0..batch.transactions.len()).map(|_| None).collect();

        // Build dependency graph
        let dep_graph = DependencyGraph::build(&batch.transactions);
        let parallel_groups = dep_graph.find_parallel_groups();
        let rounds = parallel_groups.len();

        // Execute each group in parallel
        for group in parallel_groups {
            // Create a snapshot for this round
            let snapshot = {
                let engine = self.engine.read();
                let mut counter = self.snapshot_counter.write();
                *counter += 1;
                StateSnapshot::from_engine(&engine, *counter)
            };

            // Execute transactions in parallel
            let group_results: Vec<(
                usize,
                ExecutionContext,
                Result<TransactionEffects, ExecutionError>,
            )> = group
                .into_par_iter()
                .map(|tx_idx| {
                    let mut context = ExecutionContext::new(
                        snapshot.clone(),
                        batch.transactions[tx_idx].clone(),
                        batch.proposing_validator,
                    );

                    let engine = self.engine.read();
                    let result = context.execute(&engine);

                    (tx_idx, context, result)
                })
                .collect();

            // Check for conflicts within the group
            let mut conflict_detector = ConflictDetector::new();
            let mut has_conflicts = false;

            for (tx_idx, context, result) in &group_results {
                if result.is_ok() {
                    conflict_detector.record_execution(*tx_idx, &context.changes);
                }
            }

            if conflict_detector.has_conflicts() {
                // Fall back to sequential execution for this group
                has_conflicts = true;
            }

            if !has_conflicts {
                // No conflicts, apply all changes atomically
                let mut engine = self.engine.write();

                for (tx_idx, context, result) in group_results {
                    if let Ok(effects) = result {
                        // Apply changes to the main state
                        for (id, metadata) in context.changes.modified {
                            engine.state.insert(id, metadata);
                        }

                        all_effects[tx_idx] = Some(Ok(effects));
                    } else {
                        all_effects[tx_idx] = Some(result);
                    }
                }
            } else {
                // Conflicts detected, execute sequentially
                let mut engine = self.engine.write();

                for (tx_idx, _, _) in group_results {
                    let tx = &batch.transactions[tx_idx];
                    let result = engine.execute_transaction(tx, batch.proposing_validator);
                    all_effects[tx_idx] = Some(result);
                }
            }
        }

        // Convert Option<Result> to Result
        let effects = all_effects.into_iter().map(|opt| opt.unwrap()).collect();

        ParallelExecutionResult {
            effects,
            execution_time_ms: start_time.elapsed().as_millis() as u64,
            rounds,
        }
    }

    /// Execute a batch with optimistic concurrency control
    pub fn execute_batch_optimistic(&self, batch: TransactionBatch) -> ParallelExecutionResult {
        let start_time = std::time::Instant::now();

        // Try to execute all transactions in parallel optimistically
        let snapshot = {
            let engine = self.engine.read();
            let mut counter = self.snapshot_counter.write();
            *counter += 1;
            StateSnapshot::from_engine(&engine, *counter)
        };

        // Execute all transactions in parallel
        let results: Vec<(
            usize,
            ExecutionContext,
            Result<TransactionEffects, ExecutionError>,
        )> = (0..batch.transactions.len())
            .into_par_iter()
            .map(|tx_idx| {
                let mut context = ExecutionContext::new(
                    snapshot.clone(),
                    batch.transactions[tx_idx].clone(),
                    batch.proposing_validator,
                );

                let engine = self.engine.read();
                let result = context.execute(&engine);

                (tx_idx, context, result)
            })
            .collect();

        // Detect conflicts
        let mut conflict_detector = ConflictDetector::new();
        let mut successful_txs = Vec::new();
        let mut failed_txs = Vec::new();

        for (tx_idx, context, result) in results {
            if let Ok(effects) = result {
                conflict_detector.record_execution(tx_idx, &context.changes);
                successful_txs.push((tx_idx, context, effects));
            } else {
                failed_txs.push((tx_idx, result));
            }
        }

        // Apply successful transactions if no conflicts
        let mut final_effects: Vec<Option<Result<TransactionEffects, ExecutionError>>> =
            (0..batch.transactions.len()).map(|_| None).collect();

        if !conflict_detector.has_conflicts() {
            // No conflicts, apply all changes atomically
            let mut engine = self.engine.write();

            for (tx_idx, context, effects) in successful_txs {
                // Apply changes to the main state
                for (id, metadata) in context.changes.modified {
                    engine.state.insert(id, metadata);
                }

                final_effects[tx_idx] = Some(Ok(effects));
            }

            for (tx_idx, result) in failed_txs {
                final_effects[tx_idx] = Some(result);
            }
        } else {
            // Conflicts detected, fall back to dependency-based execution
            return self.execute_batch(batch);
        }

        // Convert Option<Result> to Result
        let effects = final_effects.into_iter().map(|opt| opt.unwrap()).collect();

        ParallelExecutionResult {
            effects,
            execution_time_ms: start_time.elapsed().as_millis() as u64,
            rounds: 1, // Optimistic execution is always 1 round
        }
    }

    /// Analyze a batch to estimate parallelism opportunity
    pub fn analyze_parallelism(batch: &TransactionBatch) -> ParallelismAnalysis {
        let dep_graph = DependencyGraph::build(&batch.transactions);
        let parallel_groups = dep_graph.find_parallel_groups();

        let max_parallelism = parallel_groups
            .iter()
            .map(|group| group.len())
            .max()
            .unwrap_or(0);

        let total_conflicts = dep_graph.dependencies.iter().map(|deps| deps.len()).sum();

        ParallelismAnalysis {
            total_transactions: batch.transactions.len(),
            parallel_rounds: parallel_groups.len(),
            max_parallelism,
            total_conflicts,
            groups: parallel_groups,
        }
    }
}

/// Analysis of parallelism opportunity in a batch
#[derive(Debug)]
pub struct ParallelismAnalysis {
    /// Total number of transactions
    pub total_transactions: usize,

    /// Number of sequential rounds needed
    pub parallel_rounds: usize,

    /// Maximum number of transactions in any round
    pub max_parallelism: usize,

    /// Total number of dependency edges
    pub total_conflicts: usize,

    /// The actual grouping
    pub groups: Vec<Vec<usize>>,
}

impl ParallelismAnalysis {
    /// Calculate the theoretical speedup from parallelization
    pub fn speedup(&self, num_cores: usize) -> f64 {
        if self.total_transactions == 0 {
            return 1.0;
        }

        // Sequential time: process all transactions one by one
        let sequential_time = self.total_transactions as f64;

        // Parallel time: sum of ceiling(group_size / num_cores) for each group
        let parallel_time: f64 = self
            .groups
            .iter()
            .map(|group| {
                let group_size = group.len() as f64;
                (group_size / num_cores as f64).ceil()
            })
            .sum();

        sequential_time / parallel_time
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transactions::{ObjectRef, Transaction};

    fn create_test_engine() -> StateTransitionEngine {
        let validators = vec![
            Pubkey::test(1),
            Pubkey::test(2),
            Pubkey::test(3),
            Pubkey::test(4),
        ];
        StateTransitionEngine::new(validators, 1)
    }

    #[test]
    fn test_parallel_execution_no_conflicts() {
        let mut engine = create_test_engine();

        // Create some accounts first
        let creators = vec![
            Pubkey::test(10),
            Pubkey::test(11),
            Pubkey::test(12),
            Pubkey::test(13),
        ];

        let mut account_ids = Vec::new();
        for creator in &creators {
            let tx = Transaction::CreateAccount {
                initial_balance: crate::types::Amount::from_units(1000),
            };
            let signed_tx = SignedTransaction::new_single_signer(*creator, tx, vec![], 0);
            let effects = engine
                .execute_transaction(&signed_tx, Pubkey::test(1))
                .unwrap();
            account_ids.push(effects.created_objects[0]);
        }

        // Create parallel executor
        let executor = ParallelExecutor::new(engine);

        // Create independent transactions (different accounts)
        let mut transactions = Vec::new();
        for (i, creator) in creators.iter().enumerate() {
            let tx = Transaction::CreateAccount {
                initial_balance: crate::types::Amount::from_units(2000 + i as u64),
            };
            let signed_tx = SignedTransaction::new_single_signer(*creator, tx, vec![], 1);
            transactions.push(signed_tx);
        }

        let batch = TransactionBatch {
            transactions,
            proposing_validator: Pubkey::test(1),
        };

        // Execute in parallel
        let result = executor.execute_batch(batch);

        // All should succeed
        assert_eq!(result.effects.len(), 4);
        for effect in &result.effects {
            assert!(effect.is_ok());
        }

        // Should execute in 1 round since no conflicts
        assert_eq!(result.rounds, 1);
    }

    #[test]
    fn test_parallel_execution_with_conflicts() {
        let mut engine = create_test_engine();

        // Create an account
        let creator = Pubkey::test(10);
        let tx = Transaction::CreateAccount {
            initial_balance: crate::types::Amount::from_units(5000),
        };
        let signed_tx = SignedTransaction::new_single_signer(creator, tx, vec![], 0);
        let effects = engine
            .execute_transaction(&signed_tx, Pubkey::test(1))
            .unwrap();
        let account_id = effects.created_objects[0];

        // Create parallel executor
        let executor = ParallelExecutor::new(engine);

        // Create conflicting transactions (same account)
        let mut transactions = Vec::new();
        for i in 0..4 {
            let tx = Transaction::CreateAccount {
                initial_balance: crate::types::Amount::from_units(1000 + i),
            };
            let signed_tx = SignedTransaction::new_single_signer(
                creator,
                tx,
                vec![ObjectRef::new(account_id, 0)], // All reference same object
                i + 1,
            );
            transactions.push(signed_tx);
        }

        let batch = TransactionBatch {
            transactions,
            proposing_validator: Pubkey::test(1),
        };

        // Execute in parallel
        let result = executor.execute_batch(batch);

        // Should handle conflicts correctly
        assert_eq!(result.effects.len(), 4);
        // With conflicts, transactions will be serialized
        assert!(result.rounds > 1);
    }

    #[test]
    fn test_optimistic_execution() {
        let engine = create_test_engine();
        let executor = ParallelExecutor::new(engine);

        // Create independent transactions
        let mut transactions = Vec::new();
        for i in 0..10 {
            let creator = Pubkey::test(20 + i);
            let tx = Transaction::CreateAccount {
                initial_balance: crate::types::Amount::from_units(1000 * (i as u64 + 1)),
            };
            let signed_tx = SignedTransaction::new_single_signer(creator, tx, vec![], 0);
            transactions.push(signed_tx);
        }

        let batch = TransactionBatch {
            transactions,
            proposing_validator: Pubkey::test(1),
        };

        // Execute optimistically
        let result = executor.execute_batch_optimistic(batch);

        // All should succeed in 1 round
        assert_eq!(result.effects.len(), 10);
        for effect in &result.effects {
            assert!(effect.is_ok());
        }
        assert_eq!(result.rounds, 1);
    }

    #[test]
    fn test_dependency_graph() {
        // Create some test transactions
        let obj1 = ObjectId::new([1; 32]);
        let obj2 = ObjectId::new([2; 32]);
        let _obj3 = ObjectId::new([3; 32]);

        let tx1 = SignedTransaction::new_single_signer(
            Pubkey::test(1),
            Transaction::CreateAccount {
                initial_balance: crate::types::Amount::from_units(100),
            },
            vec![ObjectRef::new(obj1, 0)],
            0,
        );

        let tx2 = SignedTransaction::new_single_signer(
            Pubkey::test(2),
            Transaction::CreateAccount {
                initial_balance: crate::types::Amount::from_units(200),
            },
            vec![ObjectRef::new(obj2, 0)],
            0,
        );

        let tx3 = SignedTransaction::new_single_signer(
            Pubkey::test(3),
            Transaction::CreateAccount {
                initial_balance: crate::types::Amount::from_units(300),
            },
            vec![ObjectRef::new(obj1, 1)], // Depends on tx1
            0,
        );

        let transactions = vec![tx1, tx2, tx3];
        let graph = DependencyGraph::build(&transactions);

        // tx3 depends on tx1, but tx2 is independent
        assert_eq!(graph.dependencies[2], vec![0]); // tx3 depends on tx1
        assert_eq!(graph.dependencies[1], Vec::<usize>::new()); // tx2 has no dependencies

        let groups = graph.find_parallel_groups();
        assert_eq!(groups.len(), 2); // Two rounds needed
        assert_eq!(groups[0].len(), 2); // tx1 and tx2 in parallel
        assert_eq!(groups[1], vec![2]); // tx3 alone
    }

    #[test]
    fn test_parallelism_analysis() {
        let batch = TransactionBatch {
            transactions: vec![
                // 4 independent transactions
                SignedTransaction::new_single_signer(
                    Pubkey::test(1),
                    Transaction::CreateAccount {
                        initial_balance: crate::types::Amount::from_units(100),
                    },
                    vec![],
                    0,
                ),
                SignedTransaction::new_single_signer(
                    Pubkey::test(2),
                    Transaction::CreateAccount {
                        initial_balance: crate::types::Amount::from_units(100),
                    },
                    vec![],
                    0,
                ),
                SignedTransaction::new_single_signer(
                    Pubkey::test(3),
                    Transaction::CreateAccount {
                        initial_balance: crate::types::Amount::from_units(100),
                    },
                    vec![],
                    0,
                ),
                SignedTransaction::new_single_signer(
                    Pubkey::test(4),
                    Transaction::CreateAccount {
                        initial_balance: crate::types::Amount::from_units(100),
                    },
                    vec![],
                    0,
                ),
            ],
            proposing_validator: Pubkey::test(10),
        };

        let analysis = ParallelExecutor::analyze_parallelism(&batch);

        assert_eq!(analysis.total_transactions, 4);
        assert_eq!(analysis.parallel_rounds, 1); // All can run in parallel
        assert_eq!(analysis.max_parallelism, 4);
        assert_eq!(analysis.total_conflicts, 0);

        // With 4 cores, we get 4x speedup
        assert_eq!(analysis.speedup(4), 4.0);

        // With 2 cores, we get 2x speedup
        assert_eq!(analysis.speedup(2), 2.0);
    }
}
