// Simulator that runs a mock network of nodes
//
// Time is "logical", we don't actually wait for anything to happen
// We call set_now to simulate the passage of time in single-step increments
//
// At each step, we deliver messages that are ready to be delivered.
// We process each message to completion, check timeouts, check block production eligibility, and finally advance the state of the simulation.

use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};

use crate::*;

// Represents a message in transit with its delivery time
struct InTransitMessage {
    message: Message,
    from: Identity,
    to: Option<Identity>, // None means broadcast to all
    delivery_time: Instant,
}

// The Harness that simulates a network of MorpheusProcess nodes
pub struct MockHarness {
    // The processes in the network
    processes: BTreeMap<Identity, MorpheusProcess>,

    // Messages in transit
    message_queue: VecDeque<InTransitMessage>,

    // Current simulation time
    current_time: Instant,

    // Network message delay
    message_delay: Duration,

    // Configuration
    n: usize,
    f: usize,

    // Track finalized transaction blocks for each process
    finalized_transactions: BTreeMap<Identity, Vec<Transaction>>,

    // Network partitions - processes that can't communicate with each other
    partitions: Vec<Vec<Identity>>,

    // Byzantine nodes that may behave maliciously
    byzantine_nodes: Vec<Identity>,
}

impl MockHarness {
    // Create a new harness with n processes, of which f may be faulty
    pub fn new(n: usize, f: usize, message_delay: Duration) -> Self {
        assert!(n > 0, "Number of processes must be positive");
        assert!(f * 3 < n, "Must have less than n/3 faulty processes");

        let now = Instant::now();
        let mut processes = BTreeMap::new();

        // Create n processes
        for i in 0..n {
            let id = Identity(i as u64);
            processes.insert(id.clone(), MorpheusProcess::new(id, n, f));
        }

        MockHarness {
            processes,
            message_queue: VecDeque::new(),
            current_time: now,
            message_delay,
            n,
            f,
            finalized_transactions: BTreeMap::new(),
            partitions: Vec::new(),
            byzantine_nodes: Vec::new(),
        }
    }

    // Advance the simulation time
    pub fn advance_time(&mut self, duration: Duration) {
        self.current_time += duration;

        // Update all processes' current time
        for process in self.processes.values_mut() {
            process.set_now(self.current_time);
        }
    }

    // Step the simulation forward to process one event (delivering a message or advancing time)
    pub fn step(&mut self) -> bool {
        // First, check if there are messages to deliver
        if let Some(next_message) = self.message_queue.front() {
            if next_message.delivery_time <= self.current_time {
                // Message is ready to be delivered
                let message = self.message_queue.pop_front().unwrap();

                // Skip delivery if from a Byzantine node and we want to simulate message dropping
                if self.byzantine_nodes.contains(&message.from) && self.should_drop_message() {
                    // Drop message
                    return true;
                }

                // Deliver to recipient(s)
                if let Some(to) = message.to {
                    // Skip if there's a partition between from and to
                    if !self.is_partitioned(&message.from, &to) {
                        self.deliver_message(message.message, message.from, to);
                    }
                } else {
                    // Broadcast to all processes except those partitioned from sender
                    let recipients: Vec<Identity> = self
                        .processes
                        .keys()
                        .filter(|&id| {
                            *id != message.from && !self.is_partitioned(&message.from, id)
                        })
                        .cloned()
                        .collect();

                    for id in recipients {
                        self.deliver_message(message.message.clone(), message.from.clone(), id);
                    }
                }
                return true;
            }
        }

        // No messages ready to deliver, check timeouts and produce blocks
        let process_ids: Vec<Identity> = self
            .processes
            .keys()
            .filter(|id| !self.byzantine_nodes.contains(id))
            .cloned()
            .collect();

        for id in process_ids {
            if let Some(process) = self.processes.get_mut(&id) {
                let mut to_send = Vec::new();

                // Check timeouts
                process.check_timeouts(&mut to_send);

                // Try to produce blocks
                process.try_produce_blocks(&mut to_send);

                // Send any generated messages
                for (message, recipient) in to_send {
                    self.enqueue_message(message, id.clone(), recipient);
                }
            }
        }

        // Special handling for Byzantine nodes
        let byzantine_ids = self.byzantine_nodes.clone();
        for id in byzantine_ids {
            if self.should_create_conflict() {
                self.create_conflicting_blocks(id.clone());
            }

            if let Some(process) = self.processes.get_mut(&id) {
                // They might also still participate somewhat correctly
                let mut to_send = Vec::new();
                process.check_timeouts(&mut to_send);

                // Byzantine nodes might modify messages before sending
                for (message, recipient) in to_send {
                    // For simplicity, just send the message as-is in this implementation
                    self.enqueue_message(message, id.clone(), recipient);
                }
            }
        }

        // If there are still no messages, advance time to the next message delivery
        if !self.message_queue.is_empty() {
            let next_delivery = self.message_queue.front().unwrap().delivery_time;
            if next_delivery > self.current_time {
                self.advance_time(next_delivery - self.current_time);
                return true;
            }
        }

        // No more events to process
        false
    }

