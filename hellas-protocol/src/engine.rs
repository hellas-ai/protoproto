//! # State Transition Engine
//!
//! This module implements the core execution engine for the Hellas protocol,
//! synthesizing concepts from Sui Lutris and Stingray with the Morpheus consensus protocol.
//!
//! ## Unified Architecture
//!
//! The key insight is that Morpheus is not a fallback consensus mechanism; its DAG structure
//! naturally expresses both a low-latency fast path and a robust slow path:
//!
//! - **Fast Path**: Transaction blocks achieve 1-QCs and execute speculatively
//! - **Slow Path**: Leader blocks with 2-QCs impose total ordering on conflicts
//!
//! ## Key Concepts
//!
//! 1. **Object Model**: All state is organized as discrete objects with unique IDs
//! 2. **Object Versioning**: Each object has a version, forming (ObjID, Version) pairs  
//! 3. **Owned vs Shared Objects**:
//!    - Owned objects can execute speculatively on the fast path
//!    - Shared objects always require consensus ordering
//! 4. **Speculative Execution**: Transactions execute on snapshots when 1-QC is formed
//! 5. **State Reconciliation**: Conflicts are resolved when leader blocks achieve 2-QCs
//!
//! ## Design Principles
//!
//! - **Deterministic**: Same inputs always produce same outputs
//! - **Atomic**: Transactions fully succeed or fully fail
//! - **Isolated**: Transactions see a consistent snapshot of state
//! - **Parallelizable**: Non-conflicting transactions can execute concurrently
//! - **Speculative**: Fast path enables optimistic execution with rollback capability

use crate::bounded_counter::{BoundedCounterManager, ValidatorLocalState};
use crate::objects::{HellasAccount, JobEscrow, JobStatus, Object, ObjectMetadata};
use crate::observability::{self, Timer, TransactionType};
use crate::transactions::{
    ObjectRef, SignedTransaction, Transaction, TransactionCertificate, TransactionEffects,
    EffectsCertificate, BudgetCertificate,
};
use crate::types::{Amount, BlockHeight, Hash, ObjectId, Pubkey, Version, TransactionDigest};
use metrics::{counter, gauge};
use rpds::{HashTrieMap, HashTrieSet};
use std::collections::{HashMap, HashSet, BTreeMap};
use std::sync::Arc;
use std::time::Instant;
use thiserror::Error;
use tracing::{debug, info, instrument, trace, warn};

/// Key for an object at a specific version
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ObjectKey {
    pub id: ObjectId,
    pub version: Version,
}

impl ObjectKey {
    pub fn new(id: ObjectId, version: Version) -> Self {
        Self { id, version }
    }
}

/// Lock status for owned objects
#[derive(Debug, Clone)]
pub enum OwnedLockStatus {
    /// No transaction has claimed this object version
    None,
    /// A transaction has locked this object version
    Locked(TransactionDigest),
}

/// Lock information for shared objects
#[derive(Debug, Clone)]
pub struct SharedLockInfo {
    /// The version assigned to a transaction for this object
    pub assigned_version: Version,
    /// Whether this lock has been executed
    pub executed: bool,
}

/// Represents the execution state of a block
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockExecutionStatus {
    /// Block has been speculatively executed
    Speculative,
    /// Block has been finalized by consensus
    Finalized,
}

/// Information about an executed block
#[derive(Debug, Clone)]
pub struct ExecutedBlockInfo {
    /// The block's unique identifier (corresponds to Morpheus BlockKey)
    pub block_key: Hash,
    /// The execution status
    pub status: BlockExecutionStatus,
    /// The transactions in the block
    pub transactions: Vec<TransactionDigest>,
    /// The state version after executing this block
    pub state_version: u64,
}

/// A snapshot of state changes for rollback capability
#[derive(Debug, Clone)]
pub struct StateSnapshot {
    /// The version of this snapshot
    pub version: u64,
    /// Objects that were modified (old versions for rollback)
    pub original_objects: HashMap<ObjectKey, Option<ObjectMetadata>>,
    /// Locks that were acquired
    pub acquired_locks: HashSet<ObjectKey>,
    /// New objects that were created
    pub created_objects: HashSet<ObjectId>,
}

/// Errors that can occur during transaction execution
#[derive(Debug, Error, PartialEq)]
pub enum ExecutionError {
    #[error("Object not found: {0}")]
    ObjectNotFound(ObjectId),

    #[error("Version mismatch for object {0}: expected {1}, found {2}")]
    VersionMismatch(ObjectId, Version, Version),

    #[error("Object {0} version {1} is already locked by another transaction")]
    ObjectLocked(ObjectId, Version),

    #[error("Insufficient balance: have {have}, need {need}")]
    InsufficientBalance { have: Amount, need: Amount },

    #[error("Invalid signature")]
    InvalidSignature,

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Invalid transaction: {0}")]
    InvalidTransaction(String),

    #[error("Certificate error: {0}")]
    CertificateError(String),

    #[error("Missing required signature from {0}")]
    MissingSignature(Pubkey),

    #[error("Transaction {0} not found")]
    TransactionNotFound(TransactionDigest),

    #[error("Consensus required for shared objects")]
    ConsensusRequired,
}

/// The main state transition engine with Sui Lutris-inspired architecture
pub struct StateTransitionEngine {
    /// The world state: persistent map of objects using rpds for cheap snapshots
    pub objects: Arc<HashTrieMap<ObjectKey, ObjectMetadata>>,

    /// Latest version of each object (for efficient lookups)
    pub latest_versions: Arc<HashTrieMap<ObjectId, Version>>,

    /// Locks for owned objects - prevents double spending
    pub owned_locks: Arc<HashTrieMap<ObjectKey, OwnedLockStatus>>,

    /// Locks for shared objects - managed by consensus
    pub shared_locks: Arc<HashTrieMap<(TransactionDigest, ObjectId), SharedLockInfo>>,

    /// Next available version for shared objects
    pub next_shared_version: Arc<HashTrieMap<ObjectId, Version>>,

    /// Executed certificates and their effects
    pub certificates: Arc<HashTrieMap<TransactionDigest, (TransactionCertificate, TransactionEffects)>>,

    /// Current block height
    pub current_height: BlockHeight,

    /// Set of active validators
    pub validators: Vec<Pubkey>,

    /// Byzantine fault tolerance parameter
    pub byzantine_tolerance: usize,

    /// Bounded counter execution manager
    pub channel_manager: BoundedCounterManager,

    /// This validator's local state
    pub validator_local_state: ValidatorLocalState,

    /// Counter for generating unique object IDs
    pub object_counter: u64,

    /// Mapping from pubkey to account ID
    pub account_lookup: Arc<HashTrieMap<Pubkey, ObjectId>>,

    /// Tracks which blocks have been executed and their status
    pub executed_blocks: Arc<HashTrieMap<Hash, ExecutedBlockInfo>>,

    /// Current state version (incremented with each state change)
    pub state_version: u64,

    /// Active snapshots for speculative execution
    pub active_snapshots: HashMap<Hash, StateSnapshot>,

    /// Maps object versions to the blocks that created them (for rollback)
    pub version_to_block: Arc<HashTrieMap<ObjectKey, Hash>>,
}

impl StateTransitionEngine {
    /// Create a new state transition engine
    pub fn new(validators: Vec<Pubkey>, byzantine_tolerance: usize) -> Self {
        Self {
            objects: Arc::new(HashTrieMap::new()),
            latest_versions: Arc::new(HashTrieMap::new()),
            owned_locks: Arc::new(HashTrieMap::new()),
            shared_locks: Arc::new(HashTrieMap::new()),
            next_shared_version: Arc::new(HashTrieMap::new()),
            certificates: Arc::new(HashTrieMap::new()),
            current_height: 0,
            validators: validators.clone(),
            byzantine_tolerance,
            channel_manager: BoundedCounterManager::new(validators.clone(), byzantine_tolerance),
            validator_local_state: ValidatorLocalState::new(validators[0]),
            object_counter: 0,
            account_lookup: Arc::new(HashTrieMap::new()),
            executed_blocks: Arc::new(HashTrieMap::new()),
            state_version: 0,
            active_snapshots: HashMap::new(),
            version_to_block: Arc::new(HashTrieMap::new()),
        }
    }

