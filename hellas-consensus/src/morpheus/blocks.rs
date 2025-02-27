use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::Duration;
use log::{debug, warn};
use muchin::automaton::Dispatcher;
use muchin::callback;

use super::types::*;
use super::state::MorpheusState;
use super::actions::{BlockAction, NetworkAction, MorpheusAction, VotingAction, ViewChangeAction};

/// Constants for block handling
pub const MAX_TIPS_PER_LEADER_BLOCK: usize = 100;

/// Block State - Extracted from MorpheusState for block-specific operations
#[derive(Debug)]
pub struct BlockState {
    /// All blocks indexed by their hash
    pub blocks: BTreeMap<Hash, Block>,
    
    /// Blocks by (block_type, view, height) for efficient lookups
    pub blocks_by_position: BTreeMap<(BlockType, View, Height), Vec<Hash>>,
    
    /// Blocks by (block_type, view, slot, author) for fast lookups
    pub block_index: BTreeMap<(BlockType, View, Slot, ProcessId), Hash>,
    
    /// Blocks authored by each process (for checking equivocation)
    pub blocks_by_author: BTreeMap<ProcessId, BTreeMap<(BlockType, Slot), Hash>>,
    
    /// Graph of block observations (block_hash -> set of blocks that observe it)
    pub observed_by: BTreeMap<Hash, BTreeSet<Hash>>,
    
    /// For each block, all blocks it observes (incrementally maintained)
    pub observes: BTreeMap<Hash, BTreeSet<Hash>>,
    
    /// All tip blocks (not observed by any other blocks)
    pub tips: BTreeSet<Hash>,
    
    /// Single tip if one exists (a tip that observes all other blocks)
    pub single_tip: Option<Hash>,
    
    /// When blocks were first seen
    pub block_seen_times: BTreeMap<Hash, std::time::Instant>,
    
    /// Maximum number of blocks allowed per time window (for DoS protection)
    pub max_blocks_per_window: usize,
    
    /// Rate limiter for blocks from each process
    pub blocks_per_process: BTreeMap<ProcessId, (std::time::Instant, usize)>,
    
    /// Rate limiting window
    pub rate_limit_window: Duration,
}

impl BlockState {
    /// Initialize a new BlockState
    pub fn new() -> Self {
        Self {
            blocks: BTreeMap::new(),
            blocks_by_position: BTreeMap::new(),
            block_index: BTreeMap::new(),
            blocks_by_author: BTreeMap::new(),
            observed_by: BTreeMap::new(),
            observes: BTreeMap::new(),
            tips: BTreeSet::new(),
            single_tip: None,
            block_seen_times: BTreeMap::new(),
            max_blocks_per_window: 100, // Default value
            blocks_per_process: BTreeMap::new(),
            rate_limit_window: Duration::from_secs(60), // Default to 1 minute
        }
    }
    
    /// Add a block to the state, updating all related indices
    pub fn add_block(&mut self, block: Block) -> Result<Hash, BlockError> {
        // Validate block
        if !self.is_valid(&block) {
            return Err(BlockError::InvalidBlock);
        }
        
        let hash = block.hash();
        
        // Skip if already present
        if self.blocks.contains_key(&hash) {
            return Err(BlockError::DuplicateBlock);
        }
        
        // Store block position
        let position = (block.block_type, block.view, block.height);
        self.blocks_by_position
            .entry(position)
            .or_insert_with(Vec::new)
            .push(hash.clone());
        
        // Store block in the index
        if let Some(author) = block.author {
            self.block_index.insert(
                (block.block_type, block.view, block.slot, author),
                hash.clone()
            );
        }
        
        // Store block author (for equivocation detection)
        if let Some(author) = block.author {
            self.blocks_by_author
                .entry(author)
                .or_insert_with(BTreeMap::new)
                .insert((block.block_type, block.slot), hash.clone());
        }
        
        // Add block to observes relation
        let mut observed = BTreeSet::new();
        
        // A block observes itself
        observed.insert(hash.clone());
        
        // A block observes all blocks that its parents observe
        for pointer in &block.prev {
            if !self.blocks.contains_key(&pointer.block_hash) {
                // Skip blocks with missing dependencies, should be caught by is_valid
                continue;
            }
            
            let parent_hash = &pointer.block_hash;
            
            if let Some(parent_observed) = self.observes.get(parent_hash) {
                observed.extend(parent_observed.iter().cloned());
            }
            
            // Add this block to observed_by for the parent
            self.observed_by
                .entry(parent_hash.clone())
                .or_insert_with(BTreeSet::new)
                .insert(hash.clone());
        }
        
        // Store the set of observed blocks
        self.observes.insert(hash.clone(), observed);
        
        // Update tips
        self.update_tips(&hash);
        
        // Store block
        self.blocks.insert(hash.clone(), block);
        
        // Record when this block was first seen
        self.block_seen_times.insert(hash.clone(), std::time::Instant::now());
        
        Ok(hash)
    }
    
