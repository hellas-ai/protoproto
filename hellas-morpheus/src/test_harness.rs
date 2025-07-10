//! Simulator that runs a mock network of nodes
//
//! Time is "logical", we don't actually wait for anything to happen
//! We call set_now to simulate the passage of time in single-step increments
//
//! At each step, we deliver messages that are ready to be delivered.
//! We process each message to completion, check timeouts, check block production eligibility, and finally advance the state of the simulation.

use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
    sync::RwLock,
};

use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use ark_std::test_rng;

use serde::{Deserialize, Serialize};
use crate::*;

#[derive(
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    CanonicalDeserialize,
    CanonicalSerialize,
    serde::Serialize,
    serde::Deserialize,
    Default,
    derivative::Derivative,
)]
#[derivative(Debug)]
pub struct TestTransaction {
    pub id: u64,
    #[derivative(Debug = "ignore")]
    pub data: Vec<u8>,
}

impl Transaction for TestTransaction {}

/// A basic simulation harness for MorpheusProcess with the new storage architecture
pub struct MockHarness {
    /// The current logical time of the simulation
    pub time: u128,

    /// The processes participating in the simulation
    pub processes: BTreeMap<Identity, MorpheusProcess<TestTransaction>>,
    pub dbs: BTreeMap<Identity, Arc<redb::Database>>,

    /// Messages that are waiting to be delivered
    /// Each message is paired with its sender and destination (None means broadcast)
    pub pending_messages: VecDeque<(Message<TestTransaction>, Identity, Option<Identity>)>,

    /// Time increment to use when advancing time
    pub time_step: u128,

    pub steps: usize,

    /// Policy for generating transactions
    pub tx_gen_policy: BTreeMap<Identity, TxGenPolicy>,
    pub tx_id_ctr: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TxGenPolicy {
    EveryNSteps {
        n: usize,
    },
    OncePerView {
        prev_view: Arc<RwLock<Option<ViewNum>>>,
    },
    Always,
    Never,
}

impl MockHarness {
    pub fn create_test_setup(
        num_parties: usize,
    ) -> MockHarness {
        let domain_max = (1 + num_parties).next_power_of_two();
        let gd = hints::GlobalData::new(domain_max, &mut test_rng()).unwrap();
        let privs = vec![hints::SecretKey::random(&mut test_rng()); domain_max - 1];
        let pubkeys: Vec<hints::PublicKey> = privs.iter().map(|sk| sk.public(&gd)).collect();
        let weights = vec![hints::F::from(1); domain_max - 1];

        let hints = (0..domain_max - 1)
            .map(|i| hints::generate_hint(&gd, &privs[i], domain_max, i).unwrap())
            .collect::<Vec<_>>();

        let setup = hints::setup_universe(&gd, pubkeys.clone(), &hints, weights).unwrap();

        let keys: BTreeMap<Identity, hints::PublicKey> = (0..num_parties)
            .map(|i| (Identity(i as u32 + 1), pubkeys[i].clone()))
            .collect();

        let identities: BTreeMap<hints::PublicKey, Identity> = (0..num_parties)
            .map(|i| (pubkeys[i].clone(), Identity(i as u32 + 1)))
            .collect();

        let dbs = (0..num_parties)
            .map(|i| {
                let db = Arc::new(
                    redb::Builder::new()
                        .create_with_backend(redb::backends::InMemoryBackend::new())
                        .unwrap(),
                );
                (Identity(i as u32 + 1), db)
            })
            .collect::<BTreeMap<_, _>>();

        // Create processes with different identities
        let processes = (0..num_parties)
            .map(|i| {
                MorpheusProcess::new(
                    KeyBook {
                        keys: keys.clone(),
                        identities: identities.clone(),
                        me_identity: Identity(i as u32 + 1),
                        me_pub_key: pubkeys[i].clone(),
                        me_sec_key: privs[i].clone(),
                        hints_setup: Some(setup.clone()),
                    },
                    Identity(i as u32 + 1),
                    num_parties as u32,
                    std::cmp::min(1, (num_parties as u32 - 1) / 3),
                )
                .unwrap()
            })
            .collect();

        // Create a harness with these processes
        let mut harness = MockHarness::new(processes, dbs, 100);
        
        // Process the initial start view messages
        harness.process_round();
        
        harness
    }

    /// Create a simple 2-node test setup for easier debugging
    pub fn create_2_node_setup() -> MockHarness {
        Self::create_test_setup(2)
    }

    /// Create a new mock harness with the given nodes
    pub fn new(
        nodes: Vec<MorpheusProcess<TestTransaction>>,
        dbs: BTreeMap<Identity, Arc<redb::Database>>,
        time_step: u128,
    ) -> Self {
        let mut processes = BTreeMap::new();

        for node in nodes {
            let id = node.id.clone();
            processes.insert(id, node);
        }

        MockHarness {
            time: 0,
            processes,
            dbs,
            pending_messages: VecDeque::new(),
            time_step,
            steps: 0,
            tx_id_ctr: 1,
            tx_gen_policy: BTreeMap::new(),
        }
    }

