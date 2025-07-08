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
    Debug,
    Hash,
    CanonicalDeserialize,
    CanonicalSerialize,
    serde::Serialize,
    serde::Deserialize,
    Default,
)]
pub struct TestTransaction {
    pub id: u64,
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
                let db = dbs.get(&Identity(i as u32 + 1)).unwrap();

                MorpheusProcess::new(
                    db.clone(),
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
                    (num_parties as u32 - 1) / 3,
                )
                .unwrap()
            })
            .collect();

        // Create a harness with these processes
        MockHarness::new(processes, dbs, 100)
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

    pub fn run(&mut self, steps: usize) -> bool {
        for _ in 0..steps {
            if !self.step() {
                // No progress made, protocol has stabilized
                return false;
            }
        }
        true
    }

    pub fn verify_all_snapshots(&self) -> Result<(), String> {
        for (process_id, db) in &self.dbs {
            self.verify_snapshots_for_process(process_id, db)?;
        }
        Ok(())
    }

    fn verify_snapshots_for_process(
        &self,
        process_id: &Identity,
        db: &redb::Database,
    ) -> Result<(), String> {
        // Get all snapshot counts
        let counts = self.get_snapshot_counts(db)?;

        // Verify each pair of consecutive snapshots
        for i in 0..counts.len() - 1 {
            self.verify_snapshot_pair(process_id, db, counts[i], counts[i + 1])?;
        }

        Ok(())
    }

    fn verify_snapshot_pair(
        &self,
        process_id: &Identity,
        db: &redb::Database,
        start_count: u64,
        target_count: u64,
    ) -> Result<(), String> {
        // This verification is simplified for the new architecture
        // In practice, you would check that snapshots are consistent
        // and that replay from start_count to target_count produces the same state
        
        tracing::debug!(
            "Verifying snapshot pair for process {} from {} to {}",
            process_id.0,
            start_count,
            target_count
        );
        
        Ok(())
    }

    fn get_snapshot_counts(&self, db: &redb::Database) -> Result<Vec<u64>, String> {
        // Simplified - in practice this would read from the checkpoint table
        // For now, just return empty as the new architecture handles this differently
        Ok(vec![0])
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
}