    /// Update tips when a new block is added
    fn update_tips(&mut self, new_block_hash: &Hash) {
        // Remove any tips that this block observes
        if let Some(observed) = self.observes.get(new_block_hash) {
            for observed_hash in observed.iter() {
                if observed_hash != new_block_hash {
                    self.tips.remove(observed_hash);
                }
            }
        }
        
        // Add new block as a tip
        self.tips.insert(new_block_hash.clone());
        
        // Update single tip
        self.update_single_tip();
    }
    
    /// Update the single tip calculation
    fn update_single_tip(&mut self) {
        self.single_tip = None;
        
        // If there's only one tip, it's the single tip
        if self.tips.len() == 1 {
            self.single_tip = self.tips.iter().next().cloned();
            return;
        }
        
        // Check if any tip observes all blocks (not just other tips)
        for tip in &self.tips {
            if let Some(tip_observes) = self.observes.get(tip) {
                let mut observes_all_blocks = true;
                
                // A single tip must observe all other blocks in the state
                for (hash, _) in &self.blocks {
                    if hash != tip && !tip_observes.contains(hash) {
                        observes_all_blocks = false;
                        break;
                    }
                }
                
                if observes_all_blocks {
                    self.single_tip = Some(tip.clone());
                    return;
                }
            }
        }
    }
    
    /// Check if a block is a single tip
    pub fn is_single_tip(&self, block_hash: &Hash) -> bool {
        self.single_tip.as_ref() == Some(block_hash)
    }
    
    /// Get a block by its hash
    pub fn get_block_by_hash(&self, hash: &Hash) -> Option<&Block> {
        self.blocks.get(hash)
    }
    
    /// Validate if a block is well-formed and its references exist
    pub fn is_valid(&self, block: &Block) -> bool {
        match block.block_type {
            BlockType::Genesis => {
                block.height == Height(0) && 
                block.view == View(0) &&
                block.slot == Slot(0) &&
                block.prev.is_empty()
            },
            BlockType::Transaction => {
                // Must have an author
                if block.author.is_none() {
                    return false;
                }
                
                // Check that all referenced blocks exist
                for ptr in &block.prev {
                    if !self.blocks.contains_key(&ptr.block_hash) {
                        return false; // Referenced block doesn't exist
                    }
                }
                
                // If slot > 0, must point to previous transaction block by same author
                if block.slot.0 > 0 {
                    let has_prev_tx_block = block.prev.iter().any(|ptr| {
                        ptr.qc.block_type == BlockType::Transaction &&
                        ptr.qc.author == block.author.unwrap() &&
                        ptr.qc.slot.0 == block.slot.0 - 1
                    });
                    
                    if !has_prev_tx_block {
                        return false;
                    }
                }
                
                // All pointed-to blocks must have view <= this block's view
                if block.prev.iter().any(|ptr| ptr.qc.view.0 > block.view.0) {
                    return false;
                }
                
                // Height must be max of prev heights + 1
                let max_prev_height = block.prev.iter()
                    .map(|ptr| ptr.qc.height.0)
                    .max()
                    .unwrap_or(0);
                    
                if block.height.0 != max_prev_height + 1 {
                    return false;
                }
                
                true
            },
            BlockType::Leader => {
                // Must have an author
                if block.author.is_none() {
                    return false;
                }
                
                // Check that all referenced blocks exist
                for ptr in &block.prev {
                    if !self.blocks.contains_key(&ptr.block_hash) {
                        return false; // Referenced block doesn't exist
                    }
                }
                
                // Author must be the leader for this view
                let leader = ProcessId(block.view.0 as usize % 100); // Assuming n < 100 for simplicity
                if block.author.unwrap() != leader {
                    return false;
                }
                
                // All pointed-to blocks must have view <= this block's view
                if block.prev.iter().any(|ptr| ptr.qc.view.0 > block.view.0) {
                    return false;
                }
                
                // Height must be max of prev heights + 1
                let max_prev_height = block.prev.iter()
                    .map(|ptr| ptr.qc.height.0)
                    .max()
                    .unwrap_or(0);
                    
                if block.height.0 != max_prev_height + 1 {
                    return false;
                }
                
                // If slot > 0, must point to previous leader block by same author
                if block.slot.0 > 0 {
                    let has_prev_leader_block = block.prev.iter().any(|ptr| {
                        ptr.qc.block_type == BlockType::Leader &&
                        ptr.qc.author == block.author.unwrap() &&
                        ptr.qc.slot.0 == block.slot.0 - 1
                    });
                    
                    if !has_prev_leader_block {
                        return false;
                    }
                }
                
                // First leader block of view or new view:
                // - needs justification
                // - 1-QC must be >= all 1-QCs in justification
                let s = block.slot.0;
                let prev_leader_in_same_view = block.prev.iter().any(|ptr| {
                    ptr.qc.block_type == BlockType::Leader &&
                    ptr.qc.author == block.author.unwrap() &&
                    ptr.qc.view == block.view &&
                    ptr.qc.slot.0 == s - 1
                });
                
                if s == 0 || !prev_leader_in_same_view {
                    // Needs justification
                    if block.justification.is_none() {
                        return false;
                    }
                    
                    // Check 1-QC against justification
                    if let Some(just) = &block.justification {
                        for msg in just {
                            if msg.qc.vote_type == VoteType::Vote1 && 
                               !msg.qc.is_less_than_or_equal(&block.qc) {
                                return false;
                            }
                        }
                    }
                } else {
                    // Subsequent leader block in view
                    // 1-QC must be for previous leader block
                    let prev_leader_hash = block.prev.iter()
                        .find(|ptr| {
                            ptr.qc.block_type == BlockType::Leader &&
                            ptr.qc.author == block.author.unwrap() &&
                            ptr.qc.slot.0 == block.slot.0 - 1
                        })
                        .map(|ptr| &ptr.block_hash);
                        
                    if Some(&block.qc.block_hash) != prev_leader_hash {
                        return false;
                    }
                }
                
                true
            }
        }
    }
    
