//! Web-based visualization and simulation harness for the Morpheus protocol
//! 
//! This module provides the infrastructure for:
//! - Capturing simulation state at each step
//! - Supporting simulation branching and rewinding
//! - Exposing simulation data to a web frontend via WASM
//! - Visualizing protocol execution across multiple nodes

use std::collections::{BTreeMap, VecDeque, HashSet};
use std::sync::{Arc, Mutex};
use std::rc::Rc;
use std::cell::RefCell;
use serde::{Serialize, Deserialize};

use crate::*;
use crate::test_harness::TxGenPolicy;

use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

/// Tracks a complete snapshot of a simulation at a point in time
#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "webviz", derive(Tsify))]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct SimulationSnapshot {
    /// Unique identifier for this snapshot
    pub id: String,
    
    /// Current simulation time
    pub time: u128,
    
    /// Current step count
    pub step_count: usize,
    
    /// Process states at this point in time
    pub processes: BTreeMap<u64, ProcessSnapshot>,
    
    /// Messages in flight at this point in time
    pub pending_messages: Vec<MessageSnapshot>,
    
    /// Events that occurred during this step
    pub events: Vec<SimulationEvent>,
}

/// Captures the relevant state of a process for visualization
#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "webviz", derive(Tsify))]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct ProcessSnapshot {
    /// Process identity
    pub id: u64,
    
    /// Current view
    pub view: i64,
    
    /// Current phase in the view (High throughput or Low throughput)
    pub phase: String,
    
    /// Current slots for transaction and leader blocks
    pub slot_tr: u64,
    pub slot_lead: u64,
    
    /// Blocks in this process's state
    pub blocks: Vec<BlockSnapshot>,
    
    /// QCs in this process's state
    pub qcs: Vec<QCSnapshot>,
    
    /// Tips of the block DAG
    pub tips: Vec<String>,
    
    /// Finalized blocks
    pub finalized_blocks: Vec<String>,
    
    /// Unfinalized blocks
    pub unfinalized_blocks: Vec<String>,
}

/// Simplified block representation for visualization
#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "webviz", derive(Tsify))]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct BlockSnapshot {
    /// Block identifier (for visualization references)
    pub id: String,
    
    /// Block type (Genesis, Lead, Transaction)
    pub block_type: String,
    
    /// Block view number
    pub view: i64,
    
    /// Block height in the DAG
    pub height: usize,
    
    /// Block author
    pub author: Option<u64>,
    
    /// Block slot number
    pub slot: u64,
    
    /// Previous blocks this block points to
    pub prev: Vec<String>,
    
    /// 1-QC this block contains
    pub one_qc: String,
    
    /// Is this block finalized?
    pub finalized: bool,
}

/// Simplified QC representation for visualization 
#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "webviz", derive(Tsify))]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct QCSnapshot {
    /// QC identifier (for visualization references)
    pub id: String,
    
    /// QC level (0, 1, or 2)
    pub z: u8,
    
    /// Block this QC is for
    pub for_block: String,
    
    /// Block type this QC is for
    pub block_type: String,
    
    /// View number this QC is for
    pub view: i64,
    
    /// Whether this QC is part of the tips
    pub is_tip: bool,
}

/// Simplified message representation for visualization
#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "webviz", derive(Tsify))]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct MessageSnapshot {
    /// Message type (Block, NewVote, QC, etc.)
    pub message_type: String,
    
    /// Message data (simplified for visualization)
    pub data: String,
    
    /// Sender of the message
    pub sender: u64,
    
    /// Recipient of the message (None for broadcast)
    pub recipient: Option<u64>,
}