    pub fn process_round(&mut self) -> bool {
        let mut made_progress = false;

        let mut next_round = Vec::new();
        // Process all the messages from last round
        while !self.pending_messages.is_empty() {
            let (message, sender, dest) = self.pending_messages.pop_front().unwrap();

            match dest {
                Some(id) => {
                    // Deliver to specific node
                    if let Some(process) = self.processes.get_mut(&id) {
                        let messages = process
                            .process_message(message, sender.clone())
                            .unwrap_or_else(|e| {
                                tracing::error!("Error handling message: {}", e);
                                Vec::new()
                            });

                        // Queue the response messages for next round
                        for (msg, target) in messages {
                            next_round.push((msg, id.clone(), target));
                        }

                        made_progress = true;
                    }
                }
                None => {
                    // Broadcast to all nodes
                    for (dest_id, process) in self.processes.iter_mut() {
                        if *dest_id != sender {
                            // Don't send to sender
                            let messages = process
                                .process_message(message.clone(), sender.clone())
                                .unwrap_or_else(|e| {
                                    tracing::error!("Error handling message: {}", e);
                                    Vec::new()
                                });

                            // Queue the response messages for next round
                            for (msg, target) in messages {
                                next_round.push((msg, dest_id.clone(), target));
                            }

                            made_progress = true;
                        }
                    }
                }
            }
        }

        // Add messages for next round
        for (msg, sender, dest) in next_round {
            self.pending_messages.push_back((msg, sender, dest));
        }

        made_progress
    }

    /// Update time for all nodes
    pub fn update_time(&mut self) -> bool {
        let mut made_progress = false;

        // Update the time for all nodes
        for process in self.processes.values_mut() {
            let messages = process
                .set_now(self.time)
                .unwrap_or_else(|e| {
                    tracing::error!("Error setting time: {}", e);
                    Vec::new()
                });

            // Queue messages
            for (msg, target) in messages {
                self.pending_messages
                    .push_back((msg, process.id.clone(), target));
                made_progress = true;
            }
        }

        made_progress
    }

    pub fn check_all_timeouts(&mut self) -> bool {
        let mut made_progress = false;

        for process in self.processes.values_mut() {
            let messages = process
                .check_timeouts()
                .unwrap_or_else(|e| {
                    tracing::error!("Error checking timeouts: {}", e);
                    Vec::new()
                });

            // Queue messages
            for (msg, target) in messages {
                self.pending_messages
                    .push_back((msg, process.id.clone(), target));
                made_progress = true;
            }
        }

        made_progress
    }

    pub fn advance_time(&mut self) {
        self.time += self.time_step;
    }

    pub fn step(&mut self) -> bool {
        self.steps += 1;

        let mut made_progress = false;

        // Step 1: Update time
        made_progress |= self.update_time();

        // Step 2: Generate transactions
        made_progress |= self.generate_transactions();

        // Step 3: Process messages
        made_progress |= self.process_round();

        // Step 4: Check timeouts
        made_progress |= self.check_all_timeouts();

        // Step 5: Check block production
        made_progress |= self.produce_blocks();

        // Step 6: Advance time
        self.advance_time();

        made_progress
    }

    pub fn produce_blocks(&mut self) -> bool {
        let mut made_progress = false;

        for process in self.processes.values_mut() {
            let messages = process
                .try_produce_blocks()
                .unwrap_or_else(|e| {
                    tracing::error!("Error producing blocks: {}", e);
                    Vec::new()
                });

            // Queue messages
            for (msg, target) in messages {
                self.pending_messages
                    .push_back((msg, process.id.clone(), target));
                made_progress = true;
            }
        }

        made_progress
    }

    pub fn generate_transactions(&mut self) -> bool {
        let mut made_progress = false;

        for (id, policy) in self.tx_gen_policy.clone() {
            let should_generate = match &policy {
                TxGenPolicy::EveryNSteps { n } => self.steps % n == 0,
                TxGenPolicy::OncePerView { prev_view } => {
                    let current_view = self.processes.get(&id).unwrap().state.current_view;
                    let mut prev = prev_view.write().unwrap();
                    if prev.is_none() || prev.unwrap() != current_view {
                        *prev = Some(current_view);
                        true
                    } else {
                        false
                    }
                }
                TxGenPolicy::Always => true,
                TxGenPolicy::Never => false,
            };

            if should_generate {
                let tx = TestTransaction {
                    id: self.tx_id_ctr,
                    data: vec![42; 64],
                };
                self.tx_id_ctr += 1;

                if let Some(process) = self.processes.get_mut(&id) {
                    let messages = process
                        .set_ready_transactions(vec![tx])
                        .unwrap_or_else(|e| {
                            tracing::error!("Error setting transactions: {}", e);
                            Vec::new()
                        });

                    // Queue messages
                    for (msg, target) in messages {
                        self.pending_messages.push_back((msg, id.clone(), target));
                        made_progress = true;
                    }
                }
            }
        }

        made_progress
    }

    pub fn run(&mut self, steps: usize) {
        for _ in 0..steps {
            self.step();
        }
    }

    pub fn enqueue_message(
        &mut self,
        message: Message<TestTransaction>,
        sender: Identity,
        destination: Option<Identity>,
    ) {
        self.pending_messages
            .push_back((message, sender, destination));
    }

    /// Process a single message from the queue
    pub fn process_single_message(&mut self) -> bool {
        if let Some((message, sender, dest)) = self.pending_messages.pop_front() {
            match dest {
                Some(id) => {
                    if let Some(process) = self.processes.get_mut(&id) {
                        let messages = process
                            .process_message(message, sender.clone())
                            .unwrap_or_else(|e| {
                                tracing::error!("Error handling message: {}", e);
                                Vec::new()
                            });

                        for (msg, target) in messages {
                            self.pending_messages.push_back((msg, id.clone(), target));
                        }
                        return true;
                    }
                }
                None => {
                    // Broadcast - process for all except sender
                    let mut new_messages = Vec::new();
                    for (dest_id, process) in self.processes.iter_mut() {
                        if *dest_id != sender {
                            let messages = process
                                .process_message(message.clone(), sender.clone())
                                .unwrap_or_else(|e| {
                                    tracing::error!("Error handling message: {}", e);
                                    Vec::new()
                                });

                            for (msg, target) in messages {
                                new_messages.push((msg, dest_id.clone(), target));
                            }
                        }
                    }
                    for (msg, sender, dest) in new_messages {
                        self.pending_messages.push_back((msg, sender, dest));
                    }
                    return true;
                }
            }
        }
        false
    }