    /// Check if we should process a block (rate limiting)
    pub fn should_process_block(&mut self, block: &Block) -> bool {
        let author = match block.author {
            Some(id) => id,
            None => return true, // Genesis block
        };
        
        let now = std::time::Instant::now();
        let (last_time, count) = self.blocks_per_process
            .entry(author)
            .or_insert((now, 0));
        
        if now.duration_since(*last_time) > self.rate_limit_window {
            // Reset window
            *last_time = now;
            *count = 1;
            true
        } else if *count < self.max_blocks_per_window {
            // Increment count
            *count += 1;
            true
        } else {
            // Rate limit exceeded
            warn!("Rate limit exceeded for process {}", author);
            false
        }
    }
    
    /// Detect equivocation (Byzantine behavior)
    pub fn detect_equivocation(&self, block: &Block) -> bool {
        let author = match block.author {
            Some(id) => id,
            None => return false, // Genesis block
        };
        
        // Check if the author has produced conflicting blocks
        for (hash, other_block) in &self.blocks {
            if other_block.author == Some(author) &&
               other_block.block_type == block.block_type &&
               other_block.slot == block.slot &&
               other_block.view == block.view &&
               hash != &block.hash() {
                // Found equivocation!
                warn!(
                    "Detected equivocation by process {}: blocks {} and {}",
                    author,
                    hash,
                    block.hash()
                );
                return true;
            }
        }
        
        false
    }
    
    /// Prune old state (from views earlier than min_view)
    pub fn prune_old_state(&mut self, min_view: View) {
        // Only keep blocks from views >= min_view
        self.blocks.retain(|_, block| block.view >= min_view);
        
        // Prune other data structures
        self.blocks_by_position.retain(|(_, view, _), _| *view >= min_view);
        self.block_index.retain(|(_, view, _, _), _| *view >= min_view);
        
        // Update observed/observes relations
        let blocks_to_retain: BTreeSet<_> = self.blocks.keys().cloned().collect();
        
        self.observed_by.retain(|hash, _| blocks_to_retain.contains(hash));
        for (_, observed_set) in self.observed_by.iter_mut() {
            observed_set.retain(|hash| blocks_to_retain.contains(hash));
        }
        
        self.observes.retain(|hash, _| blocks_to_retain.contains(hash));
        for (_, observed_set) in self.observes.iter_mut() {
            observed_set.retain(|hash| blocks_to_retain.contains(hash));
        }
        
        // Update tips
        self.tips.retain(|hash| blocks_to_retain.contains(hash));
        if let Some(ref tip) = self.single_tip {
            if !blocks_to_retain.contains(tip) {
                self.single_tip = None;
            }
        }
        
        // Update block_seen_times
        self.block_seen_times.retain(|hash, _| blocks_to_retain.contains(hash));
    }
}

