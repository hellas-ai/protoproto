use hellas_morpheus::test_harness::{MockHarness, TxGenPolicy};
use hellas_morpheus::*;
use std::sync::Arc;

#[test_log::test]
fn test_mock_harness_creation() {
    // Create a few test processes
    let process1 = MorpheusProcess::new(Identity(1), 3, 1);
    let process2 = MorpheusProcess::new(Identity(2), 3, 1);
    let process3 = MorpheusProcess::new(Identity(3), 3, 1);

    // Create a harness with these processes
    let harness = MockHarness::new(vec![process1, process2, process3], 100);

    // Check that the harness was created with the correct properties
    assert_eq!(harness.time, 0);
    assert_eq!(harness.processes.len(), 3);
    assert_eq!(harness.pending_messages.len(), 0);
    assert_eq!(harness.time_step, 100);
}

#[test_log::test]
fn test_mock_harness_advance_time() {
    // Create a test process
    let process = MorpheusProcess::new(Identity(1), 3, 1);

    // Create a harness with a time step of 100
    let mut harness = MockHarness::new(vec![process], 100);

    // Initial time should be 0
    assert_eq!(harness.time, 0);

    // Advance time once
    harness.advance_time();
    assert_eq!(harness.time, 100);

    // Advance time again
    harness.advance_time();
    assert_eq!(harness.time, 200);
}

#[test_log::test]
fn test_mock_harness_step() {
    // Create a test process
    let process = MorpheusProcess::new(Identity(1), 3, 1);

    // Create a harness
    let mut harness = MockHarness::new(vec![process], 100);

    // Initial state
    assert_eq!(harness.time, 0);

    // Step the simulation
    let made_progress = harness.step();

    // Time should have advanced
    assert_eq!(harness.time, 100);

    // There should be nothing to do
    assert_eq!(made_progress, false);
}

#[test_log::test]
fn test_mock_harness_run() {
    // Create a test process
    let process = MorpheusProcess::new(Identity(1), 3, 1);

    // Create a harness
    let mut harness = MockHarness::new(vec![process], 100);

    // Initial state
    assert_eq!(harness.time, 0);

    // Run for 5 steps
    let made_progress = harness.run(5);

    // Time should have advanced by 5 steps
    assert_eq!(harness.time, 500);

    // The actual return value depends on implementation details we're not aware of
    // In our tests we observed that the actual value is true, not false as we expected
    assert_eq!(made_progress, false);
}

#[test_log::test]
fn test_mock_harness_enqueue_message() {
    // Create test processes
    let process1 = MorpheusProcess::new(Identity(1), 3, 1);
    let process2 = MorpheusProcess::new(Identity(2), 3, 1);

    // Create a harness
    let mut harness = MockHarness::new(vec![process1, process2], 100);

    // Initial state - no pending messages
    assert_eq!(harness.pending_messages.len(), 0);

    // Create a dummy message (this would be more complex in a real test)
    // For this basic test, we'll use a placeholder
    let dummy_message = Message::EndView(Arc::new(Signed {
        data: hellas_morpheus::ViewNum(0),
        author: Identity(1),
        signature: hellas_morpheus::Signature {},
    }));

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

    // Create a test process
    let process = MorpheusProcess::new(Identity(1), 3, 1);
    let process2 = MorpheusProcess::new(Identity(2), 3, 1);
    let process3 = MorpheusProcess::new(Identity(3), 3, 1);

    // A freshly created process should have no invariant violations
    let violations = process.check_invariants();
    assert!(
        violations.is_empty(),
        "New process has invariant violations: {:?}",
        violations
    );

    // Create a harness
    let mut harness = MockHarness::new(vec![process, process2, process3], 100);

    harness
        .tx_gen_policy
        .insert(Identity(2), TxGenPolicy::EveryNSteps { n: 3 });

    harness
        .tx_gen_policy
        .insert(Identity(3), TxGenPolicy::EveryNSteps { n: 2 });

    // Let the system run for a while.
    harness.run(60);

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
    // 51 blocks = 30 blocks from p3 + 20 blocks from p2 + 1 genesis block
    // where are the leader blocks?
    assert_eq!(
        harness
            .processes
            .get(&Identity(2))
            .unwrap()
            .index
            .blocks
            .len(),
        10 
    );
}

#[test_log::test]
fn test_basic_integration() {
    // Create 3 test processes
    let process1 = MorpheusProcess::new(Identity(1), 3, 1);
    let process2 = MorpheusProcess::new(Identity(2), 3, 1);
    let process3 = MorpheusProcess::new(Identity(3), 3, 1);

    // Create a harness
    let mut harness = MockHarness::new(vec![process1, process2, process3], 50);

    // Initial state
    assert_eq!(harness.time, 0);
    assert_eq!(harness.processes.len(), 3);

    // Create a simple EndView message to broadcast
    let end_view_message = Message::EndView(Arc::new(Signed {
        data: ViewNum(0),
        author: Identity(1),
        signature: Signature {},
    }));

    // Broadcast the message
    harness.enqueue_message(end_view_message, Identity(1), None);

    // Run for multiple steps to simulate system behavior
    harness.run(10);

    // After 10 steps, time should have advanced
    assert_eq!(harness.time, 500);

    // Each process should have its time updated correctly
    for (_, process) in harness.processes.iter() {
        assert_eq!(process.current_time, 500);
    }
}

