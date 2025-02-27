use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::{Duration, Instant};
use log::{debug, info, warn};

use super::types::*;
use super::MorpheusConfig;
use super::Morpheus;

/// Network simulator for testing
pub struct NetworkSimulator {
    /// Nodes in the network
    pub nodes: Vec<Morpheus>,
    
    /// Matrix of delays between nodes
    pub delays: Vec<Vec<Duration>>,
    
    /// Message queue: (from, to, message, delivery_time)
    pub message_queue: VecDeque<(usize, usize, Message, Instant)>,
    
    /// Whether to simulate random message loss
    pub message_loss_rate: f64,
    
    /// Random number generator seed
    pub rng_seed: u64,
}

impl NetworkSimulator {
    /// Create a new network simulator
    pub fn new(
        num_nodes: usize, 
        base_delay: Duration, 
        jitter: Duration,
        message_loss_rate: f64,
    ) -> Self {
        let mut nodes = Vec::with_capacity(num_nodes);
        let mut delays = Vec::with_capacity(num_nodes);
        
        // Calculate maximum tolerable Byzantine faults
        let f = (num_nodes - 1) / 3;
        
        // Create nodes
        for i in 0..num_nodes {
            let config = MorpheusConfig {
                process_id: ProcessId(i),
                num_processes: num_nodes,
                f,
                delta: base_delay * 2, // Conservative estimate
            };
            
            nodes.push(Morpheus::new(config));
            
            // Create delay matrix with randomized delays
            let mut node_delays = Vec::with_capacity(num_nodes);
            for j in 0..num_nodes {
                if i == j {
                    node_delays.push(Duration::from_millis(0));
                } else {
                    // Simple randomization for testing
                    let random_jitter = (i * j % 100) as f64 / 100.0 * jitter.as_secs_f64();
                    let delay = base_delay + Duration::from_secs_f64(random_jitter);
                    node_delays.push(delay);
                }
            }
            delays.push(node_delays);
        }
        
        Self {
            nodes,
            delays,
            message_queue: VecDeque::new(),
            message_loss_rate,
            rng_seed: 42,
        }
    }
    
    /// Deliver all messages whose delivery time has arrived
    pub fn deliver_messages(&mut self) {
        let now = Instant::now();
        
        // Deliver all messages whose delivery time has arrived
        while let Some((from, to, message, delivery_time)) = self.message_queue.front() {
            if *delivery_time <= now {
                // Pop the message
                let (from, to, message, _) = self.message_queue.pop_front().unwrap();
                
                // Deliver the message
                self.deliver_message(from, to, message);
            } else {
                // No more messages to deliver now
                break;
            }
        }
    }
    
    /// Deliver a message to a node
    fn deliver_message(&mut self, from: usize, to: usize, message: Message) {
        // Simple random message loss simulation
        let random_val = (from * to * self.rng_seed as usize) % 100;
        if (random_val as f64 / 100.0) < self.message_loss_rate {
            // Message lost
            return;
        }
        
        // Process the message based on its type
        match message {
            Message::Block(block) => {
                // In a real implementation, this would be handled by the network model
                self.nodes[to].get_state_mut().block_state.add_block(block).ok();
            },
            Message::Vote(vote) => {
                let quorum_size = self.nodes[to].get_state().num_processes 
                                - self.nodes[to].get_state().f;
                self.nodes[to].get_state_mut().vote_state.add_vote(vote, quorum_size);
            },
            Message::QC(qc) => {
                self.nodes[to].get_state_mut().vote_state.add_qc(qc);
            },
            Message::ViewMessage(msg) => {
                self.nodes[to].get_state_mut().view_state.add_view_message(msg);
            },
            Message::EndViewMessage(msg) => {
                self.nodes[to].get_state_mut().view_state.add_end_view_message(msg);
            },
            Message::ViewCertificate(cert) => {
                self.nodes[to].get_state_mut().view_state.add_view_certificate(cert);
            },
        }
    }
    
    /// Send a message from one node to another
    pub fn send_message(&mut self, from: usize, to: usize, message: Message) {
        let delay = self.delays[from][to];
        let delivery_time = Instant::now() + delay;
        
        self.message_queue.push_back((from, to, message, delivery_time));
    }
    
    /// Broadcast a message from one node to all others
    pub fn broadcast_message(&mut self, from: usize, message: Message) {
        for to in 0..self.nodes.len() {
            if to != from {
                self.send_message(from, to, message.clone());
            }
        }
    }
    
