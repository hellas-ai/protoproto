use std::collections::{HashMap, VecDeque};

use crate::types::{Message, ProcessId, MorpheusProcess};

/// A mock network harness for testing Morpheus protocol with multiple nodes.
///
/// This harness creates and manages multiple Morpheus process instances,
/// delivering messages between them in a controlled way. Messages are delivered
/// in the order they were sent, but can be delayed or dropped if needed for testing.
pub struct MorpheusHarness {
    /// The collection of processes being tested
    processes: HashMap<ProcessId, MorpheusProcess>,
    /// Message queues for each process
    message_queues: HashMap<ProcessId, VecDeque<Message>>,
    /// Number of processes in the system
    n: usize,
    /// Maximum number of faulty processes the system can tolerate
    f: usize,
    /// A record of all messages sent during the test
    message_history: Vec<(ProcessId, ProcessId, Message)>,
}

impl MorpheusHarness {
    /// Create a new mock harness with the specified number of processes.
    ///
    /// # Arguments
    ///
    /// * `n` - Total number of processes in the system
    /// * `f` - Maximum number of faulty processes the system can tolerate
    ///
    /// # Returns
    ///
    /// A new `MorpheusHarness` instance with n processes initialized
    pub fn new(n: usize, f: usize) -> Self {
        let mut processes = HashMap::new();
        let mut message_queues = HashMap::new();

        // Create n processes with IDs from 0 to n-1
        for i in 0..n {
            let id = ProcessId(i);
            let process = MorpheusProcess::new(id, n, f);
            processes.insert(id, process);
            message_queues.insert(id, VecDeque::new());
        }

        MorpheusHarness {
            processes,
            message_queues,
            n,
            f,
            message_history: Vec::new(),
        }
    }

    /// Get a reference to a process by ID.
    ///
    /// # Arguments
    ///
    /// * `id` - The ID of the process to retrieve
    ///
    /// # Returns
    ///
    /// An Option containing a reference to the process, or None if not found
    pub fn get_process(&self, id: ProcessId) -> Option<&MorpheusProcess> {
        self.processes.get(&id)
    }

    /// Get a mutable reference to a process by ID.
    ///
    /// # Arguments
    ///
    /// * `id` - The ID of the process to retrieve
    ///
    /// # Returns
    ///
    /// An Option containing a mutable reference to the process, or None if not found
    pub fn get_process_mut(&mut self, id: ProcessId) -> Option<&mut MorpheusProcess> {
        self.processes.get_mut(&id)
    }

    /// Send a message from one process to another.
    ///
    /// # Arguments
    ///
    /// * `from` - The ID of the sending process
    /// * `to` - The ID of the receiving process
    /// * `message` - The message to send
    pub fn send_message(&mut self, from: ProcessId, to: ProcessId, message: Message) {
        if let Some(queue) = self.message_queues.get_mut(&to) {
            queue.push_back(message.clone());
            self.message_history.push((from, to, message));
        }
    }

    /// Broadcast a message from one process to all other processes.
    ///
    /// # Arguments
    ///
    /// * `from` - The ID of the sending process
    /// * `message` - The message to broadcast
    pub fn broadcast_message(&mut self, from: ProcessId, message: Message) {
        for i in 0..self.n {
            let to = ProcessId(i);
            if to != from {
                self.send_message(from, to, message.clone());
            }
        }
    }

    /// Deliver the next message to a process, if any.
    ///
    /// # Arguments
    ///
    /// * `to` - The ID of the process to deliver a message to
    ///
    /// # Returns
    ///
    /// `true` if a message was delivered, `false` otherwise
    pub fn deliver_next_message(&mut self, to: ProcessId) -> bool {
        if let Some(queue) = self.message_queues.get_mut(&to) {
            if let Some(message) = queue.pop_front() {
                if let Some(process) = self.processes.get_mut(&to) {
                    process.process_message(message);
                    return true;
                }
            }
        }
        false
    }

    /// Deliver all queued messages to a process.
    ///
    /// # Arguments
    ///
    /// * `to` - The ID of the process to deliver messages to
    ///
    /// # Returns
    ///
    /// The number of messages delivered
    pub fn deliver_all_messages(&mut self, to: ProcessId) -> usize {
        let mut count = 0;
        while self.deliver_next_message(to) {
            count += 1;
        }
        count
    }

    /// Run a step for a process and deliver any resulting messages.
    ///
    /// # Arguments
    ///
    /// * `id` - The ID of the process to run a step for
    ///
    /// # Returns
    ///
    /// The number of messages sent by the process
    pub fn run_step(&mut self, id: ProcessId) -> usize {
        if let Some(process) = self.processes.get_mut(&id) {
            let messages = process.step();
            let count = messages.len();

            // Send each message to all other processes
            for message in messages {
                self.broadcast_message(id, message);
            }

            count
        } else {
            0
        }
    }

    /// Run steps for all processes and deliver all messages until no more messages are generated.
    ///
    /// # Arguments
    ///
    /// * `max_iterations` - Maximum number of iterations to run to prevent infinite loops
    ///
    /// # Returns
    ///
    /// The total number of steps executed across all processes
    pub fn run_until_completion(&mut self, max_iterations: usize) -> usize {
        let mut total_steps = 0;
        let mut any_messages = true;
        let mut iterations = 0;

        while any_messages && iterations < max_iterations {
            iterations += 1;
            any_messages = false;

            // Run a step for each process
            for i in 0..self.n {
                let id = ProcessId(i);
                
                // Deliver all messages to this process
                let delivered = self.deliver_all_messages(id);
                if delivered > 0 {
                    any_messages = true;
                }

                // Run a step
                let sent = self.run_step(id);
                if sent > 0 {
                    any_messages = true;
                    total_steps += 1;
                }
            }
        }

        if iterations >= max_iterations {
            println!("Warning: Run until completion reached the maximum iteration limit of {}", max_iterations);
        }

        total_steps
    }

    /// Run a limited number of steps for each process.
    ///
    /// # Arguments
    ///
    /// * `steps_per_process` - The number of steps to run for each process
    ///
    /// # Returns
    ///
    /// The total number of messages delivered
    pub fn run_steps(&mut self, steps_per_process: usize) -> usize {
        let mut total_messages = 0;

        for _ in 0..steps_per_process {
            for i in 0..self.n {
                let id = ProcessId(i);
                
                // Deliver all messages to this process
                total_messages += self.deliver_all_messages(id);

                // Run a step
                self.run_step(id);
            }
        }

        total_messages
    }

    /// Get a copy of the message history.
    ///
    /// # Returns
    ///
    /// A vector of (sender, receiver, message) tuples
    pub fn get_message_history(&self) -> &Vec<(ProcessId, ProcessId, Message)> {
        &self.message_history
    }
} 