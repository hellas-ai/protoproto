use hellas_morpheus::{
    BlockData, BlockHash, BlockKey, BlockType, Identity, Message, MorpheusProcess,
    Signature, SlotNum, ThreshSignature, Transaction, ViewNum, VoteData
};
use hellas_morpheus::mock_harness::MockHarness;
use std::sync::Arc;


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
    harness.enqueue_message(end_view_message,  Identity(1), None);
    
    // In the case of a broadcast, the message should be delivered to all processes
    // In our mock harness implementation, the broadcast is done during process_round
    // and the message is consumed only once, so pending_messages should contain just one item
    assert_eq!(harness.pending_messages.len(), 1);
    
    // Process the round to broadcast the message
    harness.process_round();
    
    // After processing, the message queue should be empty
    assert_eq!(harness.pending_messages.len(), 0);
} 