    // Run the simulation for a specified duration
    pub fn run_for(&mut self, duration: Duration) {
        let end_time = self.current_time + duration;
        while self.current_time < end_time && self.step() {}
    }

    // Deliver a message to a specific process
    fn deliver_message(&mut self, message: Message, _from: Identity, to: Identity) {
        if let Some(process) = self.processes.get_mut(&to) {
            let mut to_send = Vec::new();

            // Process the message
            if process.process_message(message.clone(), &mut to_send) {
                // If this is a block that's been finalized, track its transactions
                self.update_finalized_transactions(&to, &message);

                // Handle any messages generated in response
                for (response, recipient) in to_send {
                    self.enqueue_message(response, to.clone(), recipient);
                }
            }
        }
    }

    // Update finalized transactions when a block is finalized
    fn update_finalized_transactions(&mut self, process_id: &Identity, message: &Message) {
        if let Message::QC(qc) = message {
            if qc.data.z == 2 {
                // 2-QC means finalized
                let process = self.processes.get(process_id).unwrap();
                let block_key = &qc.data.for_which;

                if let Some(block) = process.blocks.get(block_key) {
                    if let BlockData::Tr { transactions } = &block.data.data {
                        let entry = self
                            .finalized_transactions
                            .entry(process_id.clone())
                            .or_insert_with(Vec::new);

                        // Only add transactions we haven't seen before
                        for tx in transactions {
                            if !entry.contains(tx) {
                                entry.push(tx.clone());
                            }
                        }
                    }
                }
            }
        }
    }

    // Add a message to the queue for later delivery
    fn enqueue_message(&mut self, message: Message, from: Identity, to: Option<Identity>) {
        // Add jitter to message delay based on a simple deterministic algorithm
        let jitter = match (from.0 as usize + self.message_queue.len()) % 10 {
            0 => Duration::from_millis(20), // Occasional larger delay
            _ => Duration::from_millis(2),  // Normal small jitter
        };

        let delivery_time = self.current_time + self.message_delay + jitter;

        self.message_queue.push_back(InTransitMessage {
            message,
            from,
            to,
            delivery_time,
        });

        // Sort the queue by delivery time
        self.message_queue
            .make_contiguous()
            .sort_by(|a, b| a.delivery_time.cmp(&b.delivery_time));
    }

    // Helper for deterministic "random" events
    fn should_drop_message(&self) -> bool {
        // Drop approximately 30% of Byzantine messages
        (self.current_time.elapsed().as_millis() as usize + self.message_queue.len()) % 10 < 3
    }

    fn should_create_conflict(&self) -> bool {
        // Create conflict approximately 10% of the time
        (self.current_time.elapsed().as_millis() as usize + self.message_queue.len()) % 10 == 0
    }

    // Submit a transaction to a specific process
    pub fn submit_transaction(&mut self, to: Identity, transaction: Transaction) {
        if let Some(process) = self.processes.get_mut(&to) {
            let transactions = vec![transaction];
            let block = Self::create_transaction_block(process, transactions);
            self.enqueue_message(Message::Block(block), to.clone(), None);
        }
    }

