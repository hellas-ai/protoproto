//! # State Transition Engine
//!
//! This module implements the core execution engine for the Hellas protocol.
//! It processes transactions from the consensus layer and updates the world state
//! according to the protocol rules.
//!
//! ## Key Responsibilities
//!
//! 1. **Authorization**: Verify that transactions are properly signed by object owners
//! 2. **Validation**: Check that transactions meet all preconditions
//! 3. **Execution**: Apply state changes atomically
//! 4. **Effects**: Generate receipts describing what changed
//!
//! ## Design Principles
//!
//! - **Deterministic**: Same inputs always produce same outputs
//! - **Atomic**: Transactions fully succeed or fully fail
//! - **Isolated**: Transactions see a consistent snapshot of state
//! - **Efficient**: Support for parallel execution when possible

use crate::bounded_counter::{BoundedCounterManager, ValidatorLocalState};
use crate::objects::{HellasAccount, JobEscrow, JobStatus, Object, ObjectMetadata};
use crate::observability::{self, Timer, TransactionType};
use crate::transactions::{ObjectRef, SignedTransaction, Transaction, TransactionEffects};
use crate::types::{Amount, BlockHeight, Hash, ObjectId, Pubkey, Version};
use metrics::{counter, gauge};
use std::collections::{HashMap, HashSet};
use std::time::Instant;
use thiserror::Error;
use tracing::{debug, info, instrument, trace, warn};

/// Parameters for posting a job
#[derive(Debug, Clone)]
struct PostJobParams {
    provider: Option<Pubkey>,
    agreement_hash: Hash,
    job_spec_hash: Hash,
    payment: Amount,
    provider_bond_required: Amount,
    claim_deadline_delta: BlockHeight,
    _commit_deadline_delta: BlockHeight,
    _finalization_delay: BlockHeight,
}

/// Errors that can occur during transaction execution
#[derive(Debug, Error, PartialEq)]
pub enum ExecutionError {
    #[error("Object not found: {0}")]
    ObjectNotFound(ObjectId),

    #[error("Version mismatch for object {0}: expected {1}, found {2}")]
    VersionMismatch(ObjectId, Version, Version),

    #[error("Insufficient balance: have {have}, need {need}")]
    InsufficientBalance { have: Amount, need: Amount },

    #[error("Insufficient budget for validator")]
    InsufficientBudget,

    #[error("Invalid signature")]
    InvalidSignature,

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Invalid job status: expected {expected:?}, got {actual:?}")]
    InvalidJobStatus {
        expected: JobStatus,
        actual: JobStatus,
    },

    #[error("Deadline missed: current {current}, deadline {deadline}")]
    DeadlineMissed {
        current: BlockHeight,
        deadline: BlockHeight,
    },

    #[error("Deadline not reached: current {current}, deadline {deadline}")]
    DeadlineNotReached {
        current: BlockHeight,
        deadline: BlockHeight,
    },

    #[error("Invalid transaction: {0}")]
    InvalidTransaction(String),

    #[error("Object type mismatch")]
    ObjectTypeMismatch,

    #[error("Missing required signature from {0}")]
    MissingSignature(Pubkey),
}

#[derive(Debug, Clone)]
/// The main state transition engine
pub struct StateTransitionEngine {
    /// The world state: all objects in the system
    pub state: HashMap<ObjectId, ObjectMetadata>,

    /// Current block height
    pub current_height: BlockHeight,

    /// Set of active validators
    pub validators: Vec<Pubkey>,

    /// Byzantine fault tolerance parameter
    pub byzantine_tolerance: usize,

    /// Bounded counter execution manager
    pub channel_manager: BoundedCounterManager,

    /// This validator's local state (in production, each validator only has their own)
    pub validator_local_state: ValidatorLocalState,

    /// Counter for generating unique object IDs
    pub object_counter: u64,

    /// Mapping from pubkey to account ID (temporary solution)
    pub account_lookup: HashMap<Pubkey, ObjectId>,
}

impl StateTransitionEngine {
    /// Create a new state transition engine
    pub fn new(validators: Vec<Pubkey>, byzantine_tolerance: usize) -> Self {
        Self {
            state: HashMap::new(),
            current_height: 0,
            validators: validators.clone(),
            byzantine_tolerance,
            channel_manager: BoundedCounterManager::new(validators.clone(), byzantine_tolerance),
            validator_local_state: ValidatorLocalState::new(validators[0]), // Using first validator for demo
            object_counter: 0,
            account_lookup: HashMap::new(),
        }
    }