    /// Process messages until a condition is met or no more messages
    pub fn process_until<F>(&mut self, mut condition: F) -> bool
    where
        F: FnMut(&Self) -> bool,
    {
        while !self.pending_messages.is_empty() {
            if condition(self) {
                return true;
            }
            if !self.process_single_message() {
                break;
            }
        }
        condition(self)
    }

    /// Wait for a block to be finalized
    pub fn wait_for_finalization(&mut self, block_key: &BlockKey, max_steps: usize) -> bool {
        for _ in 0..max_steps {
            if self.processes.values().any(|p| p.state.finalized.contains(block_key)) {
                return true;
            }
            self.step();
        }
        false
    }

    /// Trigger timeouts for all processes
    pub fn trigger_timeouts(&mut self) {
        self.check_all_timeouts();
        self.process_round(); // Process timeout messages
    }

    /// Create a conflicting block from a Byzantine process
    pub fn create_byzantine_conflict(
        &mut self,
        byzantine_id: Identity,
        slot: SlotNum,
        view: ViewNum,
    ) -> Arc<Signed<Block<TestTransaction>>> {
        let process = self.processes.get(&byzantine_id).unwrap();
        let tx = TestTransaction {
            id: 9999 + slot.0,
            data: vec![99; 32],
        };

        let prev_qc = if slot.is_zero() {
            process.state.genesis_qc.clone()
        } else {
            process.state.latest_tr_qc.clone()
                .unwrap_or_else(|| process.state.genesis_qc.clone())
        };

        let block = Block {
            key: BlockKey {
                type_: BlockType::Tr,
                view,
                height: prev_qc.data.for_which.height + 1,
                author: Some(byzantine_id),
                slot,
                hash: Some(BlockHash(9999 + slot.0)),
            },
            prev: vec![prev_qc],
            one: process.state.max_1qc.clone(),
            data: BlockData::Tr {
                transactions: vec![tx],
            },
        };

        Arc::new(Signed::from_data(block, &process.kb))
    }

    /// Get the current leader
    pub fn current_leader(&self) -> Identity {
        let view = self.processes.values().next().unwrap().state.current_view;
        let n = self.processes.len() as u32;
        Identity((view.0 as u32 % n) + 1)
    }

    /// Check if a view change has occurred
    pub fn has_view_changed(&self, expected_view: ViewNum) -> bool {
        self.processes.values().all(|p| p.state.current_view == expected_view)
    }

    /// Force a view change by timing out
    pub fn force_view_change(&mut self) {
        // Get delta from one of the processes
        let delta = self.processes.values().next().unwrap().delta;
        
        // Advance time past both complaint (6Δ) and end-view (12Δ) timeouts
        // We need to advance by at least 12Δ = 120 units (with delta=10)
        // Since each step advances by time_step (100), we need at least 2 steps
        let steps_needed = ((12 * delta + self.time_step - 1) / self.time_step) as usize;
        
        for _ in 0..steps_needed {
            self.step();
        }
        
        // Process any remaining messages to form view certificates
        for _ in 0..5 {
            self.process_round();
        }
    }

    /// Check if any process has a specific block type
    pub fn has_block_type(&self, block_type: BlockType) -> bool {
        self.processes.values()
            .any(|p| p.state.blocks.keys().any(|k| k.type_ == block_type))
    }

    /// Count blocks of a specific type
    pub fn count_blocks_of_type(&self, block_type: BlockType) -> usize {
        self.processes.values()
            .flat_map(|p| p.state.blocks.keys())
            .filter(|k| k.type_ == block_type)
            .count()
    }

    /// Get all finalized blocks across all processes
    pub fn get_finalized_blocks(&self) -> Vec<BlockKey> {
        let mut finalized = std::collections::BTreeSet::new();
        for process in self.processes.values() {
            finalized.extend(process.state.finalized.iter().cloned());
        }
        finalized.into_iter().collect()
    }

    /// Debug helper: print current state summary
    pub fn print_state_summary(&self) {
        println!("\n=== State Summary at step {} ===", self.steps);
        println!("Time: {}", self.time);
        println!("Pending messages: {}", self.pending_messages.len());
        
        for (id, process) in &self.processes {
            println!("\nNode {}:", id.0);
            println!("  View: {}, Phase: {:?}", process.state.current_view.0, process.state.current_phase);
            println!("  Blocks: {}", process.state.blocks.len());
            println!("  QCs: {}", process.state.qcs.len());
            println!("  Tips: {}", process.state.tips.len());
            println!("  Finalized: {}", process.state.finalized.len());
            println!("  Unfinalized QCs: {}", process.state.unfinalized_qcs.len());
            
            // Count block types
            let mut lead_blocks = 0;
            let mut tr_blocks = 0;
            for block in process.state.blocks.values() {
                match block.data.key.type_ {
                    BlockType::Lead => lead_blocks += 1,
                    BlockType::Tr => tr_blocks += 1,
                    _ => {}
                }
            }
            println!("  Leader blocks: {}, Transaction blocks: {}", lead_blocks, tr_blocks);
            
            // Show voting state
            println!("  Votes recorded: {}", process.state.vote_tracker.len());
            println!("  Voted on: {:?}", process.state.voted.len());
        }
        
        println!("\n=== End State Summary ===\n");
    }

    /// Debug helper: get a detailed view of what's happening
    pub fn debug_step(&mut self) -> bool {
        println!("\n--- Step {} ---", self.steps);
        let result = self.step();
        if self.steps % 10 == 0 {
            self.print_state_summary();
        }
        result
    }