/// Process a block
pub fn process_block(
    state: &mut MorpheusState,
    block: Block,
    dispatcher: &mut Dispatcher,
) {
    let block_hash = block.hash();
    
    debug!(
        "Process {}: Processing block type={:?} view={} height={} author={:?}",
        state.process_id,
        block.block_type,
        block.view,
        block.height,
        block.author
    );
    
    // Rate limiting check
    if !state.block_state.should_process_block(&block) {
        debug!(
            "Process {}: Rate limit exceeded for block from {:?}",
            state.process_id,
            block.author
        );
        return;
    }
    
    // Check for equivocation
    if state.block_state.detect_equivocation(&block) {
        warn!(
            "Process {}: Detected equivocation in block from {:?}",
            state.process_id,
            block.author
        );
        // In a real implementation, we might want to penalize Byzantine behavior
        // For now, we'll still process the block
    }
    
    // Try to add the block to state
    match state.block_state.add_block(block.clone()) {
        Ok(hash) => {
            // Handle view change if block has higher view
            if block.view.0 > state.current_view.0 {
                dispatcher.dispatch(MorpheusAction::ViewChange(
                    ViewChangeAction::UpdateView { 
                        new_view: block.view 
                    }
                ));
            }
            
            // Vote for the block if eligible
            dispatcher.dispatch(MorpheusAction::Voting(
                VotingAction::CheckVoteEligibility { 
                    block: block.clone(),
                    block_hash: hash.clone()
                }
            ));
        },
        Err(BlockError::DuplicateBlock) => {
            // Already have this block, nothing to do
            debug!(
                "Process {}: Received duplicate block {:?}",
                state.process_id,
                block_hash
            );
        },
        Err(err) => {
            warn!(
                "Process {}: Failed to add block: {:?}",
                state.process_id,
                err
            );
        }
    }
}

/// Create a transaction block
pub fn create_transaction_block(state: &mut MorpheusState) -> Block {
    // MakeTrBlock_i procedure from pseudocode
    let mut block = Block {
        block_type: BlockType::Transaction,
        author: Some(state.process_id),
        view: state.current_view,
        slot: state.transaction_slot,
        height: Height(0), // Will set later
        transactions: std::mem::take(&mut state.pending_transactions),
        prev: Vec::new(),
        qc: state.vote_state.greatest_1qc.clone().unwrap_or_else(|| QC {
            vote_type: VoteType::Vote1,
            block_type: BlockType::Genesis,
            view: View(0),
            height: Height(0),
            author: ProcessId(0),
            slot: Slot(0),
            block_hash: Hash([0u8; 32]), // genesis hash
            signatures: ThresholdSignature(vec![]), // placeholder
        }),
        justification: None,
        signature: None, // Will set later
    };
    
    // Set prev pointers
    let s = state.transaction_slot.0;
    
    // Point to own previous transaction block if it exists
    let mut prev_pointers = Vec::new();
    
    if s > 0 {
        // Find QC for previous transaction block
        for ((block_type, author, slot), qc) in &state.vote_state.latest_qcs {
            if *block_type == BlockType::Transaction && 
               *author == state.process_id && 
               slot.0 == s - 1 {
                prev_pointers.push(BlockPointer {
                    block_hash: qc.block_hash.clone(),
                    qc: qc.clone(),
                });
                break;
            }
        }
    }
    
    // Point to single tip if one exists
    if let Some(single_tip) = state.block_state.single_tip.as_ref() {
        if let Some(highest_qc) = state.get_highest_qc(single_tip) {
            if !prev_pointers.iter().any(|ptr| ptr.block_hash == highest_qc.block_hash) {
                prev_pointers.push(BlockPointer {
                    block_hash: highest_qc.block_hash.clone(),
                    qc: highest_qc.clone(),
                });
            }
        }
    }
    
    // Set height to max of prev pointers + 1
    let max_height = prev_pointers.iter()
        .map(|ptr| ptr.qc.height.0)
        .max()
        .unwrap_or(0);
    
    block.height = Height(max_height + 1);
    block.prev = prev_pointers;
    
    // Set signature
    block.signature = Some(Signature(vec![])); // placeholder
    
    block
}

