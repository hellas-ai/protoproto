//! Tests for the test harness functionality
//!
//! These tests verify that the test harness itself works correctly, including:
//! - Event sourcing and replay
//! - Snapshot verification
//! - Deterministic execution

mod common;

use ark_serialize::CanonicalSerialize;
use common::*;
use hellas_morpheus::snapshots_table_default;
use hellas_morpheus::storage::{bulk::RedbBulkStore, snapshot::RedbSnapshotStore};
use hellas_morpheus::test_harness::{MockHarness, TestTransaction, TxGenPolicy};
use hellas_morpheus::{
    BlockKey, BlockType, Message, SlotNum, ThreshPartial, ThreshSigned, VoteData,
};
use hellas_morpheus::{Identity, MorpheusProcess, ViewNum};
use redb::ReadableTable;
use std::sync::Arc;

/// Helper function to create a test harness with default storage
fn create_test_harness(
    num_parties: usize,
) -> MockHarness<RedbBulkStore<TestTransaction>, RedbSnapshotStore> {
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
fn test_multiple_rounds_end_view() {
    let mut harness = create_test_harness(3);

    // Create a few simple messages
    let message1 = Message::EndView(Arc::new(ThreshPartial::from_data(
        ViewNum(0),
        &harness.processes.get(&Identity(1)).unwrap().kb,
    )));

    let message2 = Message::EndView(Arc::new(ThreshPartial::from_data(
        ViewNum(1),
        &harness.processes.get(&Identity(2)).unwrap().kb,
    )));

    // Enqueue the messages for specific destinations
    harness.enqueue_message(message1, Identity(1), Some(Identity(2)));
    harness.enqueue_message(message2, Identity(2), Some(Identity(3)));

    // Initial queue length
    assert_eq!(harness.pending_messages.len(), 2);

    // Run multiple rounds until the queue is empty
    let mut rounds = 0;
    while !harness.pending_messages.is_empty() && rounds < 10 {
        harness.process_round();
        rounds += 1;
    }

    // Queue should be empty after processing
    assert_eq!(harness.pending_messages.len(), 0);

    // All processes should have recorded some events
    assert!(
        harness
            .processes
            .get(&Identity(1))
            .unwrap()
            .event_log
            .recorded_entries
            > 0
    );
    assert!(
        harness
            .processes
            .get(&Identity(2))
            .unwrap()
            .event_log
            .recorded_entries
            > 0
    );
    assert!(
        harness
            .processes
            .get(&Identity(3))
            .unwrap()
            .event_log
            .recorded_entries
            > 0
    );
}

#[test_log::test]
fn test_time_advancement_affects_processes() {
    let mut harness = create_test_harness(3);

    // Initial time should be 0 for harness and all processes
    assert_eq!(harness.time, 0);
    for (_, process) in harness.processes.iter() {
        assert_eq!(process.timeout_manager.current_time, 0);
    }

    // Advance time
    harness.advance_time();

    // Harness time should be updated
    assert_eq!(harness.time, 100);

    // All processes should have their time updated
    for (_, process) in harness.processes.iter() {
        assert_eq!(process.timeout_manager.current_time, 100);
    }
}

#[test_log::test]
fn test_complex_simulation() {
    let mut harness = create_test_harness(3);

    // Initial state
    assert_eq!(harness.time, 0);
    assert_eq!(harness.pending_messages.len(), 0);

    // Create a vote data for a test message
    let vote_data = VoteData {
        z: 1,
        for_which: BlockKey {
            type_: BlockType::Genesis,
            view: ViewNum(-1),
            height: 0,
            author: None,
            slot: SlotNum(0),
            hash: None,
        },
    };

    let p1_vote = ThreshPartial::from_data(
        vote_data.clone(),
        &harness.processes.get(&Identity(1)).unwrap().kb,
    );
    let p2_vote = ThreshPartial::from_data(
        vote_data.clone(),
        &harness.processes.get(&Identity(1)).unwrap().kb,
    );
    let agg = harness
        .processes
        .get(&Identity(1))
        .unwrap()
        .kb
        .hints_setup
        .as_ref()
        .expect("hints_setup should be present")
        .aggregator();
    let mut msg = Vec::new();
    vote_data.serialize_compressed(&mut msg).unwrap();
    // Create a QC message
    let qc_message = Message::QC(Arc::new(ThreshSigned {
        data: vote_data,
        signature: hints::sign_aggregate(
            &agg,
            hints::F::from(2),
            &[(1, p1_vote.signature), (2, p2_vote.signature)],
            &msg,
        )
        .unwrap(),
    }));

    // Broadcast the message
    harness.enqueue_message(qc_message, Identity(1), None);

    // Run for several steps
    harness.run(5);

    // Check final state after simulation
    assert_eq!(harness.time, 500);
}

#[test_log::test]
fn test_message_enqueue_and_processing() {
    let mut harness = create_test_harness(3);

    // Create a simple vote data
    let vote_data = VoteData {
        z: 0,
        for_which: BlockKey {
            type_: BlockType::Genesis,
            view: ViewNum(-1),
            height: 0,
            author: None,
            slot: SlotNum(0),
            hash: None,
        },
    };

    // Create a signed vote
    let signed_vote = ThreshPartial::from_data(
        vote_data.clone(),
        &harness.processes.get(&Identity(1)).unwrap().kb,
    );

    // Create a NewVote message
    let vote_message = Message::NewVote(Arc::new(signed_vote));

    // Enqueue the message for a specific destination
    harness.enqueue_message(vote_message, Identity(1), Some(Identity(2)));

    // Check initial queue length
    assert_eq!(harness.pending_messages.len(), 1);

    // Process the round
    harness.process_round();

    // Queue should be empty after processing
    assert_eq!(harness.pending_messages.len(), 0);
}

#[test]
fn test_snapshot_verification() {
    let mut harness = create_test_harness(4);

    harness.run(10);

    harness
        .verify_all_snapshots()
        .expect("Snapshot verification failed");
}

#[test_log::test]
fn test_snapshot_replay_determinism() {
    let mut harness = create_test_harness(3);

    // Configure transaction generation policy
    harness
        .tx_gen_policy
        .insert(Identity(1), TxGenPolicy::EveryNSteps { n: 2 });

    // Run for some steps to generate various event types
    harness.run(2);
    for (_, process) in harness.processes.iter() {
        let db = harness.dbs.get(&process.id).unwrap();
        process.event_log.save_snapshot(db, process).unwrap();
    }
    harness.run(3);

    // Take a snapshot for process 1
    let process1 = harness.processes.get(&Identity(1)).unwrap();
    let db1 = harness.dbs.get(&Identity(1)).unwrap();
    let snapshot_count = process1.event_log.save_snapshot(db1, process1).unwrap();

    tracing::info!("Saved snapshot at event count: {}", snapshot_count);

    // Verify all snapshots can be replayed correctly
    harness
        .verify_all_snapshots()
        .expect("Snapshot verification should succeed");

    // Additional verification: Get all snapshots and replay from early to latest
    let tx = db1.begin_read().unwrap();
    let snapshots_table =
        snapshots_table_default::<TestTransaction>().expect("Should have snapshots table");
    let snapshots = tx.open_table(snapshots_table).expect("Should open table");

    let mut snapshot_counts: Vec<u64> = Vec::new();
    for item in snapshots.iter().expect("Should iterate") {
        let (count, _) = item.expect("Should read item");
        snapshot_counts.push(count.value());
    }
    snapshot_counts.sort();
    drop(tx);

    if snapshot_counts.len() >= 2 {
        // Verify that we can load early and late snapshots
        let early_count = snapshot_counts[0];
        let latest_count = snapshot_counts[snapshot_counts.len() - 1];

        // Create storage factories for loading snapshots
        let db_arc = db1.clone();
        let bulk_store = RedbBulkStore::new(db_arc.clone()).unwrap();
        let snapshot_store = RedbSnapshotStore::new(db_arc.clone()).unwrap();

        // Load snapshot before early_count
        let (_, early_snapshot) = process1
            .event_log
            .load_snapshot_before(
                db1,
                early_count + 1,
                bulk_store.clone(),
                snapshot_store.clone(),
                None,
            )
            .expect("Failed to load early snapshot")
            .expect("Early snapshot should exist");
        assert!(early_snapshot.event_log.recorded_entries <= early_count);

        // Load snapshot before latest_count
        let (_, latest_snapshot) = process1
            .event_log
            .load_snapshot_before(db1, latest_count + 1, bulk_store, snapshot_store, None)
            .expect("Failed to load latest snapshot")
            .expect("Latest snapshot should exist");
        assert!(latest_snapshot.event_log.recorded_entries <= latest_count);
    }
}
