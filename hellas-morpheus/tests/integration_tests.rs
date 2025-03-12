use std::sync::Arc;

use hellas_morpheus::{
    Identity, Message, MorpheusProcess, ViewNum, Signed, Signature
};
use hellas_morpheus::mock_harness::MockHarness;

#[test]
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

#[test]
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

#[test]
fn test_harness_time_advancement() {
    // Create test processes
    let process1 = MorpheusProcess::new(Identity(1), 3, 1);
    let process2 = MorpheusProcess::new(Identity(2), 3, 1);
    
    // Create a harness with a time step of 25
    let mut harness = MockHarness::new(vec![process1, process2], 25);
    
    // Initial state
    assert_eq!(harness.time, 0);
    
    // Perform multiple steps of the simulation
    for i in 1..=10 {
        harness.step();
        
        // Time should advance by the time step each iteration
        assert_eq!(harness.time, i * 25);
        
        // All processes should have their time updated
        for (_, process) in harness.processes.iter() {
            assert_eq!(process.current_time, i * 25);
        }
    }
} 