/// Events that occur during simulation steps
#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "webviz", derive(Tsify))]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub enum SimulationEvent {
    /// A block was created
    BlockCreated {
        /// Block identifier 
        block_id: String,
        /// Process that created the block
        process_id: u64,
        /// Block type
        block_type: String,
    },
    
    /// A QC was formed
    QCFormed {
        /// QC identifier
        qc_id: String,
        /// Process that formed the QC
        process_id: u64,
        /// QC level
        z: u8,
    },
    
    /// A block was finalized
    BlockFinalized {
        /// Block identifier
        block_id: String,
        /// Process that finalized the block
        process_id: u64,
    },
    
    /// A view change occurred
    ViewChange {
        /// Process that changed view
        process_id: u64,
        /// Old view
        old_view: i64,
        /// New view
        new_view: i64,
        /// Reason for view change
        reason: String,
    },
    
    /// A message was sent
    MessageSent {
        /// Sender process
        sender: u64,
        /// Recipient process (None for broadcast)
        recipient: Option<u64>,
        /// Message type
        message_type: String,
    },
    
    /// Phase changed within a view
    PhaseChange {
        /// Process that changed phase
        process_id: u64,
        /// View number
        view: i64,
        /// Old phase
        old_phase: String,
        /// New phase
        new_phase: String,
    },
}

/// Configurations for simulation visualization
#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "webviz", derive(Tsify))]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct VisualizationConfig {
    /// Maximum number of steps to keep in history
    pub max_history: usize,
    
    /// Whether to automatically capture state after each step
    pub auto_capture: bool,
    
    /// Whether to track messages for sequence diagrams
    pub track_messages: bool,
    
    /// Whether to track events
    pub track_events: bool,
    
    /// Process IDs to focus on (None for all)
    pub focus_processes: Option<Vec<u64>>,
}

/// Represents the history of a simulation with branches
pub struct SimulationHistory {
    /// Main timeline snapshots
    main_timeline: Vec<SimulationSnapshot>,
    
    /// Branch timelines (branch_id -> snapshots)
    branches: BTreeMap<String, Vec<SimulationSnapshot>>,
    
    /// Currently active branch (None for main timeline)
    active_branch: Option<String>,
    
    /// Position in the active timeline
    position: usize,
    
    /// Maximum number of snapshots to keep
    max_history: usize,
}

impl SimulationHistory {
    pub fn new(max_history: usize) -> Self {
        SimulationHistory {
            main_timeline: Vec::with_capacity(max_history),
            branches: BTreeMap::new(),
            active_branch: None,
            position: 0,
            max_history,
        }
    }
    
    /// Add a snapshot to the current active timeline
    pub fn add_snapshot(&mut self, snapshot: SimulationSnapshot) {
        let timeline = if let Some(branch) = &self.active_branch {
            self.branches.get_mut(branch).unwrap()
        } else {
            &mut self.main_timeline
        };
        
        // If we're not at the end, truncate the timeline
        if self.position < timeline.len() {
            timeline.truncate(self.position);
        }
        
        // Add the new snapshot
        timeline.push(snapshot);
        self.position = timeline.len();
        
        // Enforce history limits
        if timeline.len() > self.max_history {
            timeline.remove(0);
            self.position -= 1;
        }
    }
    
    /// Go back in time by a number of steps
    pub fn rewind(&mut self, steps: usize) -> Option<&SimulationSnapshot> {
        let timeline = if let Some(branch) = &self.active_branch {
            self.branches.get(branch).unwrap()
        } else {
            &self.main_timeline
        };
        
        if steps < self.position {
            self.position -= steps;
            timeline.get(self.position)
        } else if !timeline.is_empty() {
            self.position = 0;
            timeline.first()
        } else {
            None
        }
    }
    
    /// Go forward in time by a number of steps
    pub fn forward(&mut self, steps: usize) -> Option<&SimulationSnapshot> {
        let timeline = if let Some(branch) = &self.active_branch {
            self.branches.get(branch).unwrap()
        } else {
            &self.main_timeline
        };
        
        let new_position = self.position + steps;
        if new_position < timeline.len() {
            self.position = new_position;
            timeline.get(self.position)
        } else if !timeline.is_empty() {
            self.position = timeline.len() - 1;
            timeline.last()
        } else {
            None
        }
    }
    