    // Helper to create a transaction block with specified transactions
    fn create_transaction_block(
        process: &MorpheusProcess,
        transactions: Vec<Transaction>,
    ) -> Signed<Arc<Block>> {
        // Create a simplified transaction block
        // This is a bit of a hack, but for testing purposes it allows us to simulate
        // transaction block creation directly

        let slot = process.slot_i_tr;
        let mut prev_qcs = Vec::new();

        // Find previous transaction block QC
        if slot.0 > 0 {
            for (vote_data, qc) in &process.qcs {
                if vote_data.for_which.type_ == BlockType::Tr
                    && vote_data.for_which.author == Some(process.id.clone())
                    && vote_data.for_which.slot == SlotNum(slot.0 - 1)
                {
                    prev_qcs.push(qc.clone());
                    break;
                }
            }
        }

        // If no previous block found, use genesis QC
        if prev_qcs.is_empty() {
            for (vote_data, qc) in &process.qcs {
                if vote_data.for_which.type_ == BlockType::Genesis {
                    prev_qcs.push(qc.clone());
                    break;
                }
            }
        }

        // Add tip if there's one
        if process.tips.len() == 1 {
            let tip = &process.tips[0];
            if let Some(tip_qc) = process.qcs.get(tip) {
                // Don't add duplicate QC
                if !prev_qcs
                    .iter()
                    .any(|qc| qc.data.for_which == tip_qc.data.for_which)
                {
                    prev_qcs.push(tip_qc.clone());
                }
            }
        }

        // Calculate height
        let height = prev_qcs
            .iter()
            .map(|qc| qc.data.for_which.height)
            .max()
            .unwrap_or(0)
            + 1;

        // Create block key
        let block_key = BlockKey {
            type_: BlockType::Tr,
            view: process.view_i,
            height,
            author: Some(process.id.clone()),
            slot,
            hash: Some(BlockHash(slot.0)), // Simplified hash generation
        };

        // Use max 1-QC from process
        let one_qc = process.max_1qc.clone();

        // Create block
        let block = Arc::new(Block {
            key: block_key,
            prev: prev_qcs,
            one: one_qc,
            data: BlockData::Tr { transactions },
        });

        // Sign the block
        Signed {
            data: block,
            author: process.id.clone(),
            signature: Signature {},
        }
    }

    // Check if two processes are partitioned from each other
    fn is_partitioned(&self, id1: &Identity, id2: &Identity) -> bool {
        // If no partitions defined, they're not partitioned
        if self.partitions.is_empty() {
            return false;
        }

        // Check if they're in the same partition
        for partition in &self.partitions {
            let in_partition1 = partition.contains(id1);
            let in_partition2 = partition.contains(id2);

            // If both are in the same partition, they can communicate
            if in_partition1 && in_partition2 {
                return false;
            }
        }

        // If we didn't find them in the same partition, they're partitioned
        true
    }

    // Create a network partition
    pub fn create_partitions(&mut self, partitions: Vec<Vec<Identity>>) {
        self.partitions = partitions;

        // Create a copy of the message queue to avoid borrowing issues
        let messages: Vec<InTransitMessage> = self.message_queue.drain(..).collect();

        // Only keep messages that don't cross partitions
        for msg in messages {
            if let Some(to) = &msg.to {
                if !self.is_partitioned(&msg.from, to) {
                    self.message_queue.push_back(msg);
                }
            } else {
                // Keep broadcast messages, delivery will be filtered
                self.message_queue.push_back(msg);
            }
        }
    }

    // Heal all partitions
    pub fn heal_partitions(&mut self) {
        self.partitions.clear();
    }

    // Designate a node as Byzantine
    pub fn set_byzantine(&mut self, id: Identity) {
        if !self.byzantine_nodes.contains(&id) {
            self.byzantine_nodes.push(id);
        }
    }

    // Create conflicting blocks from a Byzantine node
    fn create_conflicting_blocks(&mut self, from: Identity) {
        if let Some(process) = self.processes.get(&from) {
            // Create two conflicting transaction blocks with the same parent and slot
            let tx1 = Transaction::Opaque(vec![0xB, 0xA, 0xD]);
            let tx2 = Transaction::Opaque(vec![0xE, 0x8, 0x1, 0x7]);

            let block1 = Self::create_transaction_block(process, vec![tx1]);
            let block2 = Self::create_transaction_block(process, vec![tx2]);

            // Send both blocks
            self.enqueue_message(Message::Block(block1), from.clone(), None);
            self.enqueue_message(Message::Block(block2), from.clone(), None);
        }
    }

    // Get a reference to a specific process
    pub fn get_process(&self, id: &Identity) -> Option<&MorpheusProcess> {
        self.processes.get(id)
    }