/// Create a leader block
pub fn create_leader_block(state: &mut MorpheusState) -> Option<Block> {
    // MakeLeaderBlock_i procedure from pseudocode
    let mut block = Block {
        block_type: BlockType::Leader,
        author: Some(state.process_id),
        view: state.current_view,
        slot: state.leader_slot,
        height: Height(0), // Will set later
        transactions: Vec::new(), // Leader blocks don't contain transactions
        prev: Vec::new(),
        qc: QC { // Will set later
            vote_type: VoteType::Vote1,
            block_type: BlockType::Genesis,
            view: View(0),
            height: Height(0),
            author: ProcessId(0),
            slot: Slot(0),
            block_hash: Hash([0u8; 32]),
            signatures: ThresholdSignature(vec![]),
        },
        justification: None,
        signature: None, // Will set later
    };
    
    // Set prev pointers to tips
    let mut prev_pointers = Vec::new();
    
    // Add QCs for tips (limited to prevent excessive block size)
    let tips = &state.block_state.tips;
    if tips.len() > MAX_TIPS_PER_LEADER_BLOCK {
        // Sort tips by height (descending) to include the most recent ones
        let mut sorted_tips: Vec<_> = tips.iter().collect();
        sorted_tips.sort_by(|a, b| {
            let block_a = &state.block_state.blocks[a];
            let block_b = &state.block_state.blocks[b];
            block_b.height.cmp(&block_a.height)
        });
        
        // Add the highest MAX_TIPS_PER_LEADER_BLOCK tips
        for tip_hash in sorted_tips.iter().take(MAX_TIPS_PER_LEADER_BLOCK) {
            if let Some(highest_qc) = state.get_highest_qc(tip_hash) {
                prev_pointers.push(BlockPointer {
                    block_hash: highest_qc.block_hash.clone(),
                    qc: highest_qc.clone(),
                });
            }
        }
    } else {
        // Add all tips if there aren't too many
        for tip_hash in tips {
            if let Some(highest_qc) = state.get_highest_qc(tip_hash) {
                prev_pointers.push(BlockPointer {
                    block_hash: highest_qc.block_hash.clone(),
                    qc: highest_qc.clone(),
                });
            }
        }
    }
    
    // Add QC for previous leader block if it exists
    let s = state.leader_slot.0;
    let v = state.current_view;
    
    if s > 0 {
        for ((block_type, author, slot), qc) in &state.vote_state.latest_qcs {
            if *block_type == BlockType::Leader && 
               *author == state.process_id && 
               slot.0 == s - 1 {
                
                if !prev_pointers.iter().any(|ptr| ptr.block_hash == qc.block_hash) {
                    prev_pointers.push(BlockPointer {
                        block_hash: qc.block_hash.clone(),
                        qc: qc.clone(),
                    });
                }
                break;
            }
        }
    }
    
    // Make sure all pointers reference actual blocks
    for pointer in &prev_pointers {
        if !state.block_state.blocks.contains_key(&pointer.block_hash) {
            // Skip this leader block creation as it references non-existent blocks
            return None;
        }
    }
    
    // Set height to max of prev pointers + 1
    let max_height = prev_pointers.iter()
        .map(|ptr| ptr.qc.height.0)
        .max()
        .unwrap_or(0);
    
    block.height = Height(max_height + 1);
    block.prev = prev_pointers;
    
    // Set 1-QC and justification
    let has_produced_leader_block = state.block_state.blocks_by_author
        .get(&state.process_id)
        .map(|blocks| {
            blocks.iter().any(|((block_type, slot), hash)| {
                *block_type == BlockType::Leader && 
                state.block_state.blocks.get(hash).unwrap().view == state.current_view
            })
        })
        .unwrap_or(false);
    
    if !has_produced_leader_block {
        // First leader block of the view
        // Set justification to view messages
        let view_messages = state.view_state.view_messages
            .get(&state.current_view)
            .cloned()
            .unwrap_or_default();
        
        // Need n-f view messages
        let quorum_size = state.num_processes - state.f;
        
        if view_messages.len() >= quorum_size {
            let justification: Vec<ViewMessage> = view_messages
                .iter()
                .take(quorum_size)
                .cloned()
                .collect();
            
            block.justification = Some(justification);
            
            // Set 1-QC to be greater than or equal to all 1-QCs in view messages
            let mut max_qc = state.vote_state.greatest_1qc.clone().unwrap_or_else(|| QC {
                vote_type: VoteType::Vote1,
                block_type: BlockType::Genesis,
                view: View(0),
                height: Height(0),
                author: ProcessId(0),
                slot: Slot(0),
                block_hash: Hash([0u8; 32]),
                signatures: ThresholdSignature(vec![]),
            });
            
            for message in &view_messages {
                if message.qc.vote_type == VoteType::Vote1 && 
                   message.qc.is_greater_than(&max_qc) {
                    max_qc = message.qc.clone();
                }
            }
            
            block.qc = max_qc;
        } else {
            // Not enough view messages
            return None;
        }
    } else {
        // Subsequent leader block in the view
        // Set 1-QC to QC for previous leader block
        for ((block_type, author, slot), qc) in &state.vote_state.latest_qcs {
            if *block_type == BlockType::Leader && 
               *author == state.process_id && 
               slot.0 == s - 1 && 
               qc.vote_type == VoteType::Vote1 {
                
                block.qc = qc.clone();
                block.justification = None;
                break;
            }
        }
    }
    
    // Set signature (in real implementation)
    block.signature = Some(Signature(vec![])); // placeholder
    
    Some(block)
}