    /// Create a snapshot of the current state for speculative execution
    pub fn create_snapshot(&self) -> Self {
        Self {
            objects: Arc::clone(&self.objects),
            latest_versions: Arc::clone(&self.latest_versions),
            owned_locks: Arc::clone(&self.owned_locks),
            shared_locks: Arc::clone(&self.shared_locks),
            next_shared_version: Arc::clone(&self.next_shared_version),
            certificates: Arc::clone(&self.certificates),
            current_height: self.current_height,
            validators: self.validators.clone(),
            byzantine_tolerance: self.byzantine_tolerance,
            channel_manager: self.channel_manager.clone(),
            validator_local_state: self.validator_local_state.clone(),
            object_counter: self.object_counter,
            account_lookup: Arc::clone(&self.account_lookup),
            executed_blocks: Arc::clone(&self.executed_blocks),
            state_version: self.state_version,
            active_snapshots: self.active_snapshots.clone(),
            version_to_block: Arc::clone(&self.version_to_block),
        }
    }

    /// Begin a speculative execution for a block
    pub fn begin_speculative_execution(&mut self, block_key: Hash) -> StateSnapshot {
        let snapshot = StateSnapshot {
            version: self.state_version,
            original_objects: HashMap::new(),
            acquired_locks: HashSet::new(),
            created_objects: HashSet::new(),
        };
        self.active_snapshots.insert(block_key, snapshot.clone());
        snapshot
    }

    /// Commit a speculative execution snapshot back to the main state
    pub fn commit_snapshot(&mut self, snapshot_engine: StateTransitionEngine, block_key: Hash) -> Result<(), ExecutionError> {
        // Remove the active snapshot
        self.active_snapshots.remove(&block_key);

        // Update state version
        self.state_version = snapshot_engine.state_version;

        // Commit all state changes atomically
        self.objects = snapshot_engine.objects;
        self.latest_versions = snapshot_engine.latest_versions;
        self.owned_locks = snapshot_engine.owned_locks;
        self.shared_locks = snapshot_engine.shared_locks;
        self.next_shared_version = snapshot_engine.next_shared_version;
        self.certificates = snapshot_engine.certificates;
        self.account_lookup = snapshot_engine.account_lookup;
        self.version_to_block = snapshot_engine.version_to_block;
        self.object_counter = snapshot_engine.object_counter;

        // Mark the block as speculatively executed
        let block_info = ExecutedBlockInfo {
            block_key,
            status: BlockExecutionStatus::Speculative,
            transactions: Vec::new(), // Will be filled by caller
            state_version: self.state_version,
        };
        self.executed_blocks = Arc::new(self.executed_blocks.insert(block_key, block_info));

        Ok(())
    }

    /// Check if a block has already been executed
    pub fn has_executed_block(&self, block_key: &Hash) -> bool {
        self.executed_blocks.contains_key(block_key)
    }

    /// Finalize a block that was speculatively executed
    pub fn finalize_block(&mut self, block_key: Hash) -> Result<(), ExecutionError> {
        if let Some(mut block_info) = self.executed_blocks.get(&block_key).cloned() {
            if block_info.status == BlockExecutionStatus::Speculative {
                block_info.status = BlockExecutionStatus::Finalized;
                self.executed_blocks = Arc::new(self.executed_blocks.insert(block_key, block_info));
            }
            Ok(())
        } else {
            Err(ExecutionError::TransactionNotFound(block_key))
        }
    }

    /// Rollback conflicting speculative executions
    pub fn rollback_conflicting_versions(&mut self, conflicting_objects: &[ObjectRef]) -> Result<(), ExecutionError> {
        let mut objects_to_rollback = HashSet::new();
        let mut blocks_to_rollback = HashSet::new();

        // Find all objects that need to be rolled back
        for obj_ref in conflicting_objects {
            let key = ObjectKey::new(obj_ref.object_id, obj_ref.version);
            if let Some(block_key) = self.version_to_block.get(&key) {
                if let Some(block_info) = self.executed_blocks.get(block_key) {
                    if block_info.status == BlockExecutionStatus::Speculative {
                        blocks_to_rollback.insert(*block_key);
                        objects_to_rollback.insert(key);
                    }
                }
            }
        }

        // Rollback the identified blocks
        for block_key in blocks_to_rollback {
            self.rollback_block(block_key)?;
        }

        Ok(())
    }

    /// Rollback a specific block's execution
    fn rollback_block(&mut self, block_key: Hash) -> Result<(), ExecutionError> {
        // Remove from executed blocks
        let mut new_executed_blocks = (*self.executed_blocks).clone();
        new_executed_blocks.remove(&block_key);
        self.executed_blocks = Arc::new(new_executed_blocks);

        // In a full implementation, we would:
        // 1. Restore original object versions
        // 2. Release locks
        // 3. Remove created objects
        // 4. Update version mappings

        // For now, we'll mark this as a TODO for a more complete implementation
        warn!("Rollback not fully implemented for block {}", block_key);

        Ok(())
    }

    /// Execute a transaction from a Morpheus block
    pub fn execute_transaction_from_block(
        &mut self,
        signed_tx: &SignedTransaction,
        block_key: Hash,
        proposing_validator: Pubkey,
    ) -> Result<TransactionEffects, ExecutionError> {
        // Track which snapshot we're working with
        if let Some(snapshot) = self.active_snapshots.get_mut(&block_key) {
            // Record original state before modifications
            for obj_ref in &signed_tx.input_objects {
                let key = ObjectKey::new(obj_ref.object_id, obj_ref.version);
                if !snapshot.original_objects.contains_key(&key) {
                    snapshot.original_objects.insert(key, self.objects.get(&key).cloned());
                }
            }
        }

        // Create a certificate for the transaction (simulating the Morpheus 1-QC)
        let certificate = TransactionCertificate {
            transaction: signed_tx.clone(),
            auth_signatures: vec![], // Will be filled by Morpheus consensus
        };

        // Execute the certificate
        let effects = self.execute_certificate(&certificate, proposing_validator)?;

        // Track object version to block mapping
        let mut new_version_to_block = (*self.version_to_block).clone();
        for obj_ref in &effects.mutated_objects {
            let key = ObjectKey::new(obj_ref.object_id, obj_ref.version);
            new_version_to_block.insert(key, block_key);
        }
        for obj_id in &effects.created_objects {
            if let Some(version) = self.latest_versions.get(obj_id) {
                let key = ObjectKey::new(*obj_id, *version);
                new_version_to_block.insert(key, block_key);
            }
        }
        self.version_to_block = Arc::new(new_version_to_block);

        // Increment state version
        self.state_version += 1;

        Ok(effects)
    }

    /// Process a transaction (Step 2 in Sui Lutris) - validators sign valid transactions
    #[instrument(skip_all, fields(tx_digest = %signed_tx.digest()))]
    pub fn process_transaction(
        &mut self,
        signed_tx: &SignedTransaction,
    ) -> Result<(), ExecutionError> {
        let start = Instant::now();
        let _timer = Timer::new("hellas_process_transaction_seconds");

        // 1. Load all input objects
        let input_objects = self.load_input_objects(signed_tx)?;

        // 2. Verify transaction validity
        self.verify_transaction(signed_tx, &input_objects)?;

        // 3. Lock owned objects (atomic operation)
        self.lock_owned_objects(signed_tx)?;

        observability::record_operation("process_transaction", true, start.elapsed().as_secs_f64());
        Ok(())
    }