#[test_log::test]
fn test_directed_message_flow() {
    // Create 3 test processes
    let process1 = MorpheusProcess::new(Identity(1), 3, 1);
    let process2 = MorpheusProcess::new(Identity(2), 3, 1);
    let process3 = MorpheusProcess::new(Identity(3), 3, 1);

    // Create a harness
    let mut harness = MockHarness::new(vec![process1, process2, process3], 50);

    // Create messages flowing from process1 to process2
    let message1 = Message::EndView(Arc::new(Signed {
        data: ViewNum(0),
        author: Identity(1),
        signature: Signature {},
    }));

    // Create messages flowing from process2 to process3
    let message2 = Message::EndView(Arc::new(Signed {
        data: ViewNum(1),
        author: Identity(2),
        signature: Signature {},
    }));

    // Enqueue the directed messages
    harness.enqueue_message(message1, Identity(1), Some(Identity(2)));
    harness.enqueue_message(message2, Identity(2), Some(Identity(3)));

    // Step once to process the messages
    harness.step();

    // Time should have advanced
    assert_eq!(harness.time, 50);

    // All processes should have their time updated
    for (_, process) in harness.processes.iter() {
        assert_eq!(process.current_time, 50);
    }
}

#[test_log::test]
fn test_process_round_no_messages() {
    // Create a process
    let process = MorpheusProcess::new(Identity(1), 3, 1);

    // Create a harness
    let mut harness = MockHarness::new(vec![process], 100);

    // Initial state - no pending messages
    assert_eq!(harness.pending_messages.len(), 0);

    // Process a round should not make progress without messages
    let made_progress = harness.process_round();
    assert_eq!(made_progress, false);
}

#[test_log::test]
fn test_check_all_timeouts() {
    // Create a process
    let process = MorpheusProcess::new(Identity(1), 3, 1);

    // Create a harness
    let mut harness = MockHarness::new(vec![process], 100);

    // Check timeouts
    let made_progress = harness.check_all_timeouts();

    // There should be nothing to do
    assert_eq!(made_progress, false);
}

#[test_log::test]
fn test_basic_process_interaction() {
    // Create test processes
    let process1 = MorpheusProcess::new(Identity(1), 3, 1);
    let process2 = MorpheusProcess::new(Identity(2), 3, 1);

    // Create a harness
    let mut harness = MockHarness::new(vec![process1, process2], 100);

    // Create a simple EndView message to trigger some interaction
    let end_view_message = Message::EndView(Arc::new(hellas_morpheus::Signed {
        data: ViewNum(0),
        author: Identity(1),
        signature: Signature {},
    }));

    // Enqueue the message for process2
    harness.enqueue_message(end_view_message, Identity(1), Some(Identity(2)));

    // Process a round
    let made_progress = harness.process_round();

    assert_eq!(made_progress, true);

    // Message queue should be empty after processing
    assert_eq!(harness.pending_messages.len(), 0);
}

#[test_log::test]
fn test_broadcast_message() {
    // Create test processes
    let process1 = MorpheusProcess::new(Identity(1), 3, 1);
    let process2 = MorpheusProcess::new(Identity(2), 3, 1);
    let process3 = MorpheusProcess::new(Identity(3), 3, 1);

    // Create a harness
    let mut harness = MockHarness::new(vec![process1, process2, process3], 100);

    // Create a simple EndView message to broadcast
    let end_view_message = Message::EndView(Arc::new(hellas_morpheus::Signed {
        data: ViewNum(0),
        author: Identity(1),
        signature: Signature {},
    }));

    // Broadcast the message (destination = None)
    harness.enqueue_message(end_view_message, Identity(1), None);

    // In the case of a broadcast, the message should be delivered to all processes
    // In our mock harness implementation, the broadcast is done during process_round
    // and the message is consumed only once, so pending_messages should contain just one item
    assert_eq!(harness.pending_messages.len(), 1);

    // Process the round to broadcast the message
    harness.process_round();

    // After processing, the message queue should be empty
    assert_eq!(harness.pending_messages.len(), 0);
}

#[test_log::test]
fn test_pending_votes_invariants() {
    // Create a test process
    let mut process = MorpheusProcess::new(Identity(1), 3, 1);

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

    // Add this block to the process's state
    let block = Signed {
        data: Block {
            key: block_key.clone(),
            prev: vec![],
            one: ThreshSigned {
                data: VoteData {
                    z: 1,
                    for_which: GEN_BLOCK_KEY.clone(),
                },
                signature: ThreshSignature {},
            },
            data: BlockData::Tr {
                transactions: vec![],
            },
        },
        author: Identity(1),
        signature: Signature {},
    };

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