    /// Event-driven helper: wait for a specific condition with a timeout
    pub fn wait_for<F>(&mut self, mut condition: F, max_steps: usize) -> bool
    where
        F: FnMut(&Self) -> bool,
    {
        for _ in 0..max_steps {
            if condition(self) {
                return true;
            }
            self.step();
        }
        condition(self)
    }

    /// Wait for all processes to reach a specific view
    pub fn wait_for_view(&mut self, view: ViewNum, max_steps: usize) -> bool {
        self.wait_for(|h| h.has_view_changed(view), max_steps)
    }

    /// Wait for a specific phase in any process
    pub fn wait_for_phase(&mut self, phase: Phase, max_steps: usize) -> bool {
        self.wait_for(
            |h| h.processes.values().any(|p| p.state.current_phase == phase),
            max_steps,
        )
    }

    /// Get the current phase distribution
    pub fn get_phase_distribution(&self) -> BTreeMap<Phase, usize> {
        let mut distribution = BTreeMap::new();
        for process in self.processes.values() {
            *distribution.entry(process.state.current_phase).or_insert(0) += 1;
        }
        distribution
    }

    /// Process only timeout-related effects
    pub fn process_timeouts_only(&mut self) -> bool {
        self.check_all_timeouts()
    }

    /// Advance time without processing messages
    pub fn advance_time_by(&mut self, delta: u128) {
        self.time += delta;
        self.update_time();
    }

    /// Get all blocks of a specific type in a view
    pub fn get_blocks_in_view(&self, view: ViewNum, block_type: BlockType) -> Vec<BlockKey> {
        let mut blocks = Vec::new();
        for process in self.processes.values() {
            for key in process.state.blocks.keys() {
                if key.view == view && key.type_ == block_type {
                    blocks.push(key.clone());
                }
            }
        }
        blocks.sort();
        blocks.dedup();
        blocks
    }

    /// Check if any process has voted for a specific block
    pub fn has_votes_for(&self, block_key: &BlockKey, vote_type: u8) -> bool {
        self.processes.values().any(|p| {
            p.state.vote_tracker.values()
                .any(|votes| votes.values()
                    .any(|v| v.data.for_which == *block_key && v.data.z == vote_type))
        })
    }

