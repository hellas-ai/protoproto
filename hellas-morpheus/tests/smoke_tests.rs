use ark_std::test_rng;
use hellas_morpheus::test_harness::{MockHarness, TxGenPolicy};
use hellas_morpheus::*;
use hints::{F, GlobalData};
use std::collections::BTreeMap;
use std::sync::Arc;

#[test_log::test]
fn test_mock_harness_enqueue_message() {
    let mut harness = MockHarness::create_test_setup(2);

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

    let mut harness = MockHarness::create_test_setup(3);

    // A freshly created process should have no invariant violations
    for process in harness.processes.values() {
        let violations = process.check_invariants();
        assert!(
            violations.is_empty(),
            "New process has invariant violations: {:?}",
            violations
        );
    }

    harness
        .tx_gen_policy
        .insert(Identity(2), TxGenPolicy::EveryNSteps { n: 3 });

    harness
        .tx_gen_policy
        .insert(Identity(3), TxGenPolicy::EveryNSteps { n: 2 });

    // Let the system run for a while.
    harness.run(2 * 3 * 5);

    for block in harness
        .processes
        .get(&Identity(2))
        .unwrap()
        .index
        .blocks
        .values()
    {
        println!("block: {:?}", block);
    }
    println!(
        "p1 blocks: {}",
        harness
            .processes
            .get(&Identity(2))
            .unwrap()
            .index
            .blocks
            .values()
            .filter(|b| b.data.key.author == Some(Identity(1)))
            .count()
    );
    println!(
        "p2 blocks: {}",
        harness
            .processes
            .get(&Identity(2))
            .unwrap()
            .index
            .blocks
            .values()
            .filter(|b| b.data.key.author == Some(Identity(2)))
            .count()
    );
    println!(
        "p3 blocks: {}",
        harness
            .processes
            .get(&Identity(2))
            .unwrap()
            .index
            .blocks
            .values()
            .filter(|b| b.data.key.author == Some(Identity(3)))
            .count()
    );
    println!(
        "lead blocks: {}",
        harness
            .processes
            .get(&Identity(2))
            .unwrap()
            .index
            .blocks
            .values()
            .filter(|b| b.data.key.type_ == BlockType::Lead)
            .count()
    );
    println!(
        "tr blocks: {}",
        harness
            .processes
            .get(&Identity(2))
            .unwrap()
            .index
            .blocks
            .values()
            .filter(|b| b.data.key.type_ == BlockType::Tr)
            .count()
    );
    assert_eq!(
        harness
            .processes
            .get(&Identity(2))
            .unwrap()
            .index
            .blocks
            .len(),
        26
    );
}

#[test_log::test]
fn test_basic_integration() {
    let mut harness = MockHarness::create_test_setup(3);

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
        assert_eq!(process.current_time, 1000);
    }
}

#[test_log::test]
fn test_directed_message_flow() {
    let mut harness = MockHarness::create_test_setup(3);

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
        assert_eq!(process.current_time, 100);
    }
}

#[test_log::test]
fn test_process_round_no_messages() {
    let mut harness = MockHarness::create_test_setup(1);

    // Initial state - no pending messages
    assert_eq!(harness.pending_messages.len(), 0);

    // Process a round should not make progress without messages
    let made_progress = harness.process_round();
    assert_eq!(made_progress, false);
}

#[test_log::test]
fn test_check_all_timeouts() {
    let mut harness = MockHarness::create_test_setup(1);

    // Check timeouts
    let made_progress = harness.check_all_timeouts();

    // There should be nothing to do
    assert_eq!(made_progress, false);
}

#[test_log::test]
fn test_basic_process_interaction() {
    let mut harness = MockHarness::create_test_setup(2);

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
}

#[test_log::test]
fn test_broadcast_message() {
    let mut harness = MockHarness::create_test_setup(3);

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
}

#[test_log::test]
fn test_pending_votes_invariants() {
    let mut harness = MockHarness::create_test_setup(1);
    let process = harness.processes.get_mut(&Identity(1)).unwrap();

    // Verify no invariant violations in initial state
    let violations = process.check_invariants();
    assert!(
        violations.is_empty(),
        "New process has invariant violations: {:?}",
        violations
    );

    // Manually create a pending votes entry for the current view
    let current_view = process.view_i;
    let pending = process.pending_votes.entry(current_view).or_default();

    // Create a block key for a non-existent block to trigger an invariant violation
    let non_existent_block = BlockKey {
        type_: BlockType::Tr,
        view: current_view,
        height: 100,
        author: Some(Identity(1)),
        slot: SlotNum(5),
        hash: Some(BlockHash(0x12345678)),
    };

    // Add to pending votes
    pending.tr_1.insert(non_existent_block.clone(), true);
    pending.dirty = true;

    // Check for invariant violation - should be PendingVotesBlockNotFound
    let violations = process.check_invariants();
    let has_block_not_found = violations.iter().any(|v| {
        if let InvariantViolation::PendingVotesBlockNotFound {
            view,
            block_key,
            vote_type,
        } = v
        {
            view == &current_view && block_key == &non_existent_block && vote_type == "tr_1"
        } else {
            false
        }
    });

    assert!(
        has_block_not_found,
        "Expected PendingVotesBlockNotFound invariant violation not found in: {:?}",
        violations
    );

    // Clean up and test with a finalized block
    process.pending_votes.clear();

    // Create a simple block and mark it as finalized
    let block_key = BlockKey {
        type_: BlockType::Tr,
        view: current_view,
        height: 1,
        author: Some(Identity(1)),
        slot: SlotNum(1),
        hash: Some(BlockHash(0xABCDEF)),
    };

    // Generate a QC for the genesis block
    let gen_vote_data = VoteData {
        z: 1,
        for_which: GEN_BLOCK_KEY.clone(),
    };

    // Create a dummy threshold signature
    let gen_qc = Arc::new(ThreshSigned {
        data: gen_vote_data,
        signature: hints::Signature::default(),
    });

    // Add this block to the process's state using proper constructors
    let block = Signed::from_data(
        Block {
            key: block_key.clone(),
            prev: vec![],
            one: gen_qc,
            data: BlockData::Tr {
                transactions: vec![],
            },
        },
        &process.kb,
    );

    // Add the block to the process
    process.record_block(&Arc::new(block));

    // Mark the block as finalized
    process.index.finalized.insert(block_key.clone(), true);

    // Add to pending votes
    let pending = process.pending_votes.entry(current_view).or_default();
    pending.tr_1.insert(block_key.clone(), true);
    pending.dirty = true;

    // Check for invariant violation - should be PendingVotesForFinalizedBlock
    let violations = process.check_invariants();
    let has_finalized_violation = violations.iter().any(|v| {
        if let InvariantViolation::PendingVotesForFinalizedBlock {
            view,
            block_key: vio_key,
            vote_type,
        } = v
        {
            view == &current_view && vio_key == &block_key && vote_type == "tr_1"
        } else {
            false
        }
    });

    assert!(
        has_finalized_violation,
        "Expected PendingVotesForFinalizedBlock invariant violation not found in: {:?}",
        violations
    );
}
