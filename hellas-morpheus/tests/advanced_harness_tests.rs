use hellas_morpheus::mock_harness::MockHarness;
use hellas_morpheus::{
    Block, BlockData, BlockHash, BlockKey, BlockType, Identity, Message, MorpheusProcess,
    Signature, Signed, SlotNum, ThreshSignature, ThreshSigned, Transaction, ViewNum, VoteData,
};
use std::sync::Arc;

// Helper function to create a new test setup with 3 processes
fn create_test_setup() -> MockHarness {
    // Create processes with different identities
    let process1 = MorpheusProcess::new(Identity(1), 3, 1);
    let process2 = MorpheusProcess::new(Identity(2), 3, 1);
    let process3 = MorpheusProcess::new(Identity(3), 3, 1);

    // Create a harness with these processes
    MockHarness::new(vec![process1, process2, process3], 100)
}

#[test_log::test]
fn test_multiple_message_processing() {
    let mut harness = create_test_setup();

    // Create a few simple messages
    let message1 = Message::EndView(Arc::new(Signed {
        data: ViewNum(0),
        author: Identity(1),
        signature: Signature {},
    }));

    let message2 = Message::EndView(Arc::new(Signed {
        data: ViewNum(1),
        author: Identity(2),
        signature: Signature {},
    }));

    // Enqueue the messages for specific destinations
    harness.enqueue_message(message1, Identity(1), Some(Identity(2)));
    harness.enqueue_message(message2, Identity(2), Some(Identity(3)));

    // Initial queue length
    assert_eq!(harness.pending_messages.len(), 2);

    // Process the round
    harness.process_round();

    // Queue should be empty after processing
    assert_eq!(harness.pending_messages.len(), 0);
    assert_eq!(
        harness
            .processes
            .get(&Identity(1))
            .unwrap()
            .received_messages
            .len(),
        2
    );
    assert_eq!(
        harness
            .processes
            .get(&Identity(2))
            .unwrap()
            .received_messages
            .len(),
        3
    );
    assert_eq!(
        harness
            .processes
            .get(&Identity(3))
            .unwrap()
            .received_messages
            .len(),
        3
    );
}

#[test_log::test]
fn test_time_advancement_affects_processes() {
    let mut harness = create_test_setup();

    // Initial time should be 0 for harness and all processes
    assert_eq!(harness.time, 0);
    for (_, process) in harness.processes.iter() {
        assert_eq!(process.current_time, 0);
    }

    // Advance time
    harness.advance_time();

    // Harness time should be updated
    assert_eq!(harness.time, 100);

    // All processes should have their time updated
    for (_, process) in harness.processes.iter() {
        assert_eq!(process.current_time, 100);
    }
}

#[test_log::test]
fn test_complex_simulation() {
    let mut harness = create_test_setup();

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

    // Create a QC message
    let qc_message = Message::QC(Arc::new(ThreshSigned {
        data: vote_data,
        signature: ThreshSignature {},
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
    let mut harness = create_test_setup();

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
    let signed_vote = Signed {
        data: vote_data.clone(),
        author: Identity(1),
        signature: Signature {},
    };

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

#[test_log::test]
fn test_step_sequence() {
    let mut harness = create_test_setup();

    // Initial state
    assert_eq!(harness.time, 0);

    // Run one step
    harness.step();

    // After one step:
    // 1. Messages should be processed
    // 2. Timeouts should be checked
    // 3. Time should be advanced
    assert_eq!(harness.time, 100);

    // Add a message after the first step
    let message = Message::EndView(Arc::new(Signed {
        data: ViewNum(0),
        author: Identity(1),
        signature: Signature {},
    }));

    harness.enqueue_message(message, Identity(1), Some(Identity(2)));

    // Run another step
    harness.step();

    // After second step:
    // 1. The message should be processed
    // 2. Timeouts checked
    // 3. Time advanced again
    assert_eq!(harness.time, 200);

    // Note: We don't make assertions about the queue size as it depends
    // on the internal implementation of process_message and processing behavior
}