    /// Create a new branch from the current position
    pub fn create_branch(&mut self, branch_name: String) -> Option<String> {
        let timeline = if let Some(branch) = &self.active_branch {
            self.branches.get(branch).unwrap()
        } else {
            &self.main_timeline
        };
        
        if self.position < timeline.len() {
            let current_snapshot = timeline.get(self.position).cloned()?;
            let new_branch = vec![current_snapshot];
            self.branches.insert(branch_name.clone(), new_branch);
            self.active_branch = Some(branch_name.clone());
            self.position = 0;
            Some(branch_name)
        } else {
            None
        }
    }
    
    /// Switch to a different branch
    pub fn switch_branch(&mut self, branch_name: Option<String>) -> Option<&SimulationSnapshot> {
        if let Some(branch) = &branch_name {
            if !self.branches.contains_key(branch) {
                return None;
            }
        }
        
        self.active_branch = branch_name;
        self.position = 0;
        
        let timeline = if let Some(branch) = &self.active_branch {
            self.branches.get(branch).unwrap()
        } else {
            &self.main_timeline
        };
        
        timeline.get(self.position)
    }
    
    /// Get the current snapshot
    pub fn current_snapshot(&self) -> Option<&SimulationSnapshot> {
        let timeline = if let Some(branch) = &self.active_branch {
            self.branches.get(branch).unwrap_or(&self.main_timeline)
        } else {
            &self.main_timeline
        };
        
        timeline.get(self.position)
    }
    
    /// Get a list of all available branches
    pub fn get_branches(&self) -> Vec<String> {
        self.branches.keys().cloned().collect()
    }
}

/// Manages the simulation for web visualization
#[wasm_bindgen]
pub struct MorpheusWorld {
    /// The actual simulation harness
    harness: test_harness::MockHarness,
    
    /// History of simulation states
    history: SimulationHistory,
    
    /// Configuration for the visualization
    config: VisualizationConfig,
    
    /// Events collected during the current step
    current_events: Vec<SimulationEvent>,
    
    /// Whether the simulation is currently at a branch point
    at_branch_point: bool,
    
    /// Message history for sequence diagrams
    message_history: Vec<MessageSnapshot>,
}

#[wasm_bindgen]
impl MorpheusWorld {
    /// Create a new simulation world
    pub fn new(processes: Vec<MorpheusProcess>, time_step: u128, config: VisualizationConfig) -> Self {
        let harness = test_harness::MockHarness::new(processes, time_step);
        
        // Capture initial state
        let initial_snapshot = Self::capture_state_from_harness(&harness, 0, Vec::new());
        
        let mut history = SimulationHistory::new(config.max_history);
        history.add_snapshot(initial_snapshot);
        
        MorpheusWorld {
            harness,
            history,
            config,
            current_events: Vec::new(),
            at_branch_point: false,
            message_history: Vec::new(),
        }
    }
    
    /// Advance the simulation by one step
    #[wasm_bindgen]
    pub fn step(&mut self) -> bool {
        // Clear events from previous step
        self.current_events.clear();
        
        // If we're at a branch point, we need to create a new branch
        if self.at_branch_point {
            self.create_branch(format!("branch_{}", self.history.position));
            self.at_branch_point = false;
        }
        
        // Custom event listener to capture events during the step
        let events_ref = Rc::new(RefCell::new(&mut self.current_events));
        let listener = {
            let events = events_ref.clone();
            move |event_type: &str, data: &str| {
                if let Ok(mut events) = events.try_borrow_mut() {
                    match event_type {
                        "block_created" => {
                            if let Some((process_id, block_type, block_id)) = 
                                Self::parse_block_created(data) {
                                events.push(SimulationEvent::BlockCreated {
                                    process_id,
                                    block_type,
                                    block_id,
                                });
                            }
                        }
                        "qc_formed" => {
                            if let Some((process_id, z, qc_id)) = 
                                Self::parse_qc_formed(data) {
                                events.push(SimulationEvent::QCFormed {
                                    process_id,
                                    z,
                                    qc_id,
                                });
                            }
                        }
                        "view_change" => {
                            if let Some((process_id, old_view, new_view, reason)) = 
                                Self::parse_view_change(data) {
                                events.push(SimulationEvent::ViewChange {
                                    process_id,
                                    old_view,
                                    new_view,
                                    reason,
                                });
                            }
                        }
                        "message_sent" => {
                            if let Some((sender, recipient, message_type)) = 
                                Self::parse_message_sent(data) {
                                events.push(SimulationEvent::MessageSent {
                                    sender,
                                    recipient,
                                    message_type,
                                });
                            }
                        }
                        "phase_change" => {
                            if let Some((process_id, view, old_phase, new_phase)) = 
                                Self::parse_phase_change(data) {
                                events.push(SimulationEvent::PhaseChange {
                                    process_id,
                                    view,
                                    old_phase,
                                    new_phase,
                                });
                            }
                        }
                        "block_finalized" => {
                            if let Some((process_id, block_id)) = 
                                Self::parse_block_finalized(data) {
                                events.push(SimulationEvent::BlockFinalized {
                                    process_id,
                                    block_id,
                                });
                            }
                        }
                        _ => {}
                    }
                }
            }
        };
        
        // Perform the simulation step
        let made_progress = self.harness.step();
        
        // Create a snapshot if auto-capture is enabled
        if self.config.auto_capture {
            self.capture_current_state();
        }
        
        made_progress
    }
    
