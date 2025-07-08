//! Tests for the test harness functionality and basic integration tests
//!
//! These tests verify:
//! - Test harness functionality (event sourcing, replay, deterministic execution)
//! - Basic message handling and queuing
//! - Process interaction
//! - Time advancement
//! - Transaction generation policies
//! - Snapshot verification

mod common;

use ark_serialize::CanonicalSerialize;
use common::*;
use hellas_morpheus::test_harness::{MockHarness, TestTransaction, TxGenPolicy};
use hellas_morpheus::{
    BlockKey, BlockType, Message, SlotNum, ThreshPartial, ThreshSigned, VoteData,
};
use hellas_morpheus::{Identity, MorpheusProcess, ViewNum};
use redb::ReadableTable;
use std::sync::Arc;

/// Helper function to create a test harness with default storage
fn create_test_harness(num_parties: usize) -> MockHarness {
    MockHarness::create_test_setup(num_parties)
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
            .storage
            .journal
            .event_count()
            > 0
    );
    assert!(
        harness
            .processes
            .get(&Identity(2))
            .unwrap()
            .storage
            .journal
            .event_count()
            > 0
    );
    assert!(
        harness
            .processes
            .get(&Identity(3))
            .unwrap()
            .storage
            .journal
            .event_count()
            > 0
    );
}

#[test_log::test]
fn test_time_advancement_affects_processes() {
    let mut harness = create_test_harness(3);

    // Initial time should be 0 for harness and all processes
    assert_eq!(harness.time, 0);
    for (_, process) in harness.processes.iter() {
        assert_eq!(process.state.current_time, 0);
    }

    // Advance time
    harness.advance_time();

    // Harness time should be updated
    assert_eq!(harness.time, 100);

    // Update time for all processes
    harness.update_time();

    // All processes should have their time updated
    for (_, process) in harness.processes.iter() {
        assert_eq!(process.state.current_time, 100);
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

    // Time should have advanced (run may stop early if no progress)
    assert!(harness.time > 0, "Time should have advanced");
    
    // Update processes with the final time
    harness.update_time();
    
    // Processes should now have the correct time
    for (_, process) in harness.processes.iter() {
        assert_eq!(process.state.current_time, harness.time);
    }
}

#[test_log::test]
fn test_message_enqueue_and_processing() {
    let mut harness = create_test_harness(3);

    // Test basic enqueue functionality first
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

    // Now test processing with a vote message
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

    // Check initial queue length (should have 3 messages now)
    assert_eq!(harness.pending_messages.len(), 3);

    // Process rounds until queue is empty
    while !harness.pending_messages.is_empty() {
        harness.process_round();
    }

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

//#[test_log::test]
// fn test_snapshot_replay_determinism() {
//     let mut harness = create_test_harness(3);

//     // Configure transaction generation policy
//     harness
//         .tx_gen_policy
//         .insert(Identity(1), TxGenPolicy::EveryNSteps { n: 2 });

//     // Run for some steps to generate various event types
//     harness.run(2);
//     for (_, process) in harness.processes.iter() {
//         let db = harness.dbs.get(&process.id).unwrap();
//     }
//     harness.run(3);

//     // Take a snapshot for process 1
//     let process1 = harness.processes.get(&Identity(1)).unwrap();
//     let db1 = harness.dbs.get(&Identity(1)).unwrap();

//     // Verify all snapshots can be replayed correctly
//     harness
//         .verify_all_snapshots()
//         .expect("Snapshot verification should succeed");

//     // Additional verification: Get all snapshots and replay from early to latest
//     let tx = db1.begin_read().unwrap();
//     let snapshots = tx.open_table(self.tab).expect("Should open table");

//     let mut snapshot_counts: Vec<u64> = Vec::new();
//     for item in snapshots.iter().expect("Should iterate") {
//         let (count, _) = item.expect("Should read item");
//         snapshot_counts.push(count.value());
//     }
//     snapshot_counts.sort();
//     drop(tx);

//     if snapshot_counts.len() >= 2 {
//         // Verify that we can load early and late snapshots
//         let early_count = snapshot_counts[0];
//         let latest_count = snapshot_counts[snapshot_counts.len() - 1];

//         // Create storage factories for loading snapshots
//         let db_arc = db1.clone();

//         // Load snapshot before early_count
//         let (_, early_snapshot) = process1
//             .storage
//             .journal
//             .load_snapshot_before(db1, early_count + 1, None)
//             .expect("Failed to load early snapshot")
//             .expect("Early snapshot should exist");
//         assert!(early_snapshot.journal.event_count() <= early_count);

//         // Load snapshot before latest_count
//         let (_, latest_snapshot) = process1
//             .storage
//             .journal
//             .load_snapshot_before(db1, latest_count + 1, None)
//             .expect("Failed to load latest snapshot")
//             .expect("Latest snapshot should exist");
//         assert!(latest_snapshot.event_log.recorded_entries <= latest_count);
//     }
// }

// Tests merged from smoke_tests.rs

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
    let block_count = process.storage.journal.event_count();

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

    // Time should have advanced (run may stop early if no progress)
    assert!(harness.time > 0, "Time should have advanced");

    // Update processes with the final time
    harness.update_time();

    // Each process should have its time updated correctly
    for (_, process) in harness.processes.iter() {
        assert_eq!(process.state.current_time, harness.time);
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

    // Update processes with the new time
    harness.update_time();

    // All processes should have their time updated
    for (_, process) in harness.processes.iter() {
        assert_eq!(process.state.current_time, 100);
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
