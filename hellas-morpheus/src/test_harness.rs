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
use redb::ReadableTable;

use serde::{Deserialize, Serialize};

use crate::*;

#[derive(
    Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Hash, CanonicalDeserialize, CanonicalSerialize,
)]
pub struct TestTransaction(pub Vec<u8>);

impl Transaction for TestTransaction {}

/// A basic simulation harness for MorpheusProcess
pub struct MockHarness {
    /// The current logical time of the simulation
    pub time: u128,

    /// The processes participating in the simulation
    pub processes: BTreeMap<Identity, MorpheusProcess<TestTransaction>>,
    pub dbs: BTreeMap<Identity, redb::Database>,

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

        let dbs = (0..num_parties)
            .map(|i| {
                let db = redb::Builder::new()
                    .create_with_backend(redb::backends::InMemoryBackend::new())
                    .unwrap();
                (Identity(i as u32 + 1), db)
            })
            .collect::<BTreeMap<_, _>>();
        // Create processes with different identities
        let processes = (0..num_parties)
            .map(|i| {
                MorpheusProcess::new(
                    dbs.get(&Identity(i as u32 + 1)).unwrap(),
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
        MockHarness::new(processes, dbs, 100)
    }

    /// Create a new mock harness with the given nodes
    pub fn new(
        nodes: Vec<MorpheusProcess<TestTransaction>>,
        dbs: BTreeMap<Identity, redb::Database>,
        time_step: u128,
    ) -> Self {
        let mut processes = BTreeMap::new();

        for mut node in nodes {
            node.delta = time_step;
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
                        let result = process.process_message(
                            self.dbs.get(&id).unwrap(),
                            message,
                            sender.clone(),
                            &mut to_send,
                        );

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
                        let result = process.process_message(
                            self.dbs.get(&process.id).unwrap(),
                            message.clone(),
                            sender.clone(),
                            &mut to_send,
                        );

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
            let db = self.dbs.get(&process.id).unwrap();
            process.check_timeouts(db, &mut to_send);

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
            let db = self.dbs.get(&process.id).unwrap();
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
            process.try_produce_blocks(db, &mut to_send);
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

    /// Verify that all snapshots can be recreated by replaying from previous snapshots
    /// This ensures the protocol is deterministic - the same sequence of messages
    /// should always produce the same state regardless of which snapshot we start from
    pub fn verify_all_snapshots(&self) -> Result<(), String> {
        for (process_id, db) in &self.dbs {
            self.verify_snapshots_for_process(process_id, db)?;
        }
        Ok(())
    }

    /// Verify snapshots for a single process
    fn verify_snapshots_for_process(
        &self,
        process_id: &Identity,
        db: &redb::Database,
    ) -> Result<(), String> {
        // Get all snapshot message counts for this process
        let snapshot_counts = self.get_snapshot_counts(db)?;

        if snapshot_counts.len() < 2 {
            // Need at least 2 snapshots to do verification
            return Ok(());
        }

        tracing::info!(
            target: "snapshot_verification",
            process_id = ?process_id,
            snapshot_count = snapshot_counts.len(),
            "Starting snapshot verification"
        );

        // For each snapshot, try to recreate later snapshots by replaying
        for (i, &start_count) in snapshot_counts.iter().enumerate() {
            for &target_count in snapshot_counts.iter().skip(i + 1) {
                self.verify_snapshot_pair(process_id, db, start_count, target_count)?;
            }
        }

        tracing::info!(
            target: "snapshot_verification",
            process_id = ?process_id,
            "All snapshots verified successfully"
        );

        Ok(())
    }

    /// Verify that a target snapshot can be recreated by replaying from a start snapshot
    fn verify_snapshot_pair(
        &self,
        process_id: &Identity,
        db: &redb::Database,
        start_count: u64,
        target_count: u64,
    ) -> Result<(), String> {
        // Load the start snapshot
        let mut recreated = MorpheusProcess::<TestTransaction>::load_snapshot_at(db, start_count)
            .ok_or_else(|| {
            format!("Failed to load snapshot at message count {}", start_count)
        })?;

        // Replay messages from start to target
        recreated.replay_messages(db, target_count)?;

        // Load the expected target snapshot
        let expected = MorpheusProcess::<TestTransaction>::load_snapshot_at(db, target_count)
            .ok_or_else(|| format!("Failed to load snapshot at message count {}", target_count))?;

        // Compare the recreated state with the saved snapshot
        if recreated != expected {
            tracing::error!(
                target: "snapshot_verification_failed",
                process_id = ?process_id,
                start_count = start_count,
                target_count = target_count,
                recreated_state = ?recreated,
                expected_state = ?expected,
                "Snapshot verification failed: recreated state does not match expected state"
            );
            return Err(format!(
                "Snapshot verification failed for process {}: snapshot at message {} does not match when replayed from message {}",
                process_id.0, target_count, start_count
            ));
        }

        tracing::debug!(
            target: "snapshot_verification",
            process_id = ?process_id,
            start_count = start_count,
            target_count = target_count,
            "Successfully verified snapshot pair"
        );

        Ok(())
    }

    /// Get all snapshot message counts from the database
    fn get_snapshot_counts(&self, db: &redb::Database) -> Result<Vec<u64>, String> {
        let tx = db
            .begin_read()
            .map_err(|e| format!("Failed to begin read transaction: {}", e))?;
        let snapshots_table = snapshots_table_default::<TestTransaction>()
            .ok_or("Failed to get snapshots table definition")?;

        // Try to open the snapshots table - it might not exist if no snapshots have been saved yet
        let snapshots = match tx.open_table(snapshots_table) {
            Ok(table) => table,
            Err(redb::TableError::TableDoesNotExist(_)) => {
                // No snapshots have been saved yet, return empty list
                return Ok(Vec::new());
            }
            Err(e) => return Err(format!("Failed to open snapshots table: {}", e)),
        };

        let mut counts = Vec::new();
        for item in snapshots
            .iter()
            .map_err(|e| format!("Failed to iterate snapshots: {}", e))?
        {
            let (count, _) = item.map_err(|e| format!("Failed to read snapshot item: {}", e))?;
            counts.push(count.value());
        }

        counts.sort();
        Ok(counts)
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