    /// Run the simulation for multiple steps
    #[wasm_bindgen]
    pub fn run(&mut self, steps: usize) -> bool {
        let mut made_progress = false;
        for _ in 0..steps {
            made_progress |= self.step();
        }
        made_progress
    }
    
    /// Create a snapshot of the current simulation state
    #[wasm_bindgen]
    pub fn capture_current_state(&mut self) {
        let snapshot = Self::capture_state_from_harness(
            &self.harness, 
            self.history.position,
            self.current_events.clone()
        );
        self.history.add_snapshot(snapshot);
    }
    
    /// Rewind the simulation to a previous state
    #[wasm_bindgen]
    pub fn rewind(&mut self, steps: usize) -> bool {
        if let Some(snapshot) = self.history.rewind(steps).cloned() {
            self.restore_from_snapshot(&snapshot);
            self.at_branch_point = true;
            true
        } else {
            false
        }
    }
    
    /// Move forward in the simulation history
    #[wasm_bindgen]
    pub fn forward(&mut self, steps: usize) -> bool {
        if let Some(snapshot) = self.history.forward(steps).cloned() {
            self.restore_from_snapshot(&snapshot);
            true
        } else {
            false
        }
    }
    
    /// Create a new branch from the current state
    #[wasm_bindgen]
    pub fn create_branch(&mut self, branch_name: String) -> Option<String> {
        self.history.create_branch(branch_name)
    }
    
    /// Switch to a different branch (or main timeline if None)
    #[wasm_bindgen]
    pub fn switch_branch(&mut self, branch_name: Option<String>) -> bool {
        if let Some(snapshot) = self.history.switch_branch(branch_name).cloned() {
            self.restore_from_snapshot(&snapshot);
            true
        } else {
            false
        }
    }
    
    /// Get a list of all available branches
    #[wasm_bindgen]
    pub fn get_branches(&self) -> Vec<String> {
        self.history.get_branches()
    }
    
    /// Get the current simulation snapshot
    #[wasm_bindgen]
    pub fn get_current_snapshot(&self) -> Option<SimulationSnapshot> {
        self.history.current_snapshot().cloned()
    }
    
    /// Get the state of a specific process
    #[wasm_bindgen]
    pub fn get_process_state(&self, id: u64) -> Option<ProcessSnapshot> {
        self.history.current_snapshot().and_then(|snapshot| {
            snapshot.processes.get(&id).cloned()
        })
    }
    
    /// Get the message history for sequence diagrams
    #[wasm_bindgen]
    pub fn get_message_history(&self) -> Vec<MessageSnapshot> {
        self.message_history.clone()
    }
    
    /// Get all blocks in the current state
    #[wasm_bindgen]
    pub fn get_all_blocks(&self) -> Vec<BlockSnapshot> {
        self.history.current_snapshot()
            .map(|snapshot| {
                snapshot.processes.values()
                    .flat_map(|p| p.blocks.clone())
                    .collect()
            })
            .unwrap_or_default()
    }
    
