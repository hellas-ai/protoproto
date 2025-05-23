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
    Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Hash, CanonicalDeserialize, CanonicalSerialize,
)]
pub struct TestTransaction(pub Vec<u8>);

impl Transaction for TestTransaction {}

/// A basic simulation harness for MorpheusProcess
#[derive(Clone)]
pub struct MockHarness {
    /// The current logical time of the simulation
    pub time: u128,

    /// The processes participating in the simulation
    pub processes: BTreeMap<Identity, MorpheusProcess<TestTransaction>>,

    /// Messages that are waiting to be delivered
    /// Each message is paired with its sender and destination (None means broadcast)
    pub pending_messages: VecDeque<(Message<TestTransaction>, Identity, Option<Identity>)>,

    /// Time increment to use when advancing time
    pub time_step: u128,

    pub steps: usize,

    /// Policy for generating transactions
    pub tx_gen_policy: BTreeMap<Identity, TxGenPolicy>,
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
    pub fn create_test_setup(num_parties: usize) -> MockHarness {
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
                        hints_setup: setup.clone(),
                    },
                    Identity(i as u32 + 1),
                    num_parties as u32,
                    (num_parties as u32 - 1) / 3,
                )
            })
            .collect();

        // Create a harness with these processes
        MockHarness::new(processes, 100)
    }

    /// Create a new mock harness with the given nodes
    pub fn new(nodes: Vec<MorpheusProcess<TestTransaction>>, time_step: u128) -> Self {
        let mut processes = BTreeMap::new();

        for mut node in nodes {
            node.delta = time_step;
            let id = node.id.clone();
            processes.insert(id, node);
        }

        MockHarness {
            time: 0,
            processes,
            pending_messages: VecDeque::new(),
            time_step,
            steps: 0,
            tx_gen_policy: BTreeMap::new(),
        }
    }

    pub fn process_round(&mut self) -> bool {
        let mut made_progress = false;

        let mut to_send = Vec::new();
        let mut next_round = Vec::new();
        // Process all the messages from last round
        while !self.pending_messages.is_empty() {
            let (message, sender, dest) = self.pending_messages.pop_front().unwrap();

            match dest {
                Some(id) => {
                    // Deliver to specific node
                    if let Some(process) = self.processes.get_mut(&id) {
                        let result = process.process_message(message, sender.clone(), &mut to_send);

                        if result {
                            made_progress = true;
                        }
                    }
                }
                None => {
                    // Broadcast to all (other) nodes
                    for (_, process) in self.processes.iter_mut() {
                        if process.id == sender {
                            continue;
                        }
                        let result =
                            process.process_message(message.clone(), sender.clone(), &mut to_send);

                        if result {
                            made_progress = true;
                        }
                    }
                }
            }

            next_round.extend(
                to_send
                    .drain(..)
                    .map(|(msg, dest)| (msg, sender.clone(), dest)),
            );
        }

        self.pending_messages.extend(next_round);

        made_progress
    }

    /// Check timeouts for all nodes
    pub fn check_all_timeouts(&mut self) -> bool {
        let mut made_progress = false;

        for (_, process) in self.processes.iter_mut() {
            let mut to_send = Vec::new();
            process.check_timeouts(&mut to_send);

            if !to_send.is_empty() {
                made_progress = true;
                // Add any new messages to pending
                for (msg, dest) in to_send {
                    self.pending_messages
                        .push_back((msg, process.id.clone(), dest));
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
        let produced = self.produce_blocks();

        // Check if we made any progress
        let made_progress = processed || timeouts || produced;

        // Advance time regardless of progress
        self.advance_time();

        self.steps += 1;

        for (_, process) in self.processes.iter() {
            let tips = process
                .index
                .tips
                .iter()
                .map(|qc| qc.data.clone())
                .collect::<Vec<_>>();
            tracing::info!(target: "process_state", process_id = ?process.id, time = self.time, steps = self.steps, tips = ?tips);
        }
        made_progress
    }

    /// Produce blocks for all nodes
    pub fn produce_blocks(&mut self) -> bool {
        let mut made_progress = false;
        for (_, process) in self.processes.iter_mut() {
            let mut to_send = Vec::new();
            match self.tx_gen_policy.get(&process.id) {
                Some(TxGenPolicy::EveryNSteps { n }) => {
                    if self.steps % n == 0 {
                        process
                            .ready_transactions
                            .push(TestTransaction(vec![1, 2, 3, 4]));
                    }
                }
                Some(TxGenPolicy::OncePerView { prev_view }) => {
                    if process.view_i != prev_view.read().unwrap().unwrap_or(ViewNum(-1)) {
                        process
                            .ready_transactions
                            .push(TestTransaction(vec![1, 2, 3, 4]));
                        *prev_view.write().unwrap() = Some(process.view_i);
                    }
                }
                Some(TxGenPolicy::Always) => {
                    process
                        .ready_transactions
                        .push(TestTransaction(vec![1, 2, 3, 4]));
                }
                None | Some(TxGenPolicy::Never) => {
                    // Do nothing
                }
            }
            process.try_produce_blocks(&mut to_send);
            for (msg, dest) in to_send {
                made_progress = true;
                self.pending_messages
                    .push_back((msg, process.id.clone(), dest));
            }
        }
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
    pub fn enqueue_message(
        &mut self,
        message: Message<TestTransaction>,
        sender: Identity,
        destination: Option<Identity>,
    ) {
        self.pending_messages
            .push_back((message, sender, destination));
    }
}