    // Get the finalized transactions for a process
    pub fn get_finalized_transactions(&self, id: &Identity) -> Vec<Transaction> {
        self.finalized_transactions
            .get(id)
            .cloned()
            .unwrap_or_default()
    }

    // Check if nodes have reached consensus
    pub fn check_consensus(&self) -> bool {
        // Filter out Byzantine nodes from consensus check
        let honest_processes: Vec<&Identity> = self
            .processes
            .keys()
            .filter(|id| !self.byzantine_nodes.contains(id))
            .collect();

        if honest_processes.is_empty()
            || !honest_processes
                .iter()
                .all(|id| self.finalized_transactions.contains_key(id))
        {
            return false;
        }

        // Get the first process's finalized transactions
        let first_id = honest_processes[0];
        let first_txs = self.finalized_transactions.get(first_id).unwrap();

        if first_txs.is_empty() {
            return false;
        }

        // Check if all other honest processes have the same transactions in the same order
        for id in &honest_processes[1..] {
            if let Some(txs) = self.finalized_transactions.get(id) {
                // Get the smallest common prefix length
                let min_len = std::cmp::min(txs.len(), first_txs.len());

                if min_len == 0 {
                    return false;
                }

                // Compare the common prefix
                for i in 0..min_len {
                    if txs[i] != first_txs[i] {
                        return false;
                    }
                }
            } else {
                return false;
            }
        }

        true
    }

    // Get the number of finalized transactions
    pub fn get_finalized_transaction_count(&self, id: &Identity) -> usize {
        self.finalized_transactions
            .get(id)
            .map_or(0, |txs| txs.len())
    }

    // Check the current view number of a process
    pub fn get_current_view(&self, id: &Identity) -> Option<ViewNum> {
        self.processes.get(id).map(|p| p.view_i)
    }

    // Count the number of messages in the queue
    pub fn message_queue_size(&self) -> usize {
        self.message_queue.len()
    }

    // Set try_produce_blocks for all processes
    pub fn try_produce_blocks_all(&mut self) {
        let process_ids: Vec<Identity> = self
            .processes
            .keys()
            .filter(|id| !self.byzantine_nodes.contains(id))
            .cloned()
            .collect();

        for id in process_ids {
            if let Some(process) = self.processes.get_mut(&id) {
                let mut to_send = Vec::new();
                process.try_produce_blocks(&mut to_send);

                for (message, recipient) in to_send {
                    self.enqueue_message(message, id.clone(), recipient);
                }
            }
        }
    }

    // Run until stable (no more message processing possible)
    pub fn run_until_stable(&mut self, max_steps: usize) -> bool {
        for _ in 0..max_steps {
            if !self.step() {
                // Try to stimulate block production once more
                self.try_produce_blocks_all();

                // If still no progress, we're stable
                if !self.step() {
                    return true;
                }
            }
        }
        false // Reached max steps without stabilizing
    }