    /// Get all QCs in the current state
    #[wasm_bindgen]
    pub fn get_all_qcs(&self) -> Vec<QCSnapshot> {
        self.history.current_snapshot()
            .map(|snapshot| {
                snapshot.processes.values()
                    .flat_map(|p| p.qcs.clone())
                    .collect()
            })
            .unwrap_or_default()
    }
    
    /// Get all pending messages in the current state
    #[wasm_bindgen]
    pub fn get_pending_messages(&self) -> Vec<MessageSnapshot> {
        self.history.current_snapshot()
            .map(|snapshot| snapshot.pending_messages.clone())
            .unwrap_or_default()
    }
    
    /// Manually add a transaction to a specific process
    #[wasm_bindgen]
    pub fn add_transaction(&mut self, process_id: u64, transaction_data: Vec<u8>) -> bool {
        if let Some(process) = self.harness.processes.get_mut(&Identity(process_id)) {
            process.ready_transactions.push(Transaction::Opaque(transaction_data));
            true
        } else {
            false
        }
    }
    
    /// Set transaction generation policy for a process
    #[wasm_bindgen]
    pub fn set_tx_gen_policy(&mut self, process_id: u64, policy: TxGenPolicy) {
        self.harness.tx_gen_policy.insert(Identity(process_id), policy);
    }
    
    /// Inject a message into the simulation
    #[wasm_bindgen]
    pub fn inject_message(
        &mut self, 
        message_type: &str, 
        sender: u64, 
        recipient: Option<u64>,
        data: String
    ) -> bool {
        // Create a message based on the type and data
        let message = self.create_message_from_type(message_type, sender, data);
        
        if let Some(msg) = message {
            self.harness.enqueue_message(
                msg, 
                Identity(sender), 
                recipient.map(|id| Identity(id))
            );
            true
        } else {
            false
        }
    }
    
    // --- Private helper methods ---
    
    /// Capture state from the harness
    fn capture_state_from_harness(
        harness: &test_harness::MockHarness, 
        step_count: usize,
        events: Vec<SimulationEvent>
    ) -> SimulationSnapshot {
        // Create a unique ID for this snapshot
        let id = format!("snapshot_{}", harness.time);
        
        // Capture process states
        let mut processes = BTreeMap::new();
        for (process_id, process) in &harness.processes {
            processes.insert(process_id.0, Self::capture_process_state(process));
        }
        
        // Capture pending messages
        let pending_messages = harness.pending_messages
            .iter()
            .map(|(msg, sender, recipient)| {
                MessageSnapshot {
                    message_type: Self::get_message_type(msg),
                    data: Self::get_message_data(msg),
                    sender: sender.0,
                    recipient: recipient.as_ref().map(|r| r.0),
                }
            })
            .collect();
        
        SimulationSnapshot {
            id,
            time: harness.time,
            step_count,
            processes,
            pending_messages,
            events,
        }
    }
    
