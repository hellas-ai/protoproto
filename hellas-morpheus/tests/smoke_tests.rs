//! Basic smoke tests for the Morpheus consensus protocol
//!
//! These tests verify basic functionality including:
//! - Message handling and queuing
//! - Process interaction
//! - Time advancement
//! - Transaction generation policies

use hellas_morpheus::test_harness::{MockHarness, TestTransaction, TxGenPolicy};
use hellas_morpheus::*;
use hellas_morpheus::{RedbBulkStore, RedbSnapshotStore};
use std::sync::Arc;

/// Helper function to create a test harness with default storage
fn create_test_harness(
    num_parties: usize,
) -> MockHarness<RedbBulkStore<TestTransaction>, RedbSnapshotStore> {
    // Create storage factories for the harness
    // Use an in-memory database for each process
    let db = Arc::new(
        redb::Builder::new()
            .create_with_backend(redb::backends::InMemoryBackend::new())
            .unwrap(),
    );

    let db_clone = db.clone();
    let create_bulk = move |_db: &redb::Database| RedbBulkStore::new(db_clone.clone()).unwrap();
    let db_clone = db.clone();
    let create_snapshot =
        move |_db: &redb::Database| RedbSnapshotStore::new(db_clone.clone()).unwrap();

    MockHarness::create_test_setup(num_parties, create_bulk, create_snapshot, None)
}

#[test_log::test]
fn test_mock_harness_enqueue_message() {
    let mut harness = create_test_harness(2);

    // Initial state - no pending messages
    assert_eq!(harness.pending_messages.len(), 0);

    // Create a dummy message using ThreshPartial::from_data
    let dummy_message = Message::EndView(Arc::new(ThreshPartial::from_data(
        ViewNum(0),
        &harness.processes.get(&Identity(1)).unwrap().kb,
    )));

    // Enqueue a message for a specific destination
    harness.enqueue_message(dummy_message.clone(), Identity(1), Some(Identity(2)));

    // Check that the message was enqueued
    assert_eq!(harness.pending_messages.len(), 1);

    // Enqueue a broadcast message
    harness.enqueue_message(dummy_message, Identity(1), None);

    // Check that the message was enqueued
    assert_eq!(harness.pending_messages.len(), 2);
}

#[test_log::test]
fn test_basic_txgen() {
    assert!(cfg!(debug_assertions));

    let mut harness = create_test_harness(3);

    harness
        .tx_gen_policy
        .insert(Identity(2), TxGenPolicy::EveryNSteps { n: 3 });

    harness
        .tx_gen_policy
        .insert(Identity(3), TxGenPolicy::EveryNSteps { n: 2 });

    // Let the system run for a while.
    harness.run(2 * 3 * 5);

    // Verify block production
    let process = harness.processes.get(&Identity(2)).unwrap();
    let block_count = process.event_log.recorded_entries;

    println!("Process recorded {} events", block_count);

    // Verify that blocks were produced
    assert!(block_count > 0, "Should have recorded some events");

    harness
        .verify_all_snapshots()
        .expect("Snapshot verification failed");
}

#[test_log::test]
fn test_basic_integration() {
    let mut harness = create_test_harness(3);

    // Initial state
    assert_eq!(harness.time, 0);
    assert_eq!(harness.processes.len(), 3);

    // Create a simple EndView message using ThreshPartial::from_data
    let end_view_message = Message::EndView(Arc::new(ThreshPartial::from_data(
        ViewNum(0),
        &harness.processes.get(&Identity(1)).unwrap().kb,
    )));

    // Broadcast the message
    harness.enqueue_message(end_view_message, Identity(1), None);

    // Run for multiple steps to simulate system behavior
    harness.run(10);

    // After 10 steps, time should have advanced
    assert_eq!(harness.time, 1000);

    // Each process should have its time updated correctly
    for (_, process) in harness.processes.iter() {
        assert_eq!(process.timeout_manager.current_time, 1000);
    }

    harness
        .verify_all_snapshots()
        .expect("Snapshot verification failed");
}

#[test_log::test]
fn test_directed_message_flow() {
    let mut harness = create_test_harness(3);

    // Create messages flowing from process1 to process2
    let message1 = Message::EndView(Arc::new(ThreshPartial::from_data(
        ViewNum(0),
        &harness.processes.get(&Identity(1)).unwrap().kb,
    )));

    // Create messages flowing from process2 to process3
    let message2 = Message::EndView(Arc::new(ThreshPartial::from_data(
        ViewNum(1),
        &harness.processes.get(&Identity(2)).unwrap().kb,
    )));

    // Enqueue the directed messages
    harness.enqueue_message(message1, Identity(1), Some(Identity(2)));
    harness.enqueue_message(message2, Identity(2), Some(Identity(3)));

    // Step once to process the messages
    harness.step();

    // Time should have advanced
    assert_eq!(harness.time, 100);

    // All processes should have their time updated
    for (_, process) in harness.processes.iter() {
        assert_eq!(process.timeout_manager.current_time, 100);
    }

    harness
        .verify_all_snapshots()
        .expect("Snapshot verification failed");
}

#[test_log::test]
fn test_process_round_no_messages() {
    let mut harness = create_test_harness(1);

    // Initial state - no pending messages
    assert_eq!(harness.pending_messages.len(), 0);

    // Process a round should not make progress without messages
    let made_progress = harness.process_round();
    assert_eq!(made_progress, false);
}

#[test_log::test]
fn test_check_all_timeouts() {
    let mut harness = create_test_harness(1);

    // Check timeouts
    let made_progress = harness.check_all_timeouts();

    // There should be nothing to do
    assert_eq!(made_progress, false);
}

#[test_log::test]
fn test_basic_process_interaction() {
    let mut harness = create_test_harness(2);

    // Create a simple EndView message to trigger some interaction
    let end_view_message = Message::EndView(Arc::new(ThreshPartial::from_data(
        ViewNum(0),
        &harness.processes.get(&Identity(1)).unwrap().kb,
    )));

    // Enqueue the message for process2
    harness.enqueue_message(end_view_message, Identity(1), Some(Identity(2)));

    // Process a round
    let made_progress = harness.process_round();
    harness.process_round();

    assert_eq!(made_progress, true);

    // Message queue should be empty after processing
    assert_eq!(harness.pending_messages.len(), 0);

    harness
        .verify_all_snapshots()
        .expect("Snapshot verification failed");
}

#[test_log::test]
fn test_broadcast_message() {
    let mut harness = create_test_harness(3);

    // Create a simple EndView message to broadcast
    let end_view_message = Message::EndView(Arc::new(ThreshPartial::from_data(
        ViewNum(0),
        &harness.processes.get(&Identity(1)).unwrap().kb,
    )));

    // Broadcast the message (destination = None)
    harness.enqueue_message(end_view_message, Identity(1), None);

    // In the case of a broadcast, the message should be delivered to all processes
    // In our mock harness implementation, the broadcast is done during process_round
    // and the message is consumed only once, so pending_messages should contain just one item
    assert_eq!(harness.pending_messages.len(), 1);

    // p1 processes the EndView and broadcasts it
    harness.process_round();
    // other processes receive and broadcast as well
    harness.process_round();

    // After processing, the message queue should be empty
    assert_eq!(harness.pending_messages.len(), 0);

    harness
        .verify_all_snapshots()
        .expect("Snapshot verification failed");
}