    /// Execute a signed transaction and return its effects
    #[instrument(skip_all, fields(
        tx_digest = %signed_tx.digest(),
        tx_type = ?signed_tx.transaction,
        proposer = %proposing_validator
    ))]
    pub fn execute_transaction(
        &mut self,
        signed_tx: &SignedTransaction,
        proposing_validator: Pubkey,
    ) -> Result<TransactionEffects, ExecutionError> {
        let start = Instant::now();
        let _timer = Timer::new("hellas_transaction_duration_seconds");
        // 1. Verify signatures
        self.verify_signatures(signed_tx)?;

        // 2. Check input objects exist at correct versions
        self.verify_input_objects(&signed_tx.input_objects)?;

        // 3. Check authorization for all input objects
        self.verify_authorization(signed_tx)?;

        // 4. Execute the transaction logic
        let (consumed, created) = self.execute_transaction_logic(signed_tx, proposing_validator)?;

        // 5. Create effects
        let effects = TransactionEffects::success(
            signed_tx.digest(),
            consumed,
            created,
            100, // Fixed gas cost for now
        );

        // Record metrics
        let tx_type = match &signed_tx.transaction {
            Transaction::CreateAccount { .. } => TransactionType::CreateAccount,
            Transaction::SettleDirectly { .. } => TransactionType::SettleDirectly,
            Transaction::PostJob { .. } => TransactionType::PostJob,
            Transaction::ClaimJob { .. } => TransactionType::ClaimJob,
            Transaction::CommitResult { .. } => TransactionType::CommitResult,
            Transaction::FinalizeJob { .. } => TransactionType::FinalizeJob,
            Transaction::AbortJob { .. } => TransactionType::AbortJob,
            Transaction::ResetBudget { .. } => TransactionType::ResetBudget,
        };

        observability::record_transaction(tx_type, true, start.elapsed().as_secs_f64());
        debug!("Transaction executed successfully");

        Ok(effects)
    }

    /// Verify all signatures on a transaction
    fn verify_signatures(&self, signed_tx: &SignedTransaction) -> Result<(), ExecutionError> {
        // Primary signature is always verified (stubbed in our implementation)
        // In reality: signed_tx.signer.verify(&signed_tx.digest(), &signed_tx.signature)?

        // Check additional signatures for multi-party transactions
        if signed_tx.transaction.requires_multi_sig() {
            let required_signers = signed_tx.transaction.required_signers();
            let additional_sigs = signed_tx.additional_signatures.as_ref().ok_or_else(|| {
                ExecutionError::InvalidTransaction(
                    "Multi-sig transaction missing additional signatures".to_string(),
                )
            })?;

            for required in &required_signers {
                if !additional_sigs.has_signed(required) {
                    return Err(ExecutionError::MissingSignature(*required));
                }
            }
        }

        Ok(())
    }

    /// Verify that all input objects exist at the specified versions
    fn verify_input_objects(&self, inputs: &[ObjectRef]) -> Result<(), ExecutionError> {
        for input in inputs {
            match self.state.get(&input.object_id) {
                None => return Err(ExecutionError::ObjectNotFound(input.object_id)),
                Some(obj) if obj.version != input.version => {
                    return Err(ExecutionError::VersionMismatch(
                        input.object_id,
                        input.version,
                        obj.version,
                    ));
                }
                Some(_) => {} // Object exists at correct version
            }
        }
        Ok(())
    }

    /// Verify that the transaction signers are authorized for all input objects
    fn verify_authorization(&self, signed_tx: &SignedTransaction) -> Result<(), ExecutionError> {
        // Special case for ClaimJob: provider may not be authorized yet for open bounties
        if let Transaction::ClaimJob { escrow_id } = &signed_tx.transaction {
            // For ClaimJob, we need to check if this is an open bounty
            let escrow_meta = self
                .state
                .get(escrow_id)
                .ok_or(ExecutionError::ObjectNotFound(*escrow_id))?;

            if let Some(escrow) = escrow_meta.object.as_job_escrow() {
                if escrow.provider.is_none() {
                    // Open bounty - skip authorization check for escrow object
                    // Still check other objects (like provider account)
                    for input in &signed_tx.input_objects {
                        if input.object_id != *escrow_id {
                            let obj = self
                                .state
                                .get(&input.object_id)
                                .ok_or(ExecutionError::ObjectNotFound(input.object_id))?;

                            if !obj.is_authorized(&signed_tx.signer) {
                                return Err(ExecutionError::PermissionDenied(format!(
                                    "No authorized signer for object {}",
                                    input.object_id
                                )));
                            }
                        }
                    }
                    return Ok(());
                }
            }
        }

        // For all other cases, check standard authorization
        for input in &signed_tx.input_objects {
            let obj = self
                .state
                .get(&input.object_id)
                .ok_or(ExecutionError::ObjectNotFound(input.object_id))?;

            let mut authorized = false;

            // Check primary signer
            if obj.is_authorized(&signed_tx.signer) {
                authorized = true;
            }

            // Check additional signers
            if let Some(additional) = &signed_tx.additional_signatures {
                for signer in &additional.signers {
                    if obj.is_authorized(signer) {
                        authorized = true;
                        break;
                    }
                }
            }

            if !authorized {
                return Err(ExecutionError::PermissionDenied(format!(
                    "No authorized signer for object {}",
                    input.object_id
                )));
            }
        }

        Ok(())
    }

    /// Execute the core logic of a transaction
    fn execute_transaction_logic(
        &mut self,
        signed_tx: &SignedTransaction,
        proposing_validator: Pubkey,
    ) -> Result<(Vec<ObjectRef>, Vec<ObjectId>), ExecutionError> {
        match &signed_tx.transaction {
            Transaction::CreateAccount { initial_balance } => {
                self.execute_create_account(&signed_tx.signer, *initial_balance)
            }

            Transaction::SettleDirectly {
                provider, payment, ..
            } => self.execute_settle_directly(signed_tx, provider, *payment, proposing_validator),

            Transaction::PostJob {
                provider,
                agreement_hash,
                job_spec_hash,
                payment,
                provider_bond_required,
                claim_deadline_delta,
                commit_deadline_delta,
                finalization_delay,
            } => self.execute_post_job(
                signed_tx,
                PostJobParams {
                    provider: *provider,
                    agreement_hash: *agreement_hash,
                    job_spec_hash: *job_spec_hash,
                    payment: *payment,
                    provider_bond_required: *provider_bond_required,
                    claim_deadline_delta: *claim_deadline_delta,
                    _commit_deadline_delta: *commit_deadline_delta,
                    _finalization_delay: *finalization_delay,
                },
            ),

            Transaction::ClaimJob { escrow_id } => self.execute_claim_job(signed_tx, escrow_id),

            Transaction::CommitResult {
                escrow_id,
                result_hash,
            } => self.execute_commit_result(signed_tx, escrow_id, *result_hash),

            Transaction::FinalizeJob { escrow_id } => self.execute_finalize_job(escrow_id),

            Transaction::AbortJob { escrow_id } => self.execute_abort_job(signed_tx, escrow_id),

            Transaction::ResetBudget {
                budget_certificates,
            } => self.execute_reset_budget(signed_tx, budget_certificates),
        }
    }

    /// Execute CreateAccount transaction
    fn execute_create_account(
        &mut self,
        creator: &Pubkey,
        initial_balance: Amount,
    ) -> Result<(Vec<ObjectRef>, Vec<ObjectId>), ExecutionError> {
        // Generate new object ID
        let account_id = self.generate_object_id();

        // Create account object
        let mut account = HellasAccount::new(initial_balance);
        account.initialize_budgets(&self.validators, self.byzantine_tolerance);

        // Initialize channels for the new account
        self.channel_manager.init_account_channels(
            account_id,
            account.balance,
            0, // Initial version
        );

        // Also initialize our local validator's channel
        self.validator_local_state.init_channel(
            account_id,
            self.channel_manager.calculate_max_budget(account.balance),
            0,
        );

        // Create metadata
        let metadata = ObjectMetadata::new(account_id, *creator, Object::Account(account));

        // Insert into state
        self.state.insert(account_id, metadata);

        // Add to account lookup
        self.account_lookup.insert(*creator, account_id);

        Ok((vec![], vec![account_id]))
    }

    /// Execute SettleDirectly transaction (interactive flow)
    fn execute_settle_directly(
        &mut self,
        signed_tx: &SignedTransaction,
        _provider: &Pubkey,
        payment: Amount,
        _proposing_validator: Pubkey,
    ) -> Result<(Vec<ObjectRef>, Vec<ObjectId>), ExecutionError> {
        // Get requestor and provider accounts
        let requestor_ref = &signed_tx.input_objects[0];
        let provider_ref = &signed_tx.input_objects[1];

        let mut requestor_meta = self
            .state
            .get(&requestor_ref.object_id)
            .ok_or(ExecutionError::ObjectNotFound(requestor_ref.object_id))?
            .clone();
        let mut provider_meta = self
            .state
            .get(&provider_ref.object_id)
            .ok_or(ExecutionError::ObjectNotFound(provider_ref.object_id))?
            .clone();

        // Extract accounts
        let requestor_account = requestor_meta
            .object
            .as_mut_account()
            .ok_or(ExecutionError::ObjectTypeMismatch)?;
        let provider_account = provider_meta
            .object
            .as_mut_account()
            .ok_or(ExecutionError::ObjectTypeMismatch)?;

        // Use bounded counter execution for concurrent payment
        // Each validator uses their own channel to process the payment
        self.validator_local_state
            .try_spend(requestor_ref.object_id, payment, signed_tx.digest())
            .map_err(|_| ExecutionError::InsufficientBudget)?;

        // Note: The account balance is NOT updated here - that happens during ResetBudget
        // This is the key to concurrent execution!

        // Update provider balance
        provider_account.balance = provider_account
            .balance
            .checked_add(payment)
            .ok_or_else(|| ExecutionError::InvalidTransaction("Balance overflow".to_string()))?;

        // Update versions
        requestor_meta.version += 1;
        provider_meta.version += 1;

        // Update nonces
        requestor_account.nonce += 1;

        // Important: We don't update the requestor's balance here!
        // The balance is only reconciled during ResetBudget transactions

        // Commit changes
        self.state.insert(requestor_ref.object_id, requestor_meta);
        self.state.insert(provider_ref.object_id, provider_meta);

        Ok((
            vec![*requestor_ref, *provider_ref],
            vec![requestor_ref.object_id, provider_ref.object_id],
        ))
    }

    /// Execute PostJob transaction
    fn execute_post_job(
        &mut self,
        signed_tx: &SignedTransaction,
        params: PostJobParams,
    ) -> Result<(Vec<ObjectRef>, Vec<ObjectId>), ExecutionError> {
        // Get requestor account
        let requestor_ref = &signed_tx.input_objects[0];
        let mut requestor_meta = self
            .state
            .get(&requestor_ref.object_id)
            .ok_or(ExecutionError::ObjectNotFound(requestor_ref.object_id))?
            .clone();

        let requestor_account = requestor_meta
            .object
            .as_mut_account()
            .ok_or(ExecutionError::ObjectTypeMismatch)?;

        // Check balance
        if requestor_account.balance < params.payment {
            return Err(ExecutionError::InsufficientBalance {
                have: requestor_account.balance,
                need: params.payment,
            });
        }

        // Deduct payment
        requestor_account.balance = requestor_account
            .balance
            .checked_sub(params.payment)
            .ok_or(ExecutionError::InsufficientBalance {
                have: requestor_account.balance,
                need: params.payment,
            })?;
        requestor_account.nonce += 1;
        requestor_meta.version += 1;

        // Create job escrow (provider optional)
        let escrow_id = self.generate_object_id();
        let escrow = JobEscrow::new(
            params.agreement_hash,
            signed_tx.signer,
            params.provider,
            params.job_spec_hash,
            params.payment,
            params.provider_bond_required,
            self.current_height + params.claim_deadline_delta,
        );

        let mut escrow_meta =
            ObjectMetadata::new(escrow_id, signed_tx.signer, Object::JobEscrow(escrow));
        // Add provider as an authorized party for the escrow if pre-selected
        if let Some(provider_pubkey) = params.provider {
            escrow_meta.add_owner(provider_pubkey);
        }

        // Update state
        self.state.insert(requestor_ref.object_id, requestor_meta);
        self.state.insert(escrow_id, escrow_meta);

        Ok((
            vec![*requestor_ref],
            vec![requestor_ref.object_id, escrow_id],
        ))
    }

    /// Helper to generate unique object IDs
    fn generate_object_id(&mut self) -> ObjectId {
        self.object_counter += 1;
        let mut bytes = [0u8; 32];
        bytes[..8].copy_from_slice(&self.object_counter.to_le_bytes());
        bytes[8..16].copy_from_slice(&self.current_height.to_le_bytes());
        ObjectId::new(bytes)
    }

    // Additional execution methods for other transaction types...
    fn execute_claim_job(
        &mut self,
        signed_tx: &SignedTransaction,
        escrow_id: &ObjectId,
    ) -> Result<(Vec<ObjectRef>, Vec<ObjectId>), ExecutionError> {
        trace!("Executing ClaimJob for escrow {}", escrow_id);

        // Get escrow and provider account
        let escrow_ref = &signed_tx.input_objects[0];
        let provider_ref = &signed_tx.input_objects[1];

        let mut escrow_meta = self
            .state
            .get(&escrow_ref.object_id)
            .ok_or(ExecutionError::ObjectNotFound(escrow_ref.object_id))?
            .clone();
        let mut provider_meta = self
            .state
            .get(&provider_ref.object_id)
            .ok_or(ExecutionError::ObjectNotFound(provider_ref.object_id))?
            .clone();

        // Verify escrow state first
        {
            let escrow = escrow_meta
                .object
                .as_job_escrow()
                .ok_or(ExecutionError::ObjectTypeMismatch)?;

            if escrow.status != JobStatus::Posted {
                return Err(ExecutionError::InvalidJobStatus {
                    expected: JobStatus::Posted,
                    actual: escrow.status,
                });
            }
        }

        // Handle provider authorization and update escrow
        let needs_owner_update = {
            let escrow = escrow_meta
                .object
                .as_mut_job_escrow()
                .ok_or(ExecutionError::ObjectTypeMismatch)?;

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
            .ok_or(ExecutionError::ObjectTypeMismatch)?;
        let provider_account = provider_meta
            .object
            .as_mut_account()
            .ok_or(ExecutionError::ObjectTypeMismatch)?;

        // Check claim deadline
        if self.current_height > escrow.claim_deadline {
            return Err(ExecutionError::DeadlineMissed {
                current: self.current_height,
                deadline: escrow.claim_deadline,
            });
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

        // Update versions and nonces
        escrow_meta.version += 1;
        provider_meta.version += 1;
        provider_account.nonce += 1;

        // Commit changes
        self.state.insert(escrow_ref.object_id, escrow_meta);
        self.state.insert(provider_ref.object_id, provider_meta);

        info!("Job claimed successfully by provider {}", provider_pubkey);

        Ok((
            vec![*escrow_ref, *provider_ref],
            vec![escrow_ref.object_id, provider_ref.object_id],
        ))
    }

    fn execute_commit_result(
        &mut self,
        signed_tx: &SignedTransaction,
        escrow_id: &ObjectId,
        result_hash: Hash,
    ) -> Result<(Vec<ObjectRef>, Vec<ObjectId>), ExecutionError> {
        trace!("Executing CommitResult for escrow {}", escrow_id);

        // Get escrow object
        let escrow_ref = &signed_tx.input_objects[0];
        let mut escrow_meta = self
            .state
            .get(&escrow_ref.object_id)
            .ok_or(ExecutionError::ObjectNotFound(escrow_ref.object_id))?
            .clone();

        let escrow = escrow_meta
            .object
            .as_mut_job_escrow()
            .ok_or(ExecutionError::ObjectTypeMismatch)?;

        // Verify escrow state
        if escrow.status != JobStatus::Claimed {
            return Err(ExecutionError::InvalidJobStatus {
                expected: JobStatus::Claimed,
                actual: escrow.status,
            });
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
            return Err(ExecutionError::DeadlineMissed {
                current: self.current_height,
                deadline: escrow.commit_deadline,
            });
        }

        // Update escrow with result
        escrow.result_hash = Some(result_hash);
        escrow.status = JobStatus::Committed;
        escrow.committed_at = Some(self.current_height);
        escrow.finalize_after = self.current_height + escrow.finalization_delay;

        // Update version
        escrow_meta.version += 1;

        // Commit changes
        self.state.insert(escrow_ref.object_id, escrow_meta);

        info!("Result committed for job {}", escrow_id);

        Ok((vec![*escrow_ref], vec![escrow_ref.object_id]))
    }

    fn execute_finalize_job(
        &mut self,
        escrow_id: &ObjectId,
    ) -> Result<(Vec<ObjectRef>, Vec<ObjectId>), ExecutionError> {
        trace!("Executing FinalizeJob for escrow {}", escrow_id);

        // Get escrow object (no input refs needed, anyone can finalize)
        let mut escrow_meta = self
            .state
            .get(escrow_id)
            .ok_or(ExecutionError::ObjectNotFound(*escrow_id))?
            .clone();

        let escrow = escrow_meta
            .object
            .as_mut_job_escrow()
            .ok_or(ExecutionError::ObjectTypeMismatch)?;

        // Verify escrow state
        if escrow.status != JobStatus::Committed {
            return Err(ExecutionError::InvalidJobStatus {
                expected: JobStatus::Committed,
                actual: escrow.status,
            });
        }

        // Check finalization deadline has passed
        if self.current_height < escrow.finalize_after {
            return Err(ExecutionError::DeadlineNotReached {
                current: self.current_height,
                deadline: escrow.finalize_after,
            });
        }

        // Get provider account to release payment and bond
        let provider = escrow.provider.ok_or_else(|| {
            ExecutionError::InvalidTransaction("No provider set for this job".to_string())
        })?;
        let provider_account_id = self
            .account_lookup
            .get(&provider)
            .ok_or_else(|| ExecutionError::ObjectNotFound(ObjectId::new([0; 32])))?;
        let mut provider_meta = self
            .state
            .get(provider_account_id)
            .ok_or(ExecutionError::ObjectNotFound(*provider_account_id))?
            .clone();

        let provider_account = provider_meta
            .object
            .as_mut_account()
            .ok_or(ExecutionError::ObjectTypeMismatch)?;

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

        // Update versions
        escrow_meta.version += 1;
        provider_meta.version += 1;

        // Commit changes
        self.state.insert(*escrow_id, escrow_meta);
        self.state.insert(*provider_account_id, provider_meta);

        // Update metrics
        gauge!("hellas_escrows_active", -1.0);
        info!("Job finalized, payment released to provider");

        Ok((vec![], vec![*escrow_id, *provider_account_id]))
    }

    fn execute_abort_job(
        &mut self,
        signed_tx: &SignedTransaction,
        escrow_id: &ObjectId,
    ) -> Result<(Vec<ObjectRef>, Vec<ObjectId>), ExecutionError> {
        trace!("Executing AbortJob for escrow {}", escrow_id);

        // Get escrow object
        let escrow_ref = &signed_tx.input_objects[0];
        let mut escrow_meta = self
            .state
            .get(&escrow_ref.object_id)
            .ok_or(ExecutionError::ObjectNotFound(escrow_ref.object_id))?
            .clone();

        let escrow = escrow_meta
            .object
            .as_mut_job_escrow()
            .ok_or(ExecutionError::ObjectTypeMismatch)?;

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
                return Err(ExecutionError::InvalidJobStatus {
                    expected: JobStatus::Posted,
                    actual: JobStatus::Committed,
                });
            }
            _ => {
                return Err(ExecutionError::InvalidJobStatus {
                    expected: JobStatus::Posted,
                    actual: escrow.status,
                });
            }
        };

        // Process refunds
        if refund_to_requestor > Amount::ZERO {
            let requestor_account_id = self
                .account_lookup
                .get(&escrow.requestor)
                .ok_or_else(|| ExecutionError::ObjectNotFound(ObjectId::new([0; 32])))?;
            let mut requestor_meta = self
                .state
                .get(requestor_account_id)
                .ok_or(ExecutionError::ObjectNotFound(*requestor_account_id))?
                .clone();
            let requestor_account = requestor_meta
                .object
                .as_mut_account()
                .ok_or(ExecutionError::ObjectTypeMismatch)?;
            requestor_account.balance = requestor_account
                .balance
                .checked_add(refund_to_requestor)
                .ok_or_else(|| {
                    ExecutionError::InvalidTransaction("Balance overflow".to_string())
                })?;
            requestor_meta.version += 1;
            self.state.insert(*requestor_account_id, requestor_meta);
        }

        if refund_to_provider > Amount::ZERO {
            let provider = escrow.provider.ok_or_else(|| {
                ExecutionError::InvalidTransaction("No provider set for this job".to_string())
            })?;
            let provider_account_id = self
                .account_lookup
                .get(&provider)
                .ok_or_else(|| ExecutionError::ObjectNotFound(ObjectId::new([0; 32])))?;
            let mut provider_meta = self
                .state
                .get(provider_account_id)
                .ok_or(ExecutionError::ObjectNotFound(*provider_account_id))?
                .clone();
            let provider_account = provider_meta
                .object
                .as_mut_account()
                .ok_or(ExecutionError::ObjectTypeMismatch)?;
            provider_account.balance = provider_account
                .balance
                .checked_add(refund_to_provider)
                .ok_or_else(|| {
                    ExecutionError::InvalidTransaction("Balance overflow".to_string())
                })?;
            provider_meta.version += 1;
            self.state.insert(*provider_account_id, provider_meta);
        }

        // Update escrow status
        escrow.status = JobStatus::Aborted;
        escrow.aborted_at = Some(self.current_height);
        escrow_meta.version += 1;

        // Commit changes
        self.state.insert(escrow_ref.object_id, escrow_meta);

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

        Ok((vec![*escrow_ref], vec![escrow_ref.object_id]))
    }

    fn execute_reset_budget(
        &mut self,
        signed_tx: &SignedTransaction,
        certificates: &[crate::transactions::BudgetCertificate],
    ) -> Result<(Vec<ObjectRef>, Vec<ObjectId>), ExecutionError> {
        // Get the account to reset
        let account_ref = &signed_tx.input_objects[0];
        let mut account_meta = self
            .state
            .get(&account_ref.object_id)
            .ok_or(ExecutionError::ObjectNotFound(account_ref.object_id))?
            .clone();

        let account = account_meta
            .object
            .as_mut_account()
            .ok_or(ExecutionError::ObjectTypeMismatch)?;

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

        // Update metadata
        account_meta.version += 1;
        account.nonce += 1;

        // Commit changes
        self.state.insert(account_ref.object_id, account_meta);

        Ok((vec![*account_ref], vec![account_ref.object_id]))
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

        let effects = engine
            .execute_transaction(&signed_tx, Pubkey::test(1))
            .unwrap();
        assert!(effects.success);
        assert_eq!(effects.created_objects.len(), 1);

        // Verify account was created
        let account_id = effects.created_objects[0];
        let account_meta = engine.state.get(&account_id).unwrap();
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
        let effects = engine
            .execute_transaction(&signed_tx, Pubkey::test(1))
            .unwrap();
        let requestor_account_id = effects.created_objects[0];

        // Create provider account
        let provider = Pubkey::test(11);
        let tx = Transaction::CreateAccount {
            initial_balance: Amount::from_units(500),
        };
        let signed_tx = SignedTransaction::new_single_signer(provider, tx, vec![], 0);
        let effects = engine
            .execute_transaction(&signed_tx, Pubkey::test(1))
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
        let signed_tx = SignedTransaction::new_single_signer(requestor, tx, input_refs, 1);
        let effects = engine
            .execute_transaction(&signed_tx, Pubkey::test(1))
            .unwrap();
        assert!(effects.success);

        // Find the escrow ID
        let escrow_id = effects
            .created_objects
            .iter()
            .find(|id| {
                engine
                    .state
                    .get(id)
                    .unwrap()
                    .object
                    .as_job_escrow()
                    .is_some()
            })
            .unwrap();

        // Verify escrow has no provider set
        let escrow_meta = engine.state.get(escrow_id).unwrap();
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
        let effects = engine
            .execute_transaction(&signed_tx, Pubkey::test(1))
            .unwrap();
        assert!(effects.success);

        // Verify provider is now set
        let escrow_meta = engine.state.get(escrow_id).unwrap();
        let escrow = escrow_meta.object.as_job_escrow().unwrap();
        assert_eq!(escrow.provider, Some(provider));
        assert_eq!(escrow.status, JobStatus::Claimed);
        assert_eq!(escrow.provider_bond_locked, Amount::from_units(50));

        // Verify provider is now authorized on the escrow
        assert!(escrow_meta.is_authorized(&provider));
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
            engine
                .execute_transaction(&signed_tx, Pubkey::test(1))
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
        let effects = engine
            .execute_transaction(&signed_tx, Pubkey::test(1))
            .unwrap();
        assert!(effects.success);

        // Find the escrow ID
        let escrow_id = effects
            .created_objects
            .iter()
            .find(|id| {
                engine
                    .state
                    .get(id)
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
        let result = engine.execute_transaction(&signed_tx, Pubkey::test(1));
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
        let effects = engine
            .execute_transaction(&signed_tx, Pubkey::test(1))
            .unwrap();
        assert!(effects.success);
    }
}