    /// Capture a process's state
    fn capture_process_state(process: &MorpheusProcess) -> ProcessSnapshot {
        // Capture blocks
        let blocks = process.blocks
            .iter()
            .map(|(key, block)| {
                BlockSnapshot {
                    id: Self::generate_block_id(key),
                    block_type: Self::block_type_to_string(&key.type_),
                    view: key.view.0,
                    height: key.height,
                    author: key.author.as_ref().map(|a| a.0),
                    slot: key.slot.0,
                    prev: block.data.prev
                        .iter()
                        .map(|qc| Self::generate_block_id(&qc.data.for_which))
                        .collect(),
                    one_qc: Self::generate_block_id(&block.data.one.data.for_which),
                    finalized: process.finalized.get(key).cloned().unwrap_or(false),
                }
            })
            .collect();
        
        // Capture QCs
        let qcs = process.qcs
            .iter()
            .map(|(vote_data, qc)| {
                QCSnapshot {
                    id: Self::generate_qc_id(vote_data),
                    z: vote_data.z,
                    for_block: Self::generate_block_id(&vote_data.for_which),
                    block_type: Self::block_type_to_string(&vote_data.for_which.type_),
                    view: vote_data.for_which.view.0,
                    is_tip: process.tips.contains(vote_data),
                }
            })
            .collect();
        
        // Capture tips
        let tips = process.tips
            .iter()
            .map(|vote_data| Self::generate_block_id(&vote_data.for_which))
            .collect();
        
        // Capture finalized blocks
        let finalized_blocks = process.finalized
            .iter()
            .filter(|(_, finalized)| **finalized)
            .map(|(key, _)| Self::generate_block_id(key))
            .collect();
        
        // Capture unfinalized blocks
        let unfinalized_blocks = process.unfinalized
            .keys()
            .map(|key| Self::generate_block_id(key))
            .collect();
        
        ProcessSnapshot {
            id: process.id.0,
            view: process.view_i.0,
            phase: match process.phase_i.get(&process.view_i) {
                Some(Phase::High) => "High".to_string(),
                Some(Phase::Low) => "Low".to_string(),
                None => "Unknown".to_string(),
            },
            slot_tr: process.slot_i_tr.0,
            slot_lead: process.slot_i_lead.0,
            blocks,
            qcs,
            tips,
            finalized_blocks,
            unfinalized_blocks,
        }
    }
    
    /// Restore simulation state from a snapshot
    fn restore_from_snapshot(&mut self, snapshot: &SimulationSnapshot) {
        // This is a complex operation that would require:
        // 1. Recreating all processes with their state
        // 2. Restoring the harness time
        // 3. Repopulating the message queue
        
        // For the web visualization, we might not need full restoration,
        // as we can just use the snapshot data for visualization while
        // maintaining the actual simulation state separately.
        
        // This would be a stub implementation that just updates the time
        self.harness.time = snapshot.time;
    }
    
    /// Generate a unique ID for a block
    fn generate_block_id(key: &BlockKey) -> String {
        match key.type_ {
            BlockType::Genesis => "genesis".to_string(),
            _ => {
                let author = key.author.as_ref().map(|a| a.0.to_string()).unwrap_or("0".to_string());
                format!(
                    "{}_v{}_s{}_h{}_p{}", 
                    Self::block_type_to_string(&key.type_),
                    key.view.0, 
                    key.slot.0, 
                    key.height,
                    author
                )
            }
        }
    }
    
    /// Generate a unique ID for a QC
    fn generate_qc_id(vote_data: &VoteData) -> String {
        format!(
            "qc_{}_{}",
            vote_data.z,
            Self::generate_block_id(&vote_data.for_which)
        )
    }
    
    /// Convert BlockType to string
    fn block_type_to_string(block_type: &BlockType) -> String {
        match block_type {
            BlockType::Genesis => "Genesis".to_string(),
            BlockType::Lead => "Leader".to_string(),
            BlockType::Tr => "Transaction".to_string(),
        }
    }
    
    /// Get the type of a message as a string
    fn get_message_type(message: &Message) -> String {
        match message {
            Message::Block(_) => "Block".to_string(),
            Message::NewVote(_) => "NewVote".to_string(),
            Message::QC(_) => "QC".to_string(),
            Message::EndView(_) => "EndView".to_string(),
            Message::EndViewCert(_) => "EndViewCert".to_string(),
            Message::StartView(_) => "StartView".to_string(),
        }
    }
    
    /// Get data from a message as a string
    fn get_message_data(message: &Message) -> String {
        match message {
            Message::Block(block) => Self::generate_block_id(&block.data.key),
            Message::NewVote(vote) => format!(
                "{}_{}",
                vote.data.z,
                Self::generate_block_id(&vote.data.for_which)
            ),
            Message::QC(qc) => Self::generate_qc_id(&qc.data),
            Message::EndView(view) => format!("v{}", view.data.0),
            Message::EndViewCert(cert) => format!("v{}_cert", cert.data.0),
            Message::StartView(start_view) => format!("start_v{}", start_view.data.view.0),
        }
    }
    
