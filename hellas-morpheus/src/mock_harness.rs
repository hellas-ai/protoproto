// Simulator that runs a mock network of nodes
//
// Time is "logical", we don't actually wait for anything to happen
// We call set_now to simulate the passage of time in single-step increments
//
// At each step, we deliver messages that are ready to be delivered.
// We process each message to completion, check timeouts, check block production eligibility, and finally advance the state of the simulation.

use std::collections::{BTreeMap, VecDeque};

use crate::*;

/// A basic simulation harness for MorpheusProcess
pub struct MockHarness {
    /// The current logical time of the simulation
    pub time: u128,

    /// The processes participating in the simulation
    pub processes: BTreeMap<Identity, MorpheusProcess>,

    /// Messages that are waiting to be delivered
    /// Each message is paired with its destination (None means broadcast)
    pub pending_messages: VecDeque<(Message, Option<Identity>)>,

    /// Time increment to use when advancing time
    pub time_step: u128,
}

impl MockHarness {
    /// Create a new mock harness with the given nodes
    pub fn new(nodes: Vec<MorpheusProcess>, time_step: u128) -> Self {
        let mut processes = BTreeMap::new();

        for node in nodes {
            let id = node.id.clone();
            processes.insert(id, node);
        }

        MockHarness {
            time: 0,
            processes,
            pending_messages: VecDeque::new(),
            time_step,
        }
    }

    pub fn process_round(&mut self) -> bool {
        let mut made_progress = false;

        let mut to_send = Vec::new();
        // Process all the messages from last round
        while !self.pending_messages.is_empty() {
            let (message, dest) = self.pending_messages.pop_front().unwrap();

            match dest {
                Some(id) => {
                    // Deliver to specific node
                    if let Some(process) = self.processes.get_mut(&id) {
                        let result = process.process_message(message, &mut to_send);

                        if result {
                            made_progress = true;
                        }
                    }
                }
                None => {
                    // Broadcast to all nodes
                    for (_, process) in self.processes.iter_mut() {
                        let mut to_send = Vec::new();
                        let result = process.process_message(message.clone(), &mut to_send);

                        if result {
                            made_progress = true;
                        }
                    }
                }
            }
        }
        self.pending_messages.extend(to_send);

        made_progress
    }

    /// Check timeouts for all nodes
    pub fn check_all_timeouts(&mut self) -> bool {
        let mut made_progress = false;

        for (_, process) in self.processes.iter_mut() {
            let mut to_send = Vec::new();
            process.check_timeouts(&mut to_send);
            process.try_produce_blocks(&mut to_send);

            if !to_send.is_empty() {
                made_progress = true;
                // Add any new messages to pending
                for msg in to_send {
                    self.pending_messages.push_back(msg);
                }
            }
        }

        made_progress
    }

    /// Advance time by the configured step
    pub fn advance_time(&mut self) {
        self.time += self.time_step;

        // Update time for all processes
        for (_, process) in self.processes.iter_mut() {
            process.set_now(self.time);
        }
    }

    /// Perform a single simulation step:
    /// 1. Process all messages
    /// 2. Check timeouts
    /// 3. Advance time
    pub fn step(&mut self) -> bool {
        let processed = self.process_round();
        let timeouts = self.check_all_timeouts();

        // Check if we made any progress
        let made_progress = processed || timeouts;

        // Advance time regardless of progress
        self.advance_time();

        made_progress
    }

    /// Run the simulation for the specified number of steps
    pub fn run(&mut self, steps: usize) -> bool {
        let mut made_progress = false;

        for _ in 0..steps {
            made_progress |= self.step();
        }

        made_progress
    }

    /// Add a message to the pending queue
    pub fn enqueue_message(&mut self, message: Message, destination: Option<Identity>) {
        self.pending_messages.push_back((message, destination));
    }
}