/// Process a create transaction block action
pub fn handle_create_transaction_block(
    state: &mut MorpheusState,
    dispatcher: &mut Dispatcher,
) {
    // Reset payload ready flag
    state.payload_ready = false;
    
    // Create the block
    let block = create_transaction_block(state);
    
    // Broadcast the block
    dispatcher.dispatch_effect(NetworkAction::BroadcastBlock {
        block: block.clone(),
        on_success: callback!(|(block: Block, hash: Hash)| 
            MorpheusAction::Block(BlockAction::BlockCreated { block, hash })),
        on_error: callback!(|(view: View, error: String)| 
            MorpheusAction::ViewChange(ViewChangeAction::SendEndView { 
                view
            })),
    });
    
    // Increment transaction slot
    state.transaction_slot = Slot(state.transaction_slot.0 + 1);
}

/// Process a create leader block action
pub fn handle_create_leader_block(
    state: &mut MorpheusState,
    dispatcher: &mut Dispatcher,
) {
    // Create the block
    if let Some(block) = create_leader_block(state) {
        // Broadcast the block
        dispatcher.dispatch_effect(NetworkAction::BroadcastBlock {
            block: block.clone(),
            on_success: callback!(|(block: Block, hash: Hash)| 
                MorpheusAction::Block(BlockAction::BlockCreated { block, hash })),
            on_error: callback!(|(view: View, error: String)| 
                MorpheusAction::ViewChange(ViewChangeAction::SendEndView {
                    view
                })),
        });
        
        // Increment leader slot
        state.leader_slot = Slot(state.leader_slot.0 + 1);
    } else {
        warn!(
            "Process {}: Failed to create leader block, not enough view messages",
            state.process_id
        );
    }
}

/// Handle the block created action
pub fn handle_block_created(
    state: &mut MorpheusState,
    block: Block,
    hash: Hash,
    dispatcher: &mut Dispatcher,
) {
    // Add the block to state
    match state.block_state.add_block(block.clone()) {
        Ok(_) => {
            // Process our own block
            dispatcher.dispatch(MorpheusAction::Block(BlockAction::ProcessBlock { block }));
        },
        Err(err) => {
            warn!(
                "Process {}: Failed to add our own created block: {:?}",
                state.process_id,
                err
            );
        }
    }
}

/// Process a block action
pub fn process_block_action(
    state: &mut MorpheusState,
    action: BlockAction,
    dispatcher: &mut Dispatcher,
) {
    match action {
        BlockAction::ProcessBlock { block } => {
            process_block(state, block, dispatcher);
        },
        BlockAction::CreateTransactionBlock => {
            handle_create_transaction_block(state, dispatcher);
        },
        BlockAction::CreateLeaderBlock => {
            handle_create_leader_block(state, dispatcher);
        },
        BlockAction::BlockCreated { block, hash } => {
            handle_block_created(state, block, hash, dispatcher);
        },
    }
}