    /// Create a message from type and data
    fn create_message_from_type(
        &self,
        message_type: &str, 
        sender: u64, 
        data: String
    ) -> Option<Message> {
        match message_type {
            "EndView" => {
                let view = ViewNum(data.parse().ok()?);
                Some(Message::EndView(Arc::new(Signed {
                    data: view,
                    author: Identity(sender),
                    signature: Signature {},
                })))
            }
            // Other message types would be implemented similarly
            _ => None
        }
    }
    
    /// Parse block creation event data
    fn parse_block_created(data: &str) -> Option<(u64, String, String)> {
        // This would parse traces from tracing_setup::block_created
        // For a real implementation, we'd need to parse the actual format
        // of the tracing data
        let parts: Vec<&str> = data.split(',').collect();
        if parts.len() >= 3 {
            let process_id = parts[0].parse().ok()?;
            let block_type = parts[1].to_string();
            let block_id = parts[2].to_string();
            Some((process_id, block_type, block_id))
        } else {
            None
        }
    }
    
    /// Parse QC formation event data
    fn parse_qc_formed(data: &str) -> Option<(u64, u8, String)> {
        // Similar parsing for QC formed events
        let parts: Vec<&str> = data.split(',').collect();
        if parts.len() >= 3 {
            let process_id = parts[0].parse().ok()?;
            let z = parts[1].parse().ok()?;
            let qc_id = parts[2].to_string();
            Some((process_id, z, qc_id))
        } else {
            None
        }
    }
    
    /// Parse view change event data
    fn parse_view_change(data: &str) -> Option<(u64, i64, i64, String)> {
        // Similar parsing for view change events
        let parts: Vec<&str> = data.split(',').collect();
        if parts.len() >= 4 {
            let process_id = parts[0].parse().ok()?;
            let old_view = parts[1].parse().ok()?;
            let new_view = parts[2].parse().ok()?;
            let reason = parts[3].to_string();
            Some((process_id, old_view, new_view, reason))
        } else {
            None
        }
    }
    
    /// Parse message sent event data
    fn parse_message_sent(data: &str) -> Option<(u64, Option<u64>, String)> {
        // Similar parsing for message sent events
        let parts: Vec<&str> = data.split(',').collect();
        if parts.len() >= 3 {
            let sender = parts[0].parse().ok()?;
            let recipient = if parts[1] == "broadcast" {
                None
            } else {
                Some(parts[1].parse().ok()?)
            };
            let message_type = parts[2].to_string();
            Some((sender, recipient, message_type))
        } else {
            None
        }
    }
    
    /// Parse phase change event data
    fn parse_phase_change(data: &str) -> Option<(u64, i64, String, String)> {
        // Similar parsing for phase change events
        let parts: Vec<&str> = data.split(',').collect();
        if parts.len() >= 4 {
            let process_id = parts[0].parse().ok()?;
            let view = parts[1].parse().ok()?;
            let old_phase = parts[2].to_string();
            let new_phase = parts[3].to_string();
            Some((process_id, view, old_phase, new_phase))
        } else {
            None
        }
    }
    
    /// Parse block finalized event data
    fn parse_block_finalized(data: &str) -> Option<(u64, String)> {
        // Similar parsing for block finalized events
        let parts: Vec<&str> = data.split(',').collect();
        if parts.len() >= 2 {
            let process_id = parts[0].parse().ok()?;
            let block_id = parts[1].to_string();
            Some((process_id, block_id))
        } else {
            None
        }
    }
}

#[wasm_bindgen]
impl MorpheusWorld {
    /// Create a new simulation world with the specified number of processes
    #[wasm_bindgen(constructor)]
    pub fn wasm_new(node_count: usize, time_step: u128) -> Self {
        // Create processes with default parameters
        let mut processes = Vec::new();
        for i in 1..=node_count {
            processes.push(MorpheusProcess::new(Identity(i as u64), node_count, (node_count - 1) / 3));
        }
        
        // Default configuration
        let config = VisualizationConfig {
            max_history: 100,
            auto_capture: true,
            track_messages: true,
            track_events: true,
            focus_processes: None,
        };
        
        Self::new(processes, time_step, config)
    }
}