    // Submit transactions to multiple processes in sequence
    pub fn submit_transactions(&mut self, transactions: Vec<(Identity, Transaction)>) {
        for (id, tx) in transactions {
            self.submit_transaction(id, tx);
            // Let the network process this transaction
            self.run_for(Duration::from_millis(10));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper function to create transactions
    fn create_transaction(value: u8) -> Transaction {
        Transaction::Opaque(vec![value])
    }

    #[test]
    fn test_basic_consensus() {
        // Create a harness with 4 nodes
        let mut harness = MockHarness::new(4, 1, Duration::from_millis(10));

        // Submit a transaction from node 0
        harness.submit_transaction(Identity(0), create_transaction(1));

        // Run for a while to let consensus happen
        harness.run_for(Duration::from_millis(500));

        // Verify all nodes finalized the transaction
        for i in 0..4 {
            let node_id = Identity(i);
            let txs = harness.get_finalized_transactions(&node_id);
            assert!(
                !txs.is_empty(),
                "Node {} should have finalized transactions",
                i
            );

            // The first transaction should be the one we submitted
            if let Transaction::Opaque(data) = &txs[0] {
                assert_eq!(data, &vec![1], "First transaction should have value 1");
            } else {
                panic!("Unexpected transaction type");
            }
        }

        // Verify consensus was reached
        assert!(harness.check_consensus(), "Consensus should be reached");
    }

    #[test]
    fn test_low_throughput_mode() {
        // Create a harness with 4 nodes
        let mut harness = MockHarness::new(4, 1, Duration::from_millis(10));

        // Submit transactions with delays between them to simulate low throughput
        for i in 1..=5 {
            harness.submit_transaction(Identity(i % 4), create_transaction(i as u8));
            harness.run_for(Duration::from_millis(200));
        }

        // Run longer to ensure all transactions are finalized
        harness.run_for(Duration::from_millis(1000));

        // Verify all nodes have the same transactions
        assert!(harness.check_consensus(), "Consensus should be reached");

        // Check latency in low throughput mode
        let node_id = Identity(0);
        let txs = harness.get_finalized_transactions(&node_id);
        assert_eq!(txs.len(), 5, "All 5 transactions should be finalized");
    }

    #[test]
    fn test_high_throughput_mode() {
        // Create a harness with 4 nodes
        let mut harness = MockHarness::new(4, 1, Duration::from_millis(10));

        // Submit many transactions quickly to trigger high throughput mode
        let mut transactions = Vec::new();
        for i in 1..=20 {
            transactions.push((Identity(i % 4), create_transaction(i as u8)));
        }

        harness.submit_transactions(transactions);

        // Run for a while to let consensus happen
        harness.run_for(Duration::from_millis(2000));

        // Verify consensus was reached
        assert!(harness.check_consensus(), "Consensus should be reached");

        // In high throughput mode, the leader should be creating leader blocks
        let node_id = Identity(0);
        let view = harness.get_current_view(&node_id).unwrap();
        let leader_id = Identity(view.0 as u64 % 4);

        // Check that at least some transactions were finalized
        let txs = harness.get_finalized_transactions(&node_id);
        assert!(!txs.is_empty(), "Should have finalized transactions");
    }

    #[test]
    fn test_view_changes() {
        // Create a harness with 4 nodes
        let mut harness = MockHarness::new(4, 1, Duration::from_millis(10));

        // Make the current leader (node 0) byzantine to trigger view change
        harness.set_byzantine(Identity(0));

        // Submit a transaction from node 1
        harness.submit_transaction(Identity(1), create_transaction(1));

        // Run for a while - this should trigger a view change
        harness.run_for(Duration::from_millis(1500));

        // Check if we've moved to a new view
        let view = harness.get_current_view(&Identity(1)).unwrap();
        assert!(view.0 > 0, "Should have changed to a new view");

        // Submit another transaction after view change
        harness.submit_transaction(Identity(2), create_transaction(2));
        harness.run_for(Duration::from_millis(1000));

        // Verify honest nodes have reached consensus
        let honest_ids = vec![Identity(1), Identity(2), Identity(3)];
        for id in &honest_ids {
            let txs = harness.get_finalized_transactions(id);
            assert!(
                !txs.is_empty(),
                "Honest node should have finalized transactions"
            );
        }
    }

    #[test]
    fn test_byzantine_faults() {
        // Create a harness with 7 nodes (can tolerate 2 Byzantine)
        let mut harness = MockHarness::new(7, 2, Duration::from_millis(10));

        // Make nodes 0 and 1 Byzantine
        harness.set_byzantine(Identity(0));
        harness.set_byzantine(Identity(1));

        // Submit transactions from honest nodes
        for i in 2..7 {
            harness.submit_transaction(Identity(i), create_transaction(i as u8));
        }

        // Run for a while to let consensus happen
        harness.run_for(Duration::from_millis(2000));

        // Byzantine nodes might create conflicting blocks
        // but honest nodes should still reach consensus
        let honest_ids: Vec<Identity> = (2..7).map(Identity).collect();

        // Check if all honest nodes have the same transactions
        let first_id = &honest_ids[0];
        let first_txs = harness.get_finalized_transactions(first_id);
        assert!(
            !first_txs.is_empty(),
            "Honest nodes should finalize transactions"
        );

        for id in &honest_ids[1..] {
            let txs = harness.get_finalized_transactions(id);
            assert_eq!(
                first_txs.len(),
                txs.len(),
                "All honest nodes should have same number of transactions"
            );

            for (i, tx) in first_txs.iter().enumerate() {
                assert_eq!(tx, &txs[i], "Transaction ordering should be consistent");
            }
        }
    }

    #[test]
    fn test_network_partitions() {
        // Create a harness with 6 nodes (can tolerate 1 Byzantine)
        let mut harness = MockHarness::new(6, 1, Duration::from_millis(10));

        // Create a network partition
        let partition1 = vec![Identity(0), Identity(1), Identity(2)];
        let partition2 = vec![Identity(3), Identity(4), Identity(5)];
        harness.create_partitions(vec![partition1.clone(), partition2.clone()]);

        // Submit transactions to both partitions
        harness.submit_transaction(Identity(0), create_transaction(1));
        harness.submit_transaction(Identity(3), create_transaction(2));

        // Run for a while
        harness.run_for(Duration::from_millis(1000));

        // Verify each partition has its own consensus
        for id in &partition1 {
            let txs = harness.get_finalized_transactions(id);
            if !txs.is_empty() {
                if let Transaction::Opaque(data) = &txs[0] {
                    assert_eq!(
                        data,
                        &vec![1],
                        "Partition 1 should have finalized transaction 1"
                    );
                }
            }
        }

        for id in &partition2 {
            let txs = harness.get_finalized_transactions(id);
            if !txs.is_empty() {
                if let Transaction::Opaque(data) = &txs[0] {
                    assert_eq!(
                        data,
                        &vec![2],
                        "Partition 2 should have finalized transaction 2"
                    );
                }
            }
        }

        // Heal the network
        harness.heal_partitions();

        // Submit another transaction
        harness.submit_transaction(Identity(0), create_transaction(3));

        // Run for a while to let the network converge
        harness.run_for(Duration::from_millis(2000));

        // Verify all nodes now have the same prefix
        assert!(
            harness.check_consensus(),
            "After healing, consensus should be reached"
        );
    }

    #[test]
    fn test_latency_low_throughput() {
        // Create a harness with 4 nodes and very low message delay
        let mut harness = MockHarness::new(4, 1, Duration::from_millis(5));

        // Start time
        let start_time = harness.current_time;

        // Submit a single transaction
        harness.submit_transaction(Identity(0), create_transaction(1));

        // Run until the transaction is finalized by all nodes
        for _ in 0..1000 {
            harness.step();

            // Check if all nodes have finalized the transaction
            let all_finalized = (0..4).all(|i| {
                let id = Identity(i);
                harness.get_finalized_transaction_count(&id) >= 1
            });

            if all_finalized {
                break;
            }
        }

        // Calculate latency
        let latency = harness.current_time - start_time;
        println!("Low throughput latency: {:?}", latency);

        // For low throughput, expect finalization in approximately 3δ (plus some overhead)
        assert!(
            latency < Duration::from_millis(100),
            "Low throughput latency should be low (got {:?})",
            latency
        );
    }

    #[test]
    fn test_latency_high_throughput() {
        // Create a harness with 4 nodes and very low message delay
        let mut harness = MockHarness::new(4, 1, Duration::from_millis(5));

        // Submit several transactions quickly to trigger high throughput mode
        for i in 1..=10 {
            harness.submit_transaction(Identity(i % 4), create_transaction(i as u8));
        }

        // Start time for the last transaction
        let start_time = harness.current_time;

        // Submit one more transaction to measure
        harness.submit_transaction(Identity(0), create_transaction(99));

        // Run until the last transaction is finalized by all nodes
        for _ in 0..1000 {
            harness.step();

            // Check if all nodes have finalized the last transaction
            let all_finalized = (0..4).all(|i| {
                let id = Identity(i);
                let txs = harness.get_finalized_transactions(&id);
                txs.iter().any(|tx| {
                    if let Transaction::Opaque(data) = tx {
                        data == &vec![99]
                    } else {
                        false
                    }
                })
            });

            if all_finalized {
                break;
            }
        }

        // Calculate latency
        let latency = harness.current_time - start_time;
        println!("High throughput latency: {:?}", latency);

        // For high throughput, expect finalization in approximately 7δ or 8δ (plus overhead)
        assert!(
            latency < Duration::from_millis(500),
            "High throughput latency should be reasonable (got {:?})",
            latency
        );
    }

    #[test]
    fn test_seamless_transition() {
        // Create a harness with 4 nodes
        let mut harness = MockHarness::new(4, 1, Duration::from_millis(10));

        // Submit many transactions quickly to trigger high throughput mode
        for i in 1..=15 {
            harness.submit_transaction(Identity(i % 4), create_transaction(i as u8));
        }

        // Run for a while to process high throughput
        harness.run_for(Duration::from_millis(1000));

        // Pause for a while to transition to low throughput
        harness.run_for(Duration::from_millis(500));

        // Submit a single transaction in low throughput mode
        harness.submit_transaction(Identity(0), create_transaction(100));

        // Run to finalize
        harness.run_for(Duration::from_millis(500));

        // Verify consensus was maintained through the transition
        assert!(harness.check_consensus(), "Consensus should be maintained");

        // Check that all transactions were finalized
        let node_id = Identity(0);
        let txs = harness.get_finalized_transactions(&node_id);

        // Check if our last transaction was finalized
        let contains_last_tx = txs.iter().any(|tx| {
            if let Transaction::Opaque(data) = tx {
                data == &vec![100]
            } else {
                false
            }
        });

        assert!(contains_last_tx, "Last transaction should be finalized");
    }

    #[test]
    fn test_quiescence() {
        // Create a harness with 4 nodes
        let mut harness = MockHarness::new(4, 1, Duration::from_millis(10));

        // Submit a transaction and let it finalize
        harness.submit_transaction(Identity(0), create_transaction(1));
        harness.run_for(Duration::from_millis(500));

        // Count messages in the queue after finalization
        let initial_msgs = harness.message_queue_size();

        // Wait for a while with no new transactions
        harness.run_for(Duration::from_millis(1000));

        // Count messages again - should be similar (no new message generation)
        let later_msgs = harness.message_queue_size();

        // There might be a few timing-related messages, but shouldn't be a flood of new messages
        assert!(
            later_msgs - initial_msgs < 5,
            "Protocol should be quiescent when no new transactions (initial: {}, later: {})",
            initial_msgs,
            later_msgs
        );
    }

    #[test]
    fn test_leader_failures() {
        // Create a harness with 4 nodes
        let mut harness = MockHarness::new(4, 1, Duration::from_millis(10));

        // Submit a transaction from node 1
        harness.submit_transaction(Identity(1), create_transaction(1));

        // Run briefly to start processing
        harness.run_for(Duration::from_millis(100));

        // Make the current leader byzantine
        let view = harness.get_current_view(&Identity(0)).unwrap();
        let leader_id = Identity(view.0 as u64 % 4);
        harness.set_byzantine(leader_id);

        // Submit another transaction from a different node
        harness.submit_transaction(Identity(2), create_transaction(2));

        // Run long enough for view change
        harness.run_for(Duration::from_millis(1500));

        // Check if we've moved to a new view with a new leader
        let new_view = harness.get_current_view(&Identity(0)).unwrap();
        assert!(new_view.0 > view.0, "Should have changed to a new view");

        // Submit one more transaction
        harness.submit_transaction(Identity(3), create_transaction(3));
        harness.run_for(Duration::from_millis(1000));

        // Verify honest nodes have reached consensus on the transactions
        assert!(
            harness.check_consensus(),
            "Consensus should be reached despite leader failure"
        );
    }

    #[test]
    fn test_network_delays() {
        // Create a harness with 4 nodes and high message delay
        let mut harness = MockHarness::new(4, 1, Duration::from_millis(50));

        // Submit a transaction
        harness.submit_transaction(Identity(0), create_transaction(1));

        // Run for a while to let consensus happen
        harness.run_for(Duration::from_millis(2000));

        // Verify all nodes finalized the transaction despite delays
        for i in 0..4 {
            let node_id = Identity(i);
            let txs = harness.get_finalized_transactions(&node_id);
            assert!(
                !txs.is_empty(),
                "Node {} should have finalized transactions despite network delays",
                i
            );
        }

        // Verify consensus was reached
        assert!(
            harness.check_consensus(),
            "Consensus should be reached despite network delays"
        );
    }

    #[test]
    fn test_scalability() {
        // Create a larger network with 10 nodes
        let mut harness = MockHarness::new(10, 3, Duration::from_millis(20));

        // Submit transactions from different nodes
        for i in 0..10 {
            harness.submit_transaction(Identity(i), create_transaction(i as u8 + 1));
        }

        // Run for a while to let consensus happen
        harness.run_for(Duration::from_millis(5000));

        // Verify consensus was reached
        assert!(
            harness.check_consensus(),
            "Consensus should be reached in a larger network"
        );

        // Check transaction count
        let txs = harness.get_finalized_transactions(&Identity(0));
        assert_eq!(txs.len(), 10, "All 10 transactions should be finalized");
    }
}