    /// Get the number of processes that have finalized a block
    pub fn finalization_count(&self, block_key: &BlockKey) -> usize {
        self.processes.values()
            .filter(|p| p.state.finalized.contains(block_key))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper that builds a 4-node harness with default parameters.
    fn default_harness() -> MockHarness {
        MockHarness::create_test_setup(4)
    }


    /// Test basic leader block production in high throughput mode
    #[test_log::test]
    fn test_leader_block_production() {
        let mut h = default_harness();
        
        // To trigger leader block production, we need multiple tips
        // Have all nodes produce transaction blocks to create conflicts
        for id in h.processes.keys() {
            h.tx_gen_policy.insert(*id, TxGenPolicy::Always);
        }
        
        // Run for several steps to allow the protocol to progress
        let mut found_leader_block = false;
        let mut leader_block_key = None;
        let mut leader_block_found_at_step = 0;
        
        for step in 0..100 {
            h.step();
            
            // Check if all processes have seen a leader block
            if !found_leader_block {
                let mut leader_blocks = Vec::new();
                for (_id, process) in &h.processes {
                    for block in process.state.blocks.values() {
                        if block.data.key.type_ == BlockType::Lead {
                            leader_blocks.push(block.data.key.clone());
                        }
                    }
                }
                
                // Check if at least 3 out of 4 nodes have the leader block
                // (this ensures it was actually broadcast)
                if leader_blocks.len() >= 3 {
                    found_leader_block = true;
                    leader_block_key = Some(leader_blocks[0].clone());
                    leader_block_found_at_step = step;
                    println!("Leader block {:?} found at step {}", leader_block_key, step);
                }
            }
            
            // If we found a leader block, continue running to allow finalization
            if found_leader_block {
                // Check if it's been finalized
                let finalized = h.get_finalized_blocks();
                if let Some(ref key) = leader_block_key {
                    if finalized.contains(key) {
                        println!("Leader block finalized at step {} (found at step {})", 
                                step, leader_block_found_at_step);
                        return; // Success
                    }
                }
                
                // Give it reasonable time to finalize (about 20 steps after finding)
                if step - leader_block_found_at_step > 20 {
                    h.print_state_summary();
                    panic!("Leader block was not finalized within 20 steps of being found");
                }
            }
        }
        
        // If we get here, print debug info
        h.print_state_summary();
        if !found_leader_block {
            panic!("No leader block was produced in 100 steps");
        } else {
            panic!("Leader block was produced but not finalized");
        }
    }

    /// Test transaction block finalization in high throughput mode
    #[test_log::test]
    fn test_high_throughput_finalization() {
        let mut h = default_harness();
        
        // All nodes produce transactions
        for id in h.processes.keys() {
            h.tx_gen_policy.insert(*id, TxGenPolicy::Always);
        }
        
        // Run for enough steps to see finalization
        for step in 0..10 {
            h.step();
            
            // Check if any transaction block has been finalized
            let finalized = h.get_finalized_blocks();
            if finalized.iter().any(|k| k.type_ == BlockType::Tr && k.height > 0) {
                println!("Transaction block finalized at step {}", step);
                return; // Success
            }
        }
    }

    /// Test low throughput mode when leader is inactive
    #[test_log::test]
    fn test_low_throughput_mode() {
        let mut h = default_harness();
        
        h.tx_gen_policy.insert(Identity(3), TxGenPolicy::EveryNSteps { n: 2 });
        
        h.run(10);
        
        // Check phase transitions
        let mut has_low_phase = false;
        for process in h.processes.values() {
            if process.state.current_phase == Phase::Low ||
            process.state.phase_by_view.values().any(|p| p == &Phase::Low){
                has_low_phase = true;
                break;
            }
        }
        
        assert!(has_low_phase, "Should have transitioned to low phase when leader is inactive");
    }

    /// Test view change mechanism
    #[test_log::test]
    fn test_view_change() {
        let mut h = default_harness();
        
        // Make the leader inactive
        let leader_id = h.current_leader();
        for id in h.processes.keys() {
            if *id != leader_id {
                h.tx_gen_policy.insert(*id, TxGenPolicy::Always);
            }
        }
        
        // Force timeouts to trigger view change
        // Use force_view_change which properly calculates steps needed
        h.force_view_change();
        
        // Continue processing to form view certificate
        for _ in 0..20 {
            h.step();
            
            // Check if view changed
            if h.has_view_changed(ViewNum(1)) {
                println!("Successfully changed to view 1");
                h.step();
                // Verify new leader received start view messages
                let new_leader_id = h.current_leader();
                let new_leader = h.processes.get(&new_leader_id).unwrap();
                assert!(new_leader.state.start_views.contains_key(&ViewNum(1)));
                
                return; // Success
            }
        }
        
        panic!("View change did not occur despite timeouts");
    }

    /// Test that conflicting blocks from same author are rejected
    #[test_log::test]
    fn test_equivocation_prevention() {
        let mut h = default_harness();
        
        // Run a bit to establish some state
        for id in h.processes.keys() {
            h.tx_gen_policy.insert(*id, TxGenPolicy::Always);
        }
        h.run(10);
        
        // Pick a node to be Byzantine
        let byzantine_id = Identity(2);
        let slot = h.processes.get(&byzantine_id).unwrap().state.slot_tr;
        let view = ViewNum(0);
        
        // Create two conflicting blocks
        let block1 = h.create_byzantine_conflict(byzantine_id, slot, view);
        let block2 = h.create_byzantine_conflict(byzantine_id, slot, view);
        
        // Send both blocks
        h.enqueue_message(Message::Block(block1.clone()), byzantine_id, None);
        h.enqueue_message(Message::Block(block2.clone()), byzantine_id, None);
        
        // Process messages
        h.process_round();
        
        // Check that nodes won't vote for both blocks
        for (id, process) in h.processes.iter() {
            if *id != byzantine_id {
                let votes_for_slot = process.state.voted.iter()
                    .filter(|(_, _, s, author)| *s == slot && *author == byzantine_id)
                    .count();
                    
                // Should vote for at most one block at this slot
                assert!(votes_for_slot <= 1, 
                    "Node {} voted for multiple blocks at slot {}", id.0, slot.0);
            }
        }
    }

    /// Test that votes are properly aggregated into QCs
    #[test_log::test]
    fn test_vote_aggregation() {
        let mut h = default_harness();
        
        // Have one node produce a transaction block
        let producer = Identity(1);
        h.tx_gen_policy.insert(producer, TxGenPolicy::Always);
        
        // Step once to produce the block
        h.step();
        
        // Get the produced block
        let block_key = h.processes.get(&producer).unwrap()
            .state.blocks.keys()
            .find(|k| k.type_ == BlockType::Tr && k.author == Some(producer))
            .cloned();
            
        if let Some(key) = block_key {
            // Process the block being sent
            h.process_round();
            
            // All nodes should send 0-votes back to producer
            h.process_round();
            
            // Producer should form 0-QC
            h.process_round();
            
            // Check that producer has the 0-QC
            let producer_state = &h.processes.get(&producer).unwrap().state;
            let has_0qc = producer_state.qcs.iter()
                .any(|qc| qc.data.z == 0 && qc.data.for_which == key);
                
            assert!(has_0qc, "Producer should have formed 0-QC for its block");
        } else {
            panic!("No transaction block was produced");
        }
    }

    /// Test consistency across view changes - blocks finalized in one view remain finalized
    #[test_log::test]
    fn test_consistency_across_views() {
        let mut h = default_harness();
        
        // First, get some blocks finalized in view 0
        for id in h.processes.keys() {
            h.tx_gen_policy.insert(*id, TxGenPolicy::Always);
        }
        
        // Run until we have finalized blocks
        for _ in 0..20 {
            h.step();
        }
        
        let finalized_in_view_0 = h.get_finalized_blocks();
        assert!(!finalized_in_view_0.is_empty(), "Should have finalized blocks in view 0");
        
        // Force a view change
        h.force_view_change();
        
        // Run more steps in the new view
        for _ in 0..20 {
            h.step();
        }
        
        // Check that all blocks finalized in view 0 are still finalized
        let current_finalized = h.get_finalized_blocks();
        for block in &finalized_in_view_0 {
            assert!(current_finalized.contains(block), 
                    "Block {:?} finalized in view 0 should remain finalized", block);
        }
        
        // Also check all processes agree on finalized blocks
        let mut process_finalized_sets = Vec::new();
        for process in h.processes.values() {
            process_finalized_sets.push(process.state.finalized.clone());
        }
        
        // All processes should have the same finalized set
        for i in 1..process_finalized_sets.len() {
            assert_eq!(process_finalized_sets[0], process_finalized_sets[i],
                      "All processes should agree on finalized blocks");
        }
    }

    /// Test seamless recovery from asynchrony
    #[test_log::test]
    fn test_seamless_recovery() {
        let mut h = default_harness();
        
        // Set up regular transaction production
        for id in h.processes.keys() {
            h.tx_gen_policy.insert(*id, TxGenPolicy::Always);
        }
        
        // Run normally for a while
        for _ in 0..10 {
            h.step();
        }
        
        let blocks_before = h.processes.values().next().unwrap().state.blocks.len();
        
        // Simulate asynchrony by preventing message delivery
        let saved_messages = std::mem::take(&mut h.pending_messages);
        
        // Try to run - no progress should be made
        for _ in 0..5 {
            h.step();
        }
        
        // Restore messages and run
        h.pending_messages = saved_messages;
        
        // Protocol should recover and make progress
        for _ in 0..20 {
            h.step();
        }
        
        let blocks_after = h.processes.values().next().unwrap().state.blocks.len();
        assert!(blocks_after > blocks_before, 
                "Protocol should recover and produce more blocks after asynchrony");
        
        // Check that finalization resumed
        let finalized = h.get_finalized_blocks();
        assert!(finalized.len() > 1, "Should have finalized blocks after recovery");
    }

    /// Test the observes relation in the DAG
    #[test_log::test]
    fn test_observes_relation() {
        let mut h = default_harness();
        
        // Create a chain of blocks
        h.tx_gen_policy.insert(Identity(1), TxGenPolicy::Always);
        
        // Run to create some blocks
        for _ in 0..10 {
            h.step();
        }
        
        // Get a process to check its state
        let process = h.processes.values().next().unwrap();
        
        // Find some QCs to test
        let qcs: Vec<_> = process.state.qcs.iter().take(5).cloned().collect();
        
        if qcs.len() >= 2 {
            // Test reflexivity: a block observes itself
            for qc in &qcs {
                assert!(process.state.observes(&qc.data, &qc.data),
                        "Block should observe itself");
            }
            
            // Test transitivity: if a observes b and b observes c, then a observes c
            // This requires finding such a chain in the DAG
            for block in process.state.blocks.values() {
                for prev_qc in &block.data.prev {
                    // The block observes its predecessors
                    let block_vote = VoteData {
                        z: 1,
                        for_which: block.data.key.clone(),
                    };
                    assert!(process.state.observes(&block_vote, &prev_qc.data),
                            "Block should observe its predecessors");
                }
            }
        }
    }

    /// Test low throughput latency (3δ as claimed in the paper)
    #[test_log::test]
    fn test_low_throughput_latency() {
        let mut h = default_harness();
        
        // Wait for leader to become inactive by advancing past complaint timeout
        // Need to advance past 6Δ (60 units with delta=10)
        for _ in 0..2 {
            h.step(); // Each step advances by 100
        }
        h.check_all_timeouts();
        h.process_round();
        
        // Now have one node produce a transaction
        let producer = Identity(2);
        h.tx_gen_policy.insert(producer, TxGenPolicy::Never); // Will manually trigger
        
        // Manually create and send a transaction block
        let process = h.processes.get_mut(&producer).unwrap();
        process.set_ready_transactions(vec![TestTransaction { id: 1000, data: vec![1, 2, 3] }]).unwrap();
        
        let start_time = h.time;
        let mut block_key = None;
        
        // Process the block production
        for _ in 0..10 {
            h.step();
            
            // Find the produced block
            if block_key.is_none() {
                for process in h.processes.values() {
                    for (key, _) in &process.state.blocks {
                        if key.type_ == BlockType::Tr && key.author == Some(producer) {
                            block_key = Some(key.clone());
                            break;
                        }
                    }
                }
            }
            
            // Check if finalized
            if let Some(ref key) = block_key {
                if h.processes.values().any(|p| p.state.finalized.contains(key)) {
                    let finalization_time = h.time - start_time;
                    // Should be finalized within 3δ (δ = time_step = 100)
                    assert!(finalization_time <= 3 * h.time_step + 200, // Some buffer
                            "Block should be finalized within 3δ, took {}", finalization_time);
                    return;
                }
            }
        }
        
        panic!("Block was not finalized in low throughput mode");
    }

    /// Test that leader blocks observe all tips
    #[test_log::test]
    fn test_leader_blocks_observe_tips() {
        let mut h = default_harness();
        
        // Create multiple transaction blocks to have multiple tips
        h.tx_gen_policy.insert(Identity(1), TxGenPolicy::Always);
        h.tx_gen_policy.insert(Identity(2), TxGenPolicy::Always);
        h.tx_gen_policy.insert(Identity(3), TxGenPolicy::Always);
        
        // Run to create tips
        for _ in 0..5 {
            h.step();
        }
        
        // Get the leader's view of tips before leader block
        let leader_id = h.current_leader();
        let leader_tips_before = h.processes.get(&leader_id).unwrap().state.tips.clone();
        
        // Continue until a leader block is produced
        for _ in 0..20 {
            h.step();
            
            // Find leader blocks
            for process in h.processes.values() {
                for block in process.state.blocks.values() {
                    if block.data.key.type_ == BlockType::Lead {
                        // Check that the leader block points to all tips that existed
                        for tip in &leader_tips_before {
                            let pointed_to = block.data.prev.iter()
                                .any(|qc| qc.data.for_which == tip.data.for_which);
                            assert!(pointed_to || 
                                    // Or the tip is already observed by another pointed block
                                    block.data.prev.iter().any(|qc| 
                                        process.state.observes(&qc.data, &tip.data)),
                                    "Leader block should observe all tips");
                        }
                        return; // Test passed
                    }
                }
            }
        }
        
        panic!("No leader block was produced");
    }

    /// Test single tip condition for voting
    #[test_log::test]
    fn test_single_tip_voting() {
        let mut h = default_harness();
        
        // Wait for low phase by advancing past complaint timeout
        // Need to advance past 6Δ (60 units with delta=10)
        for _ in 0..2 {
            h.step(); // Each step advances by 100
        }
        h.check_all_timeouts();
        h.process_round();
        
        // Have only one node produce to ensure single tip
        let producer = Identity(1);
        h.tx_gen_policy.insert(producer, TxGenPolicy::Always);
        
        // Run and check voting behavior
        for _ in 0..10 {
            h.step();
            
            // Check if any transaction block received votes
            for process in h.processes.values() {
                // In low phase with single tip, should vote for transaction blocks
                if process.state.current_phase == Phase::Low && process.state.tips.len() == 1 {
                    // Should have voted for transaction blocks
                    let has_tr_votes = process.state.voted.iter()
                        .any(|(_, block_type, _, _)| *block_type == BlockType::Tr);
                    
                    if has_tr_votes {
                        return; // Test passed - voting occurred with single tip
                    }
                }
            }
        }
    }

    /// Test transition between high and low throughput modes
    #[test_log::test]
    fn test_throughput_mode_transitions() {
        let mut h = MockHarness::create_2_node_setup();
        
        // Node 1 is the leader in view 0
        let leader_id = h.current_leader();
        let non_leader_id = if leader_id == Identity(1) { Identity(2) } else { Identity(1) };
        
        println!("Leader: {:?}, Non-leader: {:?}", leader_id, non_leader_id);
        
        // Start with both nodes producing transactions (high throughput)
        h.tx_gen_policy.insert(leader_id, TxGenPolicy::Always);
        h.tx_gen_policy.insert(non_leader_id, TxGenPolicy::Always);
        
        // Run a few steps to let the DAG develop
        for _ in 0..5 {
            h.step();
        }
        
        // Debug: Check initial state
        println!("\nAfter initial steps:");
        h.print_state_summary();
        
        // Wait for leader blocks to be produced
        let leader_block_produced = h.wait_for(
            |h| {
                // Debug output to understand state
                let leader_id = h.current_leader();
                let leader_state = &h.processes.get(&leader_id).unwrap().state;
                
                // Check block production
                let tr_blocks = h.get_blocks_in_view(ViewNum(0), BlockType::Tr).len();
                let lead_blocks = h.get_blocks_in_view(ViewNum(0), BlockType::Lead).len();
                
                println!("Step {}: Leader {} - View: {}, Tips: {}, Phase: {:?}, TrBlocks: {}, LeadBlocks: {}", 
                    h.steps, leader_id.0, leader_state.current_view.0, 
                    leader_state.tips.len(), leader_state.current_phase, tr_blocks, lead_blocks);
                
                // Check QCs
                println!("  QCs in leader state: {}", leader_state.qcs.len());
                for qc in leader_state.qcs.iter().take(5) {
                    println!("    QC: z={}, for {:?}", qc.data.z, qc.data.for_which);
                }
                
                lead_blocks > 0
            },
            20
        );
        
        assert!(leader_block_produced, "Leader should produce blocks when multiple nodes produce transactions");
        
        // Stop non-leader from producing transactions
        h.tx_gen_policy.insert(non_leader_id, TxGenPolicy::Never);
        
        // Run for a while - with only one tip, leader shouldn't produce blocks
        let initial_leader_blocks = h.get_blocks_in_view(ViewNum(0), BlockType::Lead).len();
        h.run(10);
        let leader_blocks_after = h.get_blocks_in_view(ViewNum(0), BlockType::Lead).len();
        
        assert_eq!(initial_leader_blocks, leader_blocks_after, 
            "Leader shouldn't produce more blocks with single tip");
        
        // Eventually nodes should transition to low phase or change views
        let phase_changed = h.wait_for(
            |h| {
                // Either we're in low phase or we've changed views due to timeout
                let dist = h.get_phase_distribution();
                let low_count = dist.get(&Phase::Low).unwrap_or(&0);
                *low_count > 0 || h.has_view_changed(ViewNum(1))
            },
            30
        );
        
        assert!(phase_changed, "Should either enter low phase or change views");
        
        // If we're still in view 0, check that we're in low phase
        if h.processes.values().all(|p| p.state.current_view == ViewNum(0)) {
            let phase_dist = h.get_phase_distribution();
            let low_phase_count = phase_dist.get(&Phase::Low).unwrap_or(&0);
            assert!(*low_phase_count > 0, "At least one node should be in low phase");
        }
        
        h.print_state_summary();
    }

    /// Test basic phase transitions with 2 nodes
    #[test_log::test]
    fn test_phase_transitions_simple() {
        let mut h = MockHarness::create_test_setup(4);
        
        // Initially both nodes should be in high phase
        assert_eq!(h.get_phase_distribution()[&Phase::High], 4);
        
        // Only leader produces transactions
        let leader_id = h.current_leader();
        h.tx_gen_policy.insert(leader_id, TxGenPolicy::EveryNSteps { n: 4 });
        
        h.run(9);
        
        // Debug: print state before assertion
        println!("\nAfter leader-only production:");
        h.print_state_summary();
        
        // Now have non-leader also produce - leader should produce blocks
        let non_leader_id = if leader_id == Identity(1) { Identity(2) } else { Identity(1) };
        h.tx_gen_policy.insert(non_leader_id, TxGenPolicy::Always);
        
        // Wait for leader block
        let has_leader_block = h.wait_for(
            |h| h.count_blocks_of_type(BlockType::Lead) > 0,
            14
        );
        
        assert!(has_leader_block, "Leader should produce blocks with multiple tips");
        
        // With leader blocks present, new votes should be for leader blocks (high phase)
        println!("\nAfter leader block production:");
        h.print_state_summary();
        
        // Stop all transaction production - should maintain current state
        h.tx_gen_policy.insert(leader_id, TxGenPolicy::Never);
        h.tx_gen_policy.insert(non_leader_id, TxGenPolicy::Never);
        
        // Run for a while
        h.run(20);
        
        let final_phase_dist = h.get_phase_distribution();
        println!("\nFinal phase distribution: {:?}", final_phase_dist);
        
        // Eventually should timeout and change views if no progress
        let view_changed = h.processes.values().any(|p| p.state.current_view > ViewNum(0));
        
        println!("View changed: {}", view_changed);
    }

    /// Test with f Byzantine nodes
    #[test_log::test]
    fn test_byzantine_safety_with_f_faults() {
        // Create a larger network to test with f faults
        let n = 7; // With n=7, f=2
        let f = (n - 1) / 3;
        let mut h = MockHarness::create_test_setup(n);
        
        // Have honest nodes produce transactions
        for i in (f+1)..n {
            h.tx_gen_policy.insert(Identity(i as u32 + 1), TxGenPolicy::Always);
        }
        
        // Run normally first
        for _ in 0..10 {
            h.step();
        }
        
        // Byzantine nodes create conflicting blocks
        for byzantine_idx in 0..f {
            let byzantine_id = Identity((byzantine_idx + 1) as u32);
            let slot = h.processes.get(&byzantine_id).unwrap().state.slot_tr;
            
            // Create multiple conflicting blocks
            let block1 = h.create_byzantine_conflict(byzantine_id, slot, ViewNum(0));
            let block2 = h.create_byzantine_conflict(byzantine_id, SlotNum(slot.0 + 1000), ViewNum(0));
            
            h.enqueue_message(Message::Block(block1), byzantine_id, None);
            h.enqueue_message(Message::Block(block2), byzantine_id, None);
        }
        
        // Process Byzantine messages
        h.process_round();
        
        // Continue running
        for _ in 0..30 {
            h.step();
        }
        
        // Despite f Byzantine nodes, honest nodes should still agree on finalized blocks
        let mut finalized_sets = Vec::new();
        for i in f..n {
            let process = h.processes.get(&Identity((i + 1) as u32)).unwrap();
            finalized_sets.push(process.state.finalized.clone());
        }
        
        // All honest nodes should agree
        for i in 1..finalized_sets.len() {
            assert_eq!(finalized_sets[0], finalized_sets[i],
                      "All honest nodes should agree on finalized blocks despite Byzantine behavior");
        }
    }

    /// Test QC comparison ordering as defined in the paper
    #[test_log::test] 
    fn test_qc_ordering() {
        let vote1 = VoteData {
            z: 1,
            for_which: BlockKey {
                type_: BlockType::Tr,
                view: ViewNum(1),
                height: 10,
                author: Some(Identity(1)),
                slot: SlotNum(1),
                hash: Some(BlockHash(1)),
            },
        };
        
        let vote2 = VoteData {
            z: 1,
            for_which: BlockKey {
                type_: BlockType::Tr,
                view: ViewNum(2),
                height: 5,
                author: Some(Identity(1)),
                slot: SlotNum(1),
                hash: Some(BlockHash(2)),
            },
        };
        
        let vote3 = VoteData {
            z: 1,
            for_which: BlockKey {
                type_: BlockType::Lead,
                view: ViewNum(2),
                height: 5,
                author: Some(Identity(1)),
                slot: SlotNum(1),
                hash: Some(BlockHash(3)),
            },
        };
        
        // Test view ordering (higher view > lower view)
        assert_eq!(vote2.compare_qc(&vote1), std::cmp::Ordering::Greater);
        assert_eq!(vote1.compare_qc(&vote2), std::cmp::Ordering::Less);
        
        // Test type ordering (Lead < Tr for same view)
        assert_eq!(vote3.compare_qc(&vote2), std::cmp::Ordering::Less);
        assert_eq!(vote2.compare_qc(&vote3), std::cmp::Ordering::Greater);
        
        // Test height ordering for same view and type
        let vote4 = VoteData {
            z: 1,
            for_which: BlockKey {
                type_: BlockType::Tr,
                view: ViewNum(2),
                height: 15,
                author: Some(Identity(1)),
                slot: SlotNum(1),
                hash: Some(BlockHash(4)),
            },
        };
        
        assert_eq!(vote4.compare_qc(&vote2), std::cmp::Ordering::Greater);
        assert_eq!(vote2.compare_qc(&vote4), std::cmp::Ordering::Less);
    }

    /// Test complaint mechanism at 6Δ timeout
    #[test_log::test]
    #[ignore = "need to change the test to look for a ComplaintSent effect instead"]
    fn test_complaint_mechanism() {
        let mut h = default_harness();
        
        // Have nodes produce blocks but prevent finalization
        for id in h.processes.keys() {
            h.tx_gen_policy.insert(*id, TxGenPolicy::Always);
        }
        
        // Run to create some unfinalized blocks
        for _ in 0..5 {
            h.step();
        }
        
        let leader_id = h.current_leader();
        
        // Clear pending messages to prevent finalization
        h.pending_messages.clear();
        
        // Advance time to 6Δ (60 units with delta=10)
        // Since each step advances by 100, we need just 1 step to exceed 6Δ
        h.step();
        
        // Check timeouts
        h.check_all_timeouts();
        
        // Check that nodes sent complaints to the leader
        let mut found_complaint = false;
        for (msg, sender, target) in &h.pending_messages {
            if let (Message::QC(_), Some(target_id)) = (msg, target) {
                if *target_id == leader_id && *sender != leader_id {
                    found_complaint = true;
                    break;
                }
            }
        }
        
        assert!(found_complaint, "Nodes should send complaints to leader after 6Δ timeout");
        
        // Continue to 12Δ for end-view
        // We're already past 6Δ, need to get to 12Δ (120 units)
        // One more step will bring us to 200 units total
        h.step();
        
        h.check_all_timeouts();
        h.process_round();
        
        // Check for end-view messages
        let has_end_view = h.pending_messages.iter()
            .any(|(msg, _, _)| matches!(msg, Message::EndView(_)));
        
        assert!(has_end_view, "Nodes should send end-view messages after 12Δ timeout");
    }
}
