//! Tests for the invariants checking system
//!
//! This module tests that the invariants checking correctly identifies various
//! protocol violations and maintains consistency.

mod common;

use common::*;
use hellas_morpheus::test_harness::TestTransaction;
use hellas_morpheus::*;
use hellas_morpheus::{
    ConsensusState, InvariantCheckConfig, InvariantChecker, RedbBulkStore, RedbSnapshotStore,
    StorageInvariant,
};
use im::HashMap;
use std::sync::Arc;

#[test]
fn test_fresh_process_has_no_violations() {
    let (process, db, checker) = create_test_process_with_checker();

    // Get consensus state from process
    let consensus_state = ConsensusState {
        current_view: process.view_manager.current_view(),
        current_phase: process
            .view_manager
            .phase(process.view_manager.current_view()),
        view_entry_time: process.view_manager.view_entry_time,
        tips: process.tips_refs.iter().cloned().collect(),
        max_1qc: process.max_1qc_ref.clone(),
        finalized_blocks: process.finalized_blocks.clone(),
        unfinalized_qcs: process.unfinalized_qcs.clone(),
        leader_blocks_by_view: im::HashMap::new(),
        unfinalized_leader_by_view: im::HashMap::new(),
    };

    let violations = checker.check_invariants(
        &process.bulk_store,
        &process.snapshot_store,
        &process.view_cache,
        &consensus_state,
    );

    // Debug: print any violations found
    for violation in &violations {
        println!("Violation found: {:?}", violation);
    }

    assert!(
        violations.is_empty(),
        "Fresh process should have no invariant violations. Found: {:?}",
        violations
    );
}

#[test]
fn test_invariants_after_normal_operation() {
    let (mut process, db, checker) = create_test_process_with_checker();

    // After normal operations like time updates, invariants should hold
    process.set_now(&db, 1000).unwrap();

    // Get consensus state from process
    let consensus_state = ConsensusState {
        current_view: process.view_manager.current_view(),
        current_phase: process
            .view_manager
            .phase(process.view_manager.current_view()),
        view_entry_time: process.view_manager.view_entry_time,
        tips: process.tips_refs.iter().cloned().collect(),
        max_1qc: process.max_1qc_ref.clone(),
        finalized_blocks: process.finalized_blocks.clone(),
        unfinalized_qcs: process.unfinalized_qcs.clone(),
        leader_blocks_by_view: im::HashMap::new(),
        unfinalized_leader_by_view: im::HashMap::new(),
    };

    let violations = checker.check_invariants(
        &process.bulk_store,
        &process.snapshot_store,
        &process.view_cache,
        &consensus_state,
    );

    assert!(
        violations.is_empty(),
        "Process should maintain invariants after time update"
    );
}

#[test]
fn test_paranoid_invariant_checking() {
    let db = create_test_db();
    let (kb, _, _) = setup_test_crypto(4);

    let bulk_store = RedbBulkStore::new(db.clone()).unwrap();
    let snapshot_store = RedbSnapshotStore::new(db.clone()).unwrap();

    // Create process with paranoid invariant checking
    let process: MorpheusProcess<TestTransaction, _, _> = MorpheusProcess::new(
        &db,
        kb,
        Identity(1),
        4,
        1,
        bulk_store,
        snapshot_store,
        Some(InvariantCheckConfig::paranoid()),
    )
    .unwrap();

    // The process should automatically check invariants after every operation
    // and log any violations through the tracing system

    // Verify the process has an invariant checker configured
    assert!(process.invariant_checker.is_some());
}

#[test]
fn test_debug_invariant_checking() {
    let db = create_test_db();
    let (kb, _, _) = setup_test_crypto(4);

    let bulk_store = RedbBulkStore::new(db.clone()).unwrap();
    let snapshot_store = RedbSnapshotStore::new(db.clone()).unwrap();

    // Create process with debug invariant checking
    let process: MorpheusProcess<TestTransaction, _, _> = MorpheusProcess::new(
        &db,
        kb,
        Identity(1),
        4,
        1,
        bulk_store,
        snapshot_store,
        Some(InvariantCheckConfig::debug()),
    )
    .unwrap();

    // The process should check invariants more frequently in debug mode
    assert!(process.invariant_checker.is_some());
}

#[test]
fn test_no_invariant_checking() {
    let db = create_test_db();
    let (kb, _, _) = setup_test_crypto(4);

    let bulk_store = RedbBulkStore::new(db.clone()).unwrap();
    let snapshot_store = RedbSnapshotStore::new(db.clone()).unwrap();

    // Create process without invariant checking
    let process: MorpheusProcess<TestTransaction, _, _> = MorpheusProcess::new(
        &db,
        kb,
        Identity(1),
        4,
        1,
        bulk_store,
        snapshot_store,
        None, // No invariant checking
    )
    .unwrap();

    // The process should not have an invariant checker
    assert!(process.invariant_checker.is_none());
}

// Helper functions

/// Create a test process with an invariant checker for testing
fn create_test_process_with_checker() -> (
    MorpheusProcess<TestTransaction, RedbBulkStore<TestTransaction>, RedbSnapshotStore>,
    Arc<redb::Database>,
    InvariantChecker,
) {
    let db = create_test_db();
    let (kb, _, _) = setup_test_crypto(4);

    let bulk_store = RedbBulkStore::new(db.clone()).unwrap();
    let snapshot_store = RedbSnapshotStore::new(db.clone()).unwrap();

    let checker = InvariantChecker::new(InvariantCheckConfig::paranoid());

    let process = MorpheusProcess::new(
        &db,
        kb,
        Identity(1),
        4,
        1,
        bulk_store,
        snapshot_store,
        Some(InvariantCheckConfig::paranoid()),
    )
    .unwrap();

    (process, db, checker)
}