    /// Run for a specified duration
    pub fn run_for(&mut self, duration: Duration) {
        let end_time = Instant::now() + duration;
        
        while Instant::now() < end_time {
            // Deliver any pending messages
            self.deliver_messages();
            
            // Run a step on each node
            for node in &mut self.nodes {
                node.step();
            }
            
            // Sleep a bit to avoid busy-waiting
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    
    /// Simulate network partition
    pub fn create_partition(&mut self, group1: Vec<usize>, group2: Vec<usize>) {
        // Set delays between groups to be very high
        let partition_delay = Duration::from_secs(1000); // Effectively disconnected
        
        for &i in &group1 {
            for &j in &group2 {
                self.delays[i][j] = partition_delay;
                self.delays[j][i] = partition_delay;
            }
        }
    }
    
    /// Heal network partition
    pub fn heal_partition(&mut self, base_delay: Duration, jitter: Duration) {
        // Reset delays to normal
        for i in 0..self.nodes.len() {
            for j in 0..self.nodes.len() {
                if i != j {
                    let random_jitter = (i * j % 100) as f64 / 100.0 * jitter.as_secs_f64();
                    let delay = base_delay + Duration::from_secs_f64(random_jitter);
                    self.delays[i][j] = delay;
                }
            }
        }
    }
    
    /// Make a node Byzantine (simulating specific faults)
    pub fn make_byzantine(&mut self, node_idx: usize, fault_type: ByzantineFaultType) {
        // In a real implementation, we would modify the node's behavior
        // For now, we'll just simulate some effects
        
        match fault_type {
            ByzantineFaultType::Crash => {
                // Remove the node from message delivery
                for i in 0..self.nodes.len() {
                    self.delays[node_idx][i] = Duration::from_secs(1000);
                    self.delays[i][node_idx] = Duration::from_secs(1000);
                }
            },
            ByzantineFaultType::Equivocation => {
                // Simulated by the caller sending conflicting blocks
            },
            ByzantineFaultType::DelayMessages => {
                // Increase delays for messages from this node
                for i in 0..self.nodes.len() {
                    if i != node_idx {
                        self.delays[node_idx][i] *= 5;
                    }
                }
            },
        }
    }
    
    /// Check if consensus has been reached
    pub fn has_consensus(&self) -> bool {
        if self.nodes.is_empty() {
            return false;
        }
        
        // Get the log from the first node
        let first_log = self.nodes[0].get_log();
        
        // Check if all nodes have the same log
        self.nodes.iter().all(|node| {
            let log = node.get_log();
            log.len() == first_log.len() && 
                log.iter().zip(first_log.iter()).all(|(a, b)| a.data == b.data)
        })
    }
    
    /// Get the current transaction throughput
    pub fn get_transaction_throughput(&self) -> usize {
        // Sum the transaction logs of all nodes
        self.nodes.iter().map(|node| node.get_log().len()).sum()
    }
}

/// Types of Byzantine faults to simulate
pub enum ByzantineFaultType {
    /// Node crashes and stops responding
    Crash,
    /// Node sends conflicting messages
    Equivocation,
    /// Node delays messages
    DelayMessages,
}

/// Test helper to create and run Morpheus networks
pub struct TestHelper;

impl TestHelper {
    /// Create a standard test network
    pub fn create_test_network(
        num_nodes: usize,
        base_delay_ms: u64,
        byzantine_nodes: Vec<(usize, ByzantineFaultType)>,
    ) -> NetworkSimulator {
        let base_delay = Duration::from_millis(base_delay_ms);
        let jitter = Duration::from_millis(base_delay_ms / 5);
        
        let mut simulator = NetworkSimulator::new(
            num_nodes,
            base_delay,
            jitter,
            0.01, // 1% message loss
        );
        
        // Make specified nodes Byzantine
        for (node_idx, fault_type) in byzantine_nodes {
            simulator.make_byzantine(node_idx, fault_type);
        }
        
        simulator
    }
    
    /// Run a simple test with transactions
    pub fn run_simple_test(
        simulator: &mut NetworkSimulator,
        transactions_per_node: usize,
        runtime_seconds: u64,
    ) {
        // Add transactions to each node
        for (i, node) in simulator.nodes.iter_mut().enumerate() {
            for j in 0..transactions_per_node {
                let transaction = Transaction {
                    data: format!("Transaction from node {} #{}", i, j).into_bytes(),
                };
                node.add_transaction(transaction);
            }
        }
        
        // Run the simulation
        simulator.run_for(Duration::from_secs(runtime_seconds));
        
        // Print results
        println!("Simulation completed after {} seconds", runtime_seconds);
        println!("Consensus reached: {}", simulator.has_consensus());
        println!("Total transactions finalized: {}", simulator.get_transaction_throughput());
    }
    
    /// Run a network partition test
    pub fn run_partition_test(
        simulator: &mut NetworkSimulator,
        transactions_per_node: usize,
        partition_start_sec: u64,
        partition_duration_sec: u64,
        total_runtime_sec: u64,
    ) {
        // Add transactions to each node
        for (i, node) in simulator.nodes.iter_mut().enumerate() {
            for j in 0..transactions_per_node {
                let transaction = Transaction {
                    data: format!("Transaction from node {} #{}", i, j).into_bytes(),
                };
                node.add_transaction(transaction);
            }
        }
        
        // Run until partition
        simulator.run_for(Duration::from_secs(partition_start_sec));
        
        // Create partition
        let num_nodes = simulator.nodes.len();
        let group1: Vec<_> = (0..num_nodes/2).collect();
        let group2: Vec<_> = (num_nodes/2..num_nodes).collect();
        simulator.create_partition(group1, group2);
        
        // Run during partition
        simulator.run_for(Duration::from_secs(partition_duration_sec));
        
        // Heal partition
        simulator.heal_partition(
            Duration::from_millis(50),
            Duration::from_millis(10),
        );
        
        // Run after healing
        simulator.run_for(Duration::from_secs(
            total_runtime_sec - partition_start_sec - partition_duration_sec
        ));
        
        // Print results
        println!("Partition test completed");
        println!("Consensus reached: {}", simulator.has_consensus());
        println!("Total transactions finalized: {}", simulator.get_transaction_throughput());
    }
}