    /// Execute a transaction certificate (Step 5 in Sui Lutris)
    #[instrument(skip_all, fields(tx_digest = %certificate.digest()))]
    pub fn execute_certificate(
        &mut self,
        certificate: &TransactionCertificate,
        proposing_validator: Pubkey,
    ) -> Result<TransactionEffects, ExecutionError> {
        let start = Instant::now();
        let _timer = Timer::new("hellas_execute_certificate_seconds");

        // Check if already executed
        if let Some((_, effects)) = self.certificates.get(&certificate.digest()) {
            return Ok(effects.clone());
        }

        // Verify certificate validity
        self.verify_certificate(certificate)?;

        // Check if transaction needs consensus (has shared objects)
        if self.has_shared_objects(&certificate.transaction) {
            // Check if consensus has assigned versions for shared objects
            if !self.check_shared_locks(certificate)? {
                return Err(ExecutionError::ConsensusRequired);
            }
        }

        // Execute the transaction
        let effects = self.execute_transaction_internal(certificate, proposing_validator)?;

        // Store certificate and effects
        self.certificates = Arc::new(
            self.certificates
                .insert(certificate.digest(), (certificate.clone(), effects.clone()))
        );

        observability::record_operation("execute_certificate", true, start.elapsed().as_secs_f64());
        Ok(effects)
    }

    /// Process a certificate from consensus (assigns shared object versions)
    pub fn assign_shared_locks(
        &mut self,
        certificate: &TransactionCertificate,
    ) -> Result<(), ExecutionError> {
        let tx_digest = certificate.digest();
        
        // Get all shared objects
        let shared_objects = self.get_shared_objects(&certificate.transaction);
        if shared_objects.is_empty() {
            return Ok(());
        }

        // Calculate Lamport timestamp for version assignment
        let mut max_version = 0u64;

        // Check owned object versions
        for obj_ref in &certificate.transaction.input_objects {
            max_version = max_version.max(obj_ref.version);
        }

        // Check current shared object versions
        for obj_id in &shared_objects {
            if let Some(version) = self.next_shared_version.get(obj_id) {
                max_version = max_version.max(*version);
            }
        }

        let new_version = max_version + 1;

        // Assign versions to all shared objects
        let mut new_shared_locks = (*self.shared_locks).clone();
        let mut new_next_versions = (*self.next_shared_version).clone();

        for obj_id in shared_objects {
            let lock_key = (tx_digest, obj_id);
            new_shared_locks.insert_mut(
                lock_key,
                SharedLockInfo {
                    assigned_version: new_version,
                    executed: false,
                },
            );
            new_next_versions.insert_mut(obj_id, new_version + 1);
        }

        self.shared_locks = Arc::new(new_shared_locks);
        self.next_shared_version = Arc::new(new_next_versions);

        Ok(())
    }

    // === Helper Methods ===

    /// Load all input objects for a transaction
    fn load_input_objects(
        &self,
        signed_tx: &SignedTransaction,
    ) -> Result<Vec<(ObjectRef, ObjectMetadata)>, ExecutionError> {
        let mut objects = Vec::new();

        for obj_ref in &signed_tx.input_objects {
            let key = ObjectKey::new(obj_ref.object_id, obj_ref.version);
            
            // For owned objects, must load exact version
            if let Some(obj) = self.objects.get(&key) {
                objects.push((*obj_ref, obj.clone()));
            } else {
                // Check if we have a different version
                if let Some(latest_ver) = self.latest_versions.get(&obj_ref.object_id) {
                    return Err(ExecutionError::VersionMismatch(
                        obj_ref.object_id,
                        obj_ref.version,
                        *latest_ver,
                    ));
                } else {
                    return Err(ExecutionError::ObjectNotFound(obj_ref.object_id));
                }
            }
        }

        Ok(objects)
    }

    /// Verify transaction validity
    fn verify_transaction(
        &self,
        signed_tx: &SignedTransaction,
        input_objects: &[(ObjectRef, ObjectMetadata)],
    ) -> Result<(), ExecutionError> {
        // Verify signatures
        self.verify_signatures(signed_tx)?;

        // Verify authorization for all input objects
        for (_, obj_meta) in input_objects {
            if !obj_meta.is_authorized(&signed_tx.signer) {
                return Err(ExecutionError::PermissionDenied(format!(
                    "Signer {} not authorized for object {}",
                    signed_tx.signer, obj_meta.id
                )));
            }
        }

        // Additional transaction-specific validation would go here

        Ok(())
    }

    /// Lock owned objects atomically
    fn lock_owned_objects(
        &mut self,
        signed_tx: &SignedTransaction,
    ) -> Result<(), ExecutionError> {
        let tx_digest = signed_tx.digest();
        let mut new_locks = (*self.owned_locks).clone();

        // First pass: check all locks are available
        for obj_ref in &signed_tx.input_objects {
            let key = ObjectKey::new(obj_ref.object_id, obj_ref.version);
            
            if let Some(lock_status) = new_locks.get(&key) {
                match lock_status {
                    OwnedLockStatus::None => {
                        // Can lock
                    }
                    OwnedLockStatus::Locked(existing_tx) => {
                        if existing_tx != &tx_digest {
                            return Err(ExecutionError::ObjectLocked(
                                obj_ref.object_id,
                                obj_ref.version,
                            ));
                        }
                        // Already locked by this transaction, ok
                    }
                }
            } else {
                // Object version exists but no lock entry yet
                new_locks.insert_mut(key, OwnedLockStatus::None);
            }
        }

        // Second pass: acquire all locks
        for obj_ref in &signed_tx.input_objects {
            let key = ObjectKey::new(obj_ref.object_id, obj_ref.version);
            new_locks.insert_mut(key, OwnedLockStatus::Locked(tx_digest));
        }

        self.owned_locks = Arc::new(new_locks);
        Ok(())
    }

    /// Check if a transaction has shared objects
    fn has_shared_objects(&self, tx: &SignedTransaction) -> bool {
        // For now, we consider an object shared if it's a JobEscrow
        // In a full implementation, this would check object metadata
        for obj_ref in &tx.input_objects {
            if let Some(latest_ver) = self.latest_versions.get(&obj_ref.object_id) {
                let key = ObjectKey::new(obj_ref.object_id, *latest_ver);
                if let Some(obj_meta) = self.objects.get(&key) {
                    if matches!(obj_meta.object, Object::JobEscrow(_)) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Get shared object IDs from a transaction
    fn get_shared_objects(&self, tx: &SignedTransaction) -> Vec<ObjectId> {
        let mut shared = Vec::new();
        for obj_ref in &tx.input_objects {
            if let Some(latest_ver) = self.latest_versions.get(&obj_ref.object_id) {
                let key = ObjectKey::new(obj_ref.object_id, *latest_ver);
                if let Some(obj_meta) = self.objects.get(&key) {
                    if matches!(obj_meta.object, Object::JobEscrow(_)) {
                        shared.push(obj_ref.object_id);
                    }
                }
            }
        }
        shared
    }

    /// Check if shared locks have been assigned by consensus
    fn check_shared_locks(&self, certificate: &TransactionCertificate) -> Result<bool, ExecutionError> {
        let tx_digest = certificate.digest();
        let shared_objects = self.get_shared_objects(&certificate.transaction);

        for obj_id in shared_objects {
            let lock_key = (tx_digest, obj_id);
            if !self.shared_locks.contains_key(&lock_key) {
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Verify a certificate has valid signatures
    fn verify_certificate(&self, certificate: &TransactionCertificate) -> Result<(), ExecutionError> {
        // Check we have a quorum of signatures
        let required_weight = self.validators.len() - self.byzantine_tolerance;
        if certificate.auth_signatures.len() < required_weight {
            return Err(ExecutionError::CertificateError(format!(
                "Insufficient signatures: have {}, need {}",
                certificate.auth_signatures.len(),
                required_weight
            )));
        }

        // In production, would verify each signature
        Ok(())
    }

    /// Execute the core logic of a transaction
    fn execute_transaction_internal(
        &mut self,
        certificate: &TransactionCertificate,
        proposing_validator: Pubkey,
    ) -> Result<TransactionEffects, ExecutionError> {
        let signed_tx = &certificate.transaction;

        // Load input objects again (they might have changed if we're executing after consensus)
        let input_objects = self.load_input_objects_for_execution(certificate)?;

        // Execute based on transaction type
        let (consumed, created, mutated) = match &signed_tx.transaction {
            Transaction::CreateAccount { initial_balance } => {
                self.execute_create_account(&signed_tx.signer, *initial_balance)?
            }

            Transaction::SettleDirectly { provider, payment, .. } => {
                self.execute_settle_directly(signed_tx, provider, *payment, proposing_validator)?
            }

            Transaction::PostJob { .. } => {
                self.execute_post_job(signed_tx)?
            }

            Transaction::ClaimJob { escrow_id } => {
                self.execute_claim_job(signed_tx, escrow_id)?
            }

            Transaction::CommitResult { escrow_id, result_hash } => {
                self.execute_commit_result(signed_tx, escrow_id, *result_hash)?
            }

            Transaction::FinalizeJob { escrow_id } => {
                self.execute_finalize_job(escrow_id)?
            }

            Transaction::AbortJob { escrow_id } => {
                self.execute_abort_job(signed_tx, escrow_id)?
            }

            Transaction::ResetBudget { budget_certificates } => {
                self.execute_reset_budget(signed_tx, budget_certificates)?
            }
        };

        // Unlock owned objects that were consumed
        let mut new_locks = (*self.owned_locks).clone();
        for obj_ref in &consumed {
            let key = ObjectKey::new(obj_ref.object_id, obj_ref.version);
            new_locks.insert_mut(key, OwnedLockStatus::None);
        }
        self.owned_locks = Arc::new(new_locks);

        // Create effects
        let effects = TransactionEffects::new(
            certificate.digest(),
            consumed,
            created,
            mutated,
            true,
            None,
            100, // Fixed gas for now
        );

        Ok(effects)
    }

    /// Load input objects for execution (handles both owned and shared)
    fn load_input_objects_for_execution(
        &self,
        certificate: &TransactionCertificate,
    ) -> Result<Vec<(ObjectRef, ObjectMetadata)>, ExecutionError> {
        let tx_digest = certificate.digest();
        let mut objects = Vec::new();

        for obj_ref in &certificate.transaction.input_objects {
            // Check if this is a shared object
            let lock_key = (tx_digest, obj_ref.object_id);
            if let Some(lock_info) = self.shared_locks.get(&lock_key) {
                // Shared object - load at consensus-assigned version
                let key = ObjectKey::new(obj_ref.object_id, lock_info.assigned_version);
                if let Some(obj) = self.objects.get(&key) {
                    objects.push((
                        ObjectRef::new(obj_ref.object_id, lock_info.assigned_version),
                        obj.clone()
                    ));
                } else {
                    return Err(ExecutionError::ObjectNotFound(obj_ref.object_id));
                }
            } else {
                // Owned object - load at specified version
                let key = ObjectKey::new(obj_ref.object_id, obj_ref.version);
                if let Some(obj) = self.objects.get(&key) {
                    objects.push((*obj_ref, obj.clone()));
                } else {
                    return Err(ExecutionError::ObjectNotFound(obj_ref.object_id));
                }
            }
        }

        Ok(objects)
    }

    /// Verify signatures on a transaction
    fn verify_signatures(&self, signed_tx: &SignedTransaction) -> Result<(), ExecutionError> {
        // In production, would verify cryptographic signatures
        // For now, just check required signers are present
        
        if signed_tx.transaction.requires_multi_sig() {
            let required_signers = signed_tx.transaction.required_signers();
            let additional_sigs = signed_tx.additional_signatures.as_ref()
                .ok_or_else(|| ExecutionError::InvalidTransaction(
                    "Multi-sig transaction missing additional signatures".to_string()
                ))?;

            for required in &required_signers {
                if !additional_sigs.has_signed(required) {
                    return Err(ExecutionError::MissingSignature(*required));
                }
            }
        }

        Ok(())
    }

    /// Helper to generate unique object IDs
    fn generate_object_id(&mut self) -> ObjectId {
        self.object_counter += 1;
        let mut bytes = [0u8; 32];
        bytes[..8].copy_from_slice(&self.object_counter.to_le_bytes());
        bytes[8..16].copy_from_slice(&self.current_height.to_le_bytes());
        ObjectId::new(bytes)
    }

    /// Update object in state with new version
    fn update_object(
        &mut self,
        obj_meta: ObjectMetadata,
        new_version: Version,
    ) -> ObjectRef {
        let old_key = ObjectKey::new(obj_meta.id, obj_meta.version);
        let new_key = ObjectKey::new(obj_meta.id, new_version);
        
        let mut updated_meta = obj_meta;
        updated_meta.version = new_version;

        // Update objects map
        let mut new_objects = (*self.objects).clone();
        new_objects.remove_mut(&old_key);
        new_objects.insert_mut(new_key, updated_meta);
        self.objects = Arc::new(new_objects);

        // Update latest version
        self.latest_versions = Arc::new(
            self.latest_versions.insert(updated_meta.id, new_version)
        );

        ObjectRef::new(updated_meta.id, new_version)
    }

    /// Create a new object in state
    fn create_object(&mut self, obj_meta: ObjectMetadata) -> ObjectRef {
        let key = ObjectKey::new(obj_meta.id, obj_meta.version);
        
        // Insert into objects map
        self.objects = Arc::new(self.objects.insert(key, obj_meta.clone()));
        
        // Update latest version
        self.latest_versions = Arc::new(
            self.latest_versions.insert(obj_meta.id, obj_meta.version)
        );

        ObjectRef::new(obj_meta.id, obj_meta.version)
    }

    // === Transaction Execution Methods ===

    /// Execute CreateAccount transaction
    fn execute_create_account(
        &mut self,
        creator: &Pubkey,
        initial_balance: Amount,
    ) -> Result<(Vec<ObjectRef>, Vec<ObjectId>, Vec<ObjectRef>), ExecutionError> {
        // Generate new object ID
        let account_id = self.generate_object_id();

        // Create account object
        let mut account = HellasAccount::new(initial_balance);
        account.initialize_budgets(&self.validators, self.byzantine_tolerance);

        // Create metadata
        let metadata = ObjectMetadata::new(account_id, *creator, Object::Account(account));

        // Create object in state
        self.create_object(metadata);

        // Add to account lookup
        self.account_lookup = Arc::new(self.account_lookup.insert(*creator, account_id));

        Ok((vec![], vec![account_id], vec![]))
    }

    /// Execute SettleDirectly transaction
    fn execute_settle_directly(
        &mut self,
        signed_tx: &SignedTransaction,
        _provider: &Pubkey,
        payment: Amount,
        _proposing_validator: Pubkey,
    ) -> Result<(Vec<ObjectRef>, Vec<ObjectId>, Vec<ObjectRef>), ExecutionError> {
        // Get requestor and provider accounts
        let requestor_ref = &signed_tx.input_objects[0];
        let provider_ref = &signed_tx.input_objects[1];

        let requestor_key = ObjectKey::new(requestor_ref.object_id, requestor_ref.version);
        let provider_key = ObjectKey::new(provider_ref.object_id, provider_ref.version);

        let mut requestor_meta = self.objects.get(&requestor_key)
            .ok_or(ExecutionError::ObjectNotFound(requestor_ref.object_id))?
            .clone();
        let mut provider_meta = self.objects.get(&provider_key)
            .ok_or(ExecutionError::ObjectNotFound(provider_ref.object_id))?
            .clone();

        // Extract accounts
        let requestor_account = requestor_meta.object.as_mut_account()
            .ok_or(ExecutionError::InvalidTransaction("Not an account".to_string()))?;
        let provider_account = provider_meta.object.as_mut_account()
            .ok_or(ExecutionError::InvalidTransaction("Not an account".to_string()))?;

        // Use bounded counter execution
        self.validator_local_state
            .try_spend(requestor_ref.object_id, payment, signed_tx.digest())
            .map_err(|_| ExecutionError::InsufficientBalance {
                have: requestor_account.balance,
                need: payment,
            })?;

        // Update provider balance
        provider_account.balance = provider_account.balance
            .checked_add(payment)
            .ok_or_else(|| ExecutionError::InvalidTransaction("Balance overflow".to_string()))?;

        // Calculate new versions (Lamport timestamp)
        let new_version = requestor_ref.version.max(provider_ref.version) + 1;

        // Update objects
        let consumed = vec![*requestor_ref, *provider_ref];
        let new_requestor_ref = self.update_object(requestor_meta, new_version);
        let new_provider_ref = self.update_object(provider_meta, new_version);
        let mutated = vec![new_requestor_ref, new_provider_ref];

        Ok((consumed, vec![], mutated))
    }

    /// Execute PostJob transaction
    fn execute_post_job(
        &mut self,
        signed_tx: &SignedTransaction,
    ) -> Result<(Vec<ObjectRef>, Vec<ObjectId>, Vec<ObjectRef>), ExecutionError> {
        // Extract parameters from transaction
        let (provider, agreement_hash, job_spec_hash, payment, provider_bond_required, claim_deadline_delta) = 
            if let Transaction::PostJob { 
                provider, agreement_hash, job_spec_hash, payment, 
                provider_bond_required, claim_deadline_delta, .. 
            } = &signed_tx.transaction {
                (*provider, *agreement_hash, *job_spec_hash, *payment, *provider_bond_required, *claim_deadline_delta)
            } else {
                return Err(ExecutionError::InvalidTransaction("Not a PostJob transaction".to_string()));
            };

        // Get requestor account
        let requestor_ref = &signed_tx.input_objects[0];
        let requestor_key = ObjectKey::new(requestor_ref.object_id, requestor_ref.version);
        let mut requestor_meta = self.objects.get(&requestor_key)
            .ok_or(ExecutionError::ObjectNotFound(requestor_ref.object_id))?
            .clone();

        let requestor_account = requestor_meta.object.as_mut_account()
            .ok_or(ExecutionError::InvalidTransaction("Not an account".to_string()))?;

        // Check balance
        if requestor_account.balance < payment {
            return Err(ExecutionError::InsufficientBalance {
                have: requestor_account.balance,
                need: payment,
            });
        }

        // Deduct payment
        requestor_account.balance = requestor_account.balance
            .checked_sub(payment)
            .ok_or(ExecutionError::InsufficientBalance {
                have: requestor_account.balance,
                need: payment,
            })?;

        // Create job escrow
        let escrow_id = self.generate_object_id();
        let escrow = JobEscrow::new(
            agreement_hash,
            signed_tx.signer,
            provider,
            job_spec_hash,
            payment,
            provider_bond_required,
            self.current_height + claim_deadline_delta,
        );

        let mut escrow_meta = ObjectMetadata::new(escrow_id, signed_tx.signer, Object::JobEscrow(escrow));
        
        // Add provider as authorized if pre-selected
        if let Some(provider_pubkey) = provider {
            escrow_meta.add_owner(provider_pubkey);
        }

        // Update state
        let new_version = requestor_ref.version + 1;
        let consumed = vec![*requestor_ref];
        let new_requestor_ref = self.update_object(requestor_meta, new_version);
        self.create_object(escrow_meta);

        Ok((consumed, vec![escrow_id], vec![new_requestor_ref]))
    }

    // Implement remaining transaction types following the same pattern...
    
    fn execute_claim_job(
        &mut self,
        signed_tx: &SignedTransaction,
        escrow_id: &ObjectId,
    ) -> Result<(Vec<ObjectRef>, Vec<ObjectId>, Vec<ObjectRef>), ExecutionError> {
        trace!("Executing ClaimJob for escrow {}", escrow_id);

        // Get escrow and provider account
        let escrow_ref = &signed_tx.input_objects[0];
        let provider_ref = &signed_tx.input_objects[1];

        let escrow_key = ObjectKey::new(escrow_ref.object_id, escrow_ref.version);
        let provider_key = ObjectKey::new(provider_ref.object_id, provider_ref.version);

        let mut escrow_meta = self.objects
            .get(&escrow_key)
            .ok_or(ExecutionError::ObjectNotFound(escrow_ref.object_id))?
            .clone();
        let mut provider_meta = self.objects
            .get(&provider_key)
            .ok_or(ExecutionError::ObjectNotFound(provider_ref.object_id))?
            .clone();

        // Verify escrow state first
        {
            let escrow = escrow_meta
                .object
                .as_job_escrow()
                .ok_or(ExecutionError::InvalidTransaction("Not a job escrow".to_string()))?;

            if escrow.status != JobStatus::Posted {
                return Err(ExecutionError::InvalidTransaction(format!(
                    "Invalid job status: expected Posted, got {:?}",
                    escrow.status
                )));
            }
        }

        // Handle provider authorization and update escrow
        let needs_owner_update = {
            let escrow = escrow_meta
                .object
                .as_mut_job_escrow()
                .ok_or(ExecutionError::InvalidTransaction("Not a job escrow".to_string()))?;

            if let Some(pre_selected_provider) = escrow.provider {
                // If provider is pre-selected, only they can claim
                if signed_tx.signer != pre_selected_provider {
                    return Err(ExecutionError::PermissionDenied(
                        "Only the pre-selected provider can claim this job".to_string(),
                    ));
                }
                false
            } else {
                // Open bounty - any provider can claim
                // Set the provider now that someone is claiming
                escrow.provider = Some(signed_tx.signer);
                true
            }
        };

        // Add provider as an authorized party if this was an open bounty
        if needs_owner_update {
            escrow_meta.add_owner(signed_tx.signer);
        }

        // Now get mutable references again for the rest of the logic
        let escrow = escrow_meta
            .object
            .as_mut_job_escrow()
            .ok_or(ExecutionError::InvalidTransaction("Not a job escrow".to_string()))?;
        let provider_account = provider_meta
            .object
            .as_mut_account()
            .ok_or(ExecutionError::InvalidTransaction("Not an account".to_string()))?;

        // Check claim deadline
        if self.current_height > escrow.claim_deadline {
            return Err(ExecutionError::InvalidTransaction(format!(
                "Deadline missed: current height {}, deadline {}",
                self.current_height, escrow.claim_deadline
            )));
        }

        // Check provider has sufficient bond
        if provider_account.balance < escrow.provider_bond_required {
            return Err(ExecutionError::InsufficientBalance {
                have: provider_account.balance,
                need: escrow.provider_bond_required,
            });
        }

        // Lock provider bond
        provider_account.balance = provider_account
            .balance
            .checked_sub(escrow.provider_bond_required)
            .ok_or(ExecutionError::InsufficientBalance {
                have: provider_account.balance,
                need: escrow.provider_bond_required,
            })?;
        escrow.provider_bond_locked = escrow.provider_bond_required;

        // Update escrow status
        escrow.status = JobStatus::Claimed;
        escrow.claimed_at = Some(self.current_height);
        escrow.commit_deadline = self.current_height + 200; // TODO: Make configurable

        // Save provider info before moving (we know it's Some at this point)
        let provider_pubkey = escrow.provider.unwrap();

        // Calculate new versions (Lamport timestamp)
        let new_version = escrow_ref.version.max(provider_ref.version) + 1;

        // Update objects
        let consumed = vec![*escrow_ref, *provider_ref];
        let new_escrow_ref = self.update_object(escrow_meta, new_version);
        let new_provider_ref = self.update_object(provider_meta, new_version);
        let mutated = vec![new_escrow_ref, new_provider_ref];

        info!("Job claimed successfully by provider {}", provider_pubkey);

        Ok((consumed, vec![], mutated))
    }

    fn execute_commit_result(
        &mut self,
        signed_tx: &SignedTransaction,
        escrow_id: &ObjectId,
        result_hash: Hash,
    ) -> Result<(Vec<ObjectRef>, Vec<ObjectId>, Vec<ObjectRef>), ExecutionError> {
        trace!("Executing CommitResult for escrow {}", escrow_id);

        // Get escrow object
        let escrow_ref = &signed_tx.input_objects[0];
        let escrow_key = ObjectKey::new(escrow_ref.object_id, escrow_ref.version);
        let mut escrow_meta = self
            .objects
            .get(&escrow_key)
            .ok_or(ExecutionError::ObjectNotFound(escrow_ref.object_id))?
            .clone();

        let escrow = escrow_meta
            .object
            .as_mut_job_escrow()
            .ok_or(ExecutionError::InvalidTransaction("Not a job escrow".to_string()))?;

        // Verify escrow state
        if escrow.status != JobStatus::Claimed {
            return Err(ExecutionError::InvalidTransaction(format!(
                "Invalid job status: expected Claimed, got {:?}",
                escrow.status
            )));
        }

        // Verify signer is the provider
        let provider = escrow.provider.ok_or_else(|| {
            ExecutionError::InvalidTransaction("No provider set for this job".to_string())
        })?;
        if signed_tx.signer != provider {
            return Err(ExecutionError::PermissionDenied(
                "Only the provider can commit results".to_string(),
            ));
        }

        // Check commit deadline
        if self.current_height > escrow.commit_deadline {
            return Err(ExecutionError::InvalidTransaction(format!(
                "Deadline missed: current height {}, deadline {}",
                self.current_height, escrow.commit_deadline
            )));
        }

        // Update escrow with result
        escrow.result_hash = Some(result_hash);
        escrow.status = JobStatus::Committed;
        escrow.committed_at = Some(self.current_height);
        escrow.finalize_after = self.current_height + escrow.finalization_delay;

        // Calculate new version
        let new_version = escrow_ref.version + 1;

        // Update object
        let consumed = vec![*escrow_ref];
        let new_escrow_ref = self.update_object(escrow_meta, new_version);
        let mutated = vec![new_escrow_ref];

        info!("Result committed for job {}", escrow_id);

        Ok((consumed, vec![], mutated))
    }

    fn execute_finalize_job(
        &mut self,
        escrow_id: &ObjectId,
    ) -> Result<(Vec<ObjectRef>, Vec<ObjectId>, Vec<ObjectRef>), ExecutionError> {
        trace!("Executing FinalizeJob for escrow {}", escrow_id);

        // Get escrow object - need to find the latest version
        let escrow_version = self.latest_versions.get(escrow_id)
            .ok_or(ExecutionError::ObjectNotFound(*escrow_id))?;
        let escrow_key = ObjectKey::new(*escrow_id, *escrow_version);
        
        let mut escrow_meta = self.objects
            .get(&escrow_key)
            .ok_or(ExecutionError::ObjectNotFound(*escrow_id))?
            .clone();

        let escrow = escrow_meta
            .object
            .as_mut_job_escrow()
            .ok_or(ExecutionError::InvalidTransaction("Not a job escrow".to_string()))?;

        // Verify escrow state
        if escrow.status != JobStatus::Committed {
            return Err(ExecutionError::InvalidTransaction(format!(
                "Invalid job status: expected Committed, got {:?}",
                escrow.status
            )));
        }

        // Check finalization deadline has passed
        if self.current_height < escrow.finalize_after {
            return Err(ExecutionError::InvalidTransaction(format!(
                "Cannot finalize yet: current height {}, finalize after {}",
                self.current_height, escrow.finalize_after
            )));
        }

        // Get provider account to release payment and bond
        let provider = escrow.provider.ok_or_else(|| {
            ExecutionError::InvalidTransaction("No provider set for this job".to_string())
        })?;
        let provider_account_id = self
            .account_lookup
            .get(&provider)
            .ok_or_else(|| ExecutionError::ObjectNotFound(ObjectId::new([0; 32])))?;
        
        let provider_version = self.latest_versions.get(provider_account_id)
            .ok_or(ExecutionError::ObjectNotFound(*provider_account_id))?;
        let provider_key = ObjectKey::new(*provider_account_id, *provider_version);
        
        let mut provider_meta = self.objects
            .get(&provider_key)
            .ok_or(ExecutionError::ObjectNotFound(*provider_account_id))?
            .clone();

        let provider_account = provider_meta
            .object
            .as_mut_account()
            .ok_or(ExecutionError::InvalidTransaction("Not an account".to_string()))?;

        // Release payment and bond to provider
        let total_payment = escrow
            .payment
            .checked_add(escrow.provider_bond_locked)
            .ok_or_else(|| ExecutionError::InvalidTransaction("Payment overflow".to_string()))?;
        provider_account.balance = provider_account
            .balance
            .checked_add(total_payment)
            .ok_or_else(|| ExecutionError::InvalidTransaction("Balance overflow".to_string()))?;

        // Update escrow status
        escrow.status = JobStatus::Finalized;
        escrow.finalized_at = Some(self.current_height);

        // Calculate new versions
        let new_version = (*escrow_version).max(*provider_version) + 1;

        // Update objects
        let consumed = vec![
            ObjectRef::new(*escrow_id, *escrow_version),
            ObjectRef::new(*provider_account_id, *provider_version),
        ];
        let new_escrow_ref = self.update_object(escrow_meta, new_version);
        let new_provider_ref = self.update_object(provider_meta, new_version);
        let mutated = vec![new_escrow_ref, new_provider_ref];

        // Update metrics
        gauge!("hellas_escrows_active", -1.0);
        info!("Job finalized, payment released to provider");

        Ok((consumed, vec![], mutated))
    }

    fn execute_abort_job(
        &mut self,
        signed_tx: &SignedTransaction,
        escrow_id: &ObjectId,
    ) -> Result<(Vec<ObjectRef>, Vec<ObjectId>, Vec<ObjectRef>), ExecutionError> {
        trace!("Executing AbortJob for escrow {}", escrow_id);

        // Get escrow object
        let escrow_ref = &signed_tx.input_objects[0];
        let escrow_key = ObjectKey::new(escrow_ref.object_id, escrow_ref.version);
        let mut escrow_meta = self.objects
            .get(&escrow_key)
            .ok_or(ExecutionError::ObjectNotFound(escrow_ref.object_id))?
            .clone();

        let escrow = escrow_meta
            .object
            .as_mut_job_escrow()
            .ok_or(ExecutionError::InvalidTransaction("Not a job escrow".to_string()))?;

        // Determine who gets refunded based on state and who's aborting
        let (refund_to_requestor, refund_to_provider, slash_provider) = match escrow.status {
            JobStatus::Posted => {
                // Before claim: only requestor can abort, gets full refund
                if signed_tx.signer != escrow.requestor {
                    return Err(ExecutionError::PermissionDenied(
                        "Only requestor can abort unclaimed jobs".to_string(),
                    ));
                }
                (escrow.payment, Amount::ZERO, false)
            }
            JobStatus::Claimed => {
                // After claim: check deadlines
                if self.current_height > escrow.commit_deadline {
                    // Provider missed deadline, requestor gets refund, provider slashed
                    (escrow.payment, Amount::ZERO, true)
                } else if signed_tx.signer == escrow.requestor {
                    // Requestor aborts early, provider gets payment + bond back
                    (
                        Amount::ZERO,
                        escrow
                            .payment
                            .checked_add(escrow.provider_bond_locked)
                            .ok_or_else(|| {
                                ExecutionError::InvalidTransaction("Payment overflow".to_string())
                            })?,
                        false,
                    )
                } else {
                    return Err(ExecutionError::PermissionDenied(
                        "Cannot abort during active work period".to_string(),
                    ));
                }
            }
            JobStatus::Committed => {
                return Err(ExecutionError::InvalidTransaction(
                    "Cannot abort committed job".to_string(),
                ));
            }
            _ => {
                return Err(ExecutionError::InvalidTransaction(
                    "Cannot abort job in this state".to_string(),
                ));
            }
        };

        let mut consumed = vec![*escrow_ref];
        let mut mutated = vec![];

        // Process refunds
        if refund_to_requestor > Amount::ZERO {
            let requestor_account_id = self
                .account_lookup
                .get(&escrow.requestor)
                .ok_or_else(|| ExecutionError::ObjectNotFound(ObjectId::new([0; 32])))?;
            
            let requestor_version = self.latest_versions.get(requestor_account_id)
                .ok_or(ExecutionError::ObjectNotFound(*requestor_account_id))?;
            let requestor_key = ObjectKey::new(*requestor_account_id, *requestor_version);
            
            let mut requestor_meta = self.objects
                .get(&requestor_key)
                .ok_or(ExecutionError::ObjectNotFound(*requestor_account_id))?
                .clone();
            let requestor_account = requestor_meta
                .object
                .as_mut_account()
                .ok_or(ExecutionError::InvalidTransaction("Not an account".to_string()))?;
            requestor_account.balance = requestor_account
                .balance
                .checked_add(refund_to_requestor)
                .ok_or_else(|| {
                    ExecutionError::InvalidTransaction("Balance overflow".to_string())
                })?;
            
            consumed.push(ObjectRef::new(*requestor_account_id, *requestor_version));
            let new_requestor_ref = self.update_object(requestor_meta, *requestor_version + 1);
            mutated.push(new_requestor_ref);
        }

        if refund_to_provider > Amount::ZERO {
            let provider = escrow.provider.ok_or_else(|| {
                ExecutionError::InvalidTransaction("No provider set for this job".to_string())
            })?;
            let provider_account_id = self
                .account_lookup
                .get(&provider)
                .ok_or_else(|| ExecutionError::ObjectNotFound(ObjectId::new([0; 32])))?;
            
            let provider_version = self.latest_versions.get(provider_account_id)
                .ok_or(ExecutionError::ObjectNotFound(*provider_account_id))?;
            let provider_key = ObjectKey::new(*provider_account_id, *provider_version);
            
            let mut provider_meta = self.objects
                .get(&provider_key)
                .ok_or(ExecutionError::ObjectNotFound(*provider_account_id))?
                .clone();
            let provider_account = provider_meta
                .object
                .as_mut_account()
                .ok_or(ExecutionError::InvalidTransaction("Not an account".to_string()))?;
            provider_account.balance = provider_account
                .balance
                .checked_add(refund_to_provider)
                .ok_or_else(|| {
                    ExecutionError::InvalidTransaction("Balance overflow".to_string())
                })?;
            
            consumed.push(ObjectRef::new(*provider_account_id, *provider_version));
            let new_provider_ref = self.update_object(provider_meta, *provider_version + 1);
            mutated.push(new_provider_ref);
        }

        // Update escrow status
        escrow.status = JobStatus::Aborted;
        escrow.aborted_at = Some(self.current_height);
        
        // Calculate new version for escrow
        let mut max_version = escrow_ref.version;
        for obj_ref in &consumed {
            max_version = max_version.max(obj_ref.version);
        }
        let new_escrow_ref = self.update_object(escrow_meta, max_version + 1);
        mutated.push(new_escrow_ref);

        // Update metrics
        gauge!("hellas_escrows_active", -1.0);

        if slash_provider {
            warn!("Provider slashed for missing deadline on job {}", escrow_id);
            counter!("hellas_provider_slashes", 1);
        }

        info!(
            "Job aborted: refund_requestor={}, refund_provider={}",
            refund_to_requestor, refund_to_provider
        );

        Ok((consumed, vec![], mutated))
    }

    fn execute_reset_budget(
        &mut self,
        signed_tx: &SignedTransaction,
        certificates: &[crate::transactions::BudgetCertificate],
    ) -> Result<(Vec<ObjectRef>, Vec<ObjectId>, Vec<ObjectRef>), ExecutionError> {
        // Get the account to reset
        let account_ref = &signed_tx.input_objects[0];
        let account_key = ObjectKey::new(account_ref.object_id, account_ref.version);
        let mut account_meta = self.objects
            .get(&account_key)
            .ok_or(ExecutionError::ObjectNotFound(account_ref.object_id))?
            .clone();

        let account = account_meta
            .object
            .as_mut_account()
            .ok_or(ExecutionError::InvalidTransaction("Not an account".to_string()))?;

        // Verify we have certificates from all validators who spent budget
        let validators_with_spending: HashSet<_> = account
            .validator_budgets
            .iter()
            .filter(|(_, budget)| **budget < account.max_budget_per_validator)
            .map(|(v, _)| *v)
            .collect();

        let certificate_validators: HashSet<_> =
            certificates.iter().map(|cert| cert.validator).collect();

        // Check if any validator who spent is missing a certificate
        let missing_validators: Vec<_> = validators_with_spending
            .difference(&certificate_validators)
            .collect();

        if !missing_validators.is_empty() {
            return Err(ExecutionError::InvalidTransaction(format!(
                "Missing certificates from validators who spent budget: {:?}",
                missing_validators
            )));
        }

        // Verify certificates and calculate total spent
        let total_spent = self
            .channel_manager
            .verify_certificates(certificates)
            .map_err(|e| {
                ExecutionError::InvalidTransaction(format!("Invalid certificates: {}", e))
            })?;

        // Update account balance
        let new_balance = account.balance.checked_sub(total_spent).ok_or(
            ExecutionError::InsufficientBalance {
                have: account.balance,
                need: total_spent,
            },
        )?;

        account.balance = new_balance;
        account.last_budget_reset_version = account_meta.version;

        // Reinitialize budgets for all validators
        account.initialize_budgets(&self.validators, self.byzantine_tolerance);

        // Reset all validator channels for this account
        self.channel_manager.init_account_channels(
            account_ref.object_id,
            account.balance,
            account_meta.version + 1,
        );

        // Also reset our local validator's channel
        self.validator_local_state.reset_channel(
            account_ref.object_id,
            self.channel_manager.calculate_max_budget(account.balance),
            account_meta.version + 1,
        );

        // Update object
        let new_version = account_ref.version + 1;
        let consumed = vec![*account_ref];
        let new_account_ref = self.update_object(account_meta, new_version);
        let mutated = vec![new_account_ref];

        Ok((consumed, vec![], mutated))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_account() {
        let validators = vec![
            Pubkey::test(1),
            Pubkey::test(2),
            Pubkey::test(3),
            Pubkey::test(4),
        ];
        let mut engine = StateTransitionEngine::new(validators, 1);

        let creator = Pubkey::test(10);
        let tx = Transaction::CreateAccount {
            initial_balance: Amount::from_units(1000),
        };
        let signed_tx = SignedTransaction::new_single_signer(creator, tx, vec![], 0);

        // Process the transaction (validators sign it)
        engine.process_transaction(&signed_tx).unwrap();

        // Create a certificate and execute it
        let certificate = TransactionCertificate {
            transaction: signed_tx,
            auth_signatures: vec![], // Would be filled with validator signatures
        };
        let effects = engine
            .execute_certificate(&certificate, Pubkey::test(1))
            .unwrap();
        assert!(effects.success);
        assert_eq!(effects.created_objects.len(), 1);

        // Verify account was created
        let account_id = effects.created_objects[0];
        let account_version = engine.latest_versions.get(&account_id).unwrap();
        let account_key = ObjectKey::new(account_id, *account_version);
        let account_meta = engine.objects.get(&account_key).unwrap();
        let account = account_meta.object.as_account().unwrap();
        assert_eq!(account.balance, Amount::from_units(1000));
    }

    #[test]
    fn test_open_marketplace_job() {
        let validators = vec![
            Pubkey::test(1),
            Pubkey::test(2),
            Pubkey::test(3),
            Pubkey::test(4),
        ];
        let mut engine = StateTransitionEngine::new(validators, 1);

        // Create requestor account
        let requestor = Pubkey::test(10);
        let tx = Transaction::CreateAccount {
            initial_balance: Amount::from_units(1000),
        };
        let signed_tx = SignedTransaction::new_single_signer(requestor, tx, vec![], 0);
        engine.process_transaction(&signed_tx).unwrap();
        let certificate = TransactionCertificate {
            transaction: signed_tx,
            auth_signatures: vec![],
        };
        let effects = engine
            .execute_certificate(&certificate, Pubkey::test(1))
            .unwrap();
        let requestor_account_id = effects.created_objects[0];

        // Create provider account
        let provider = Pubkey::test(11);
        let tx = Transaction::CreateAccount {
            initial_balance: Amount::from_units(500),
        };
        let signed_tx = SignedTransaction::new_single_signer(provider, tx, vec![], 0);
        engine.process_transaction(&signed_tx).unwrap();
        let certificate = TransactionCertificate {
            transaction: signed_tx,
            auth_signatures: vec![],
        };
        let effects = engine
            .execute_certificate(&certificate, Pubkey::test(1))
            .unwrap();
        let provider_account_id = effects.created_objects[0];

        // Post job without pre-selected provider (open bounty)
        let tx = Transaction::PostJob {
            provider: None, // No pre-selected provider
            agreement_hash: Hash::compute(b"agreement"),
            job_spec_hash: Hash::compute(b"job spec"),
            payment: Amount::from_units(100),
            provider_bond_required: Amount::from_units(50),
            claim_deadline_delta: 100,
            commit_deadline_delta: 200,
            finalization_delay: 50,
        };
        let input_refs = vec![ObjectRef::new(requestor_account_id, 0)];
        let signed_tx = SignedTransaction::new_single_signer(requestor, tx, input_refs, 0);
        engine.process_transaction(&signed_tx).unwrap();
        let certificate = TransactionCertificate {
            transaction: signed_tx,
            auth_signatures: vec![],
        };
        let effects = engine
            .execute_certificate(&certificate, Pubkey::test(1))
            .unwrap();
        assert!(effects.success);

        // Find the escrow ID
        let escrow_id = effects
            .created_objects
            .iter()
            .find(|id| {
                let version = engine.latest_versions.get(id).unwrap();
                let key = ObjectKey::new(**id, *version);
                engine
                    .objects
                    .get(&key)
                    .unwrap()
                    .object
                    .as_job_escrow()
                    .is_some()
            })
            .unwrap();

        // Verify escrow has no provider set
        let escrow_version = engine.latest_versions.get(escrow_id).unwrap();
        let escrow_key = ObjectKey::new(*escrow_id, *escrow_version);
        let escrow_meta = engine.objects.get(&escrow_key).unwrap();
        let escrow = escrow_meta.object.as_job_escrow().unwrap();
        assert_eq!(escrow.provider, None);
        assert_eq!(escrow.status, JobStatus::Posted);

        // Any provider can claim the job
        let tx = Transaction::ClaimJob {
            escrow_id: *escrow_id,
        };
        let input_refs = vec![
            ObjectRef::new(*escrow_id, 0), // Escrow is still at version 0
            ObjectRef::new(provider_account_id, 0),
        ];
        let signed_tx = SignedTransaction::new_single_signer(provider, tx, input_refs, 0);
        engine.process_transaction(&signed_tx).unwrap();
        let certificate = TransactionCertificate {
            transaction: signed_tx,
            auth_signatures: vec![],
        };
        let effects = engine
            .execute_certificate(&certificate, Pubkey::test(1))
            .unwrap();
        assert!(effects.success);

        // Verify provider is now set
        let escrow_version = engine.latest_versions.get(escrow_id).unwrap();
        let escrow_key = ObjectKey::new(*escrow_id, *escrow_version);
        let escrow_meta = engine.objects.get(&escrow_key).unwrap();
        let escrow = escrow_meta.object.as_job_escrow().unwrap();
        assert_eq!(escrow.provider, Some(provider));
        assert_eq!(escrow.status, JobStatus::Claimed);
        assert_eq!(escrow.provider_bond_locked, Amount::from_units(50));

        // Verify provider is now authorized on the escrow
        assert!(escrow_meta.is_authorized(&provider));
    }

    #[test]
    fn test_object_locking() {
        let validators = vec![Pubkey::test(1), Pubkey::test(2), Pubkey::test(3)];
        let mut engine = StateTransitionEngine::new(validators, 1);
        
        // Create a test account
        let creator = Pubkey::test(10);
        let tx = Transaction::CreateAccount {
            initial_balance: Amount::from_units(1000),
        };
        let signed_tx = SignedTransaction::new_single_signer(creator, tx, vec![], 0);
        
        // Process creation should work
        assert!(engine.process_transaction(&signed_tx).is_ok());
    }

    #[test]
    fn test_pre_selected_provider_job() {
        let validators = vec![
            Pubkey::test(1),
            Pubkey::test(2),
            Pubkey::test(3),
            Pubkey::test(4),
        ];
        let mut engine = StateTransitionEngine::new(validators, 1);

        // Create requestor and provider accounts
        let requestor = Pubkey::test(10);
        let provider = Pubkey::test(11);
        let other_provider = Pubkey::test(12);

        // Create accounts
        for (pubkey, balance) in &[(requestor, 1000), (provider, 500), (other_provider, 500)] {
            let tx = Transaction::CreateAccount {
                initial_balance: Amount::from_units(*balance),
            };
            let signed_tx = SignedTransaction::new_single_signer(*pubkey, tx, vec![], 0);
            engine.process_transaction(&signed_tx).unwrap();
            let certificate = TransactionCertificate {
                transaction: signed_tx,
                auth_signatures: vec![],
            };
            engine
                .execute_certificate(&certificate, Pubkey::test(1))
                .unwrap();
        }

        let requestor_account_id = *engine.account_lookup.get(&requestor).unwrap();
        let provider_account_id = *engine.account_lookup.get(&provider).unwrap();
        let other_provider_account_id = *engine.account_lookup.get(&other_provider).unwrap();

        // Post job with pre-selected provider
        let tx = Transaction::PostJob {
            provider: Some(provider), // Pre-selected provider
            agreement_hash: Hash::compute(b"agreement"),
            job_spec_hash: Hash::compute(b"job spec"),
            payment: Amount::from_units(100),
            provider_bond_required: Amount::from_units(50),
            claim_deadline_delta: 100,
            commit_deadline_delta: 200,
            finalization_delay: 50,
        };
        let input_refs = vec![ObjectRef::new(requestor_account_id, 0)];
        let signed_tx = SignedTransaction::new_single_signer(requestor, tx, input_refs, 0);
        engine.process_transaction(&signed_tx).unwrap();
        let certificate = TransactionCertificate {
            transaction: signed_tx,
            auth_signatures: vec![],
        };
        let effects = engine
            .execute_certificate(&certificate, Pubkey::test(1))
            .unwrap();
        assert!(effects.success);

        // Find the escrow ID
        let escrow_id = effects
            .created_objects
            .iter()
            .find(|id| {
                let version = engine.latest_versions.get(id).unwrap();
                let key = ObjectKey::new(**id, *version);
                engine
                    .objects
                    .get(&key)
                    .unwrap()
                    .object
                    .as_job_escrow()
                    .is_some()
            })
            .unwrap();

        // Try to claim with wrong provider - should fail
        let tx = Transaction::ClaimJob {
            escrow_id: *escrow_id,
        };
        let input_refs = vec![
            ObjectRef::new(*escrow_id, 0), // Escrow is still at version 0
            ObjectRef::new(other_provider_account_id, 0),
        ];
        let signed_tx = SignedTransaction::new_single_signer(other_provider, tx, input_refs, 0);
        engine.process_transaction(&signed_tx).unwrap();
        let certificate = TransactionCertificate {
            transaction: signed_tx,
            auth_signatures: vec![],
        };
        let result = engine.execute_certificate(&certificate, Pubkey::test(1));
        assert!(matches!(result, Err(ExecutionError::PermissionDenied(_))));

        // Claim with correct provider - should succeed
        let tx = Transaction::ClaimJob {
            escrow_id: *escrow_id,
        };
        let input_refs = vec![
            ObjectRef::new(*escrow_id, 0), // Escrow is still at version 0
            ObjectRef::new(provider_account_id, 0),
        ];
        let signed_tx = SignedTransaction::new_single_signer(provider, tx, input_refs, 0);
        engine.process_transaction(&signed_tx).unwrap();
        let certificate = TransactionCertificate {
            transaction: signed_tx,
            auth_signatures: vec![],
        };
        let effects = engine
            .execute_certificate(&certificate, Pubkey::test(1))
            .unwrap();
        assert!(effects.success);
    }
}
