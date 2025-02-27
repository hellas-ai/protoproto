use std::collections::{BTreeMap, BTreeSet, HashMap};
use log::{debug, warn};
use muchin::automaton::{Dispatcher};
use muchin::callback;

use super::types::*;
use super::state::MorpheusState;
use super::actions::{VotingAction, NetworkAction, MorpheusAction, ViewChangeAction};

/// Vote State - Extracted from MorpheusState for vote-specific operations
#[derive(Debug)]
pub struct VoteState {
    /// Votes received by block hash and vote type
    pub votes: BTreeMap<(VoteType, Hash), Vec<Vote>>,
    
    /// QCs by block hash and vote type
    pub qcs: BTreeMap<(VoteType, Hash), QC>,
    
    /// Latest QC by (block_type, author, slot)
    pub latest_qcs: BTreeMap<(BlockType, ProcessId, Slot), QC>,
    
    /// Blocks that have been finalized (have a 2-QC)
    pub finalized_blocks: BTreeSet<Hash>,
    
    /// Records which blocks this process has voted for
    pub voted: BTreeSet<VotedKey>,
    
    /// QCs by the QC block hash for lookup
    pub qcs_by_block: BTreeMap<Hash, Vec<QC>>,
    
    /// Index mapping QC to blocks that contain this QC as their 1-QC
    pub blocks_by_1qc: BTreeMap<Hash, Vec<Hash>>,
    
    /// Greatest 1-QC seen
    pub greatest_1qc: Option<QC>,
}

impl VoteState {
    /// Initialize a new VoteState
    pub fn new() -> Self {
        Self {
            votes: BTreeMap::new(),
            qcs: BTreeMap::new(),
            latest_qcs: BTreeMap::new(),
            finalized_blocks: BTreeSet::new(),
            voted: BTreeSet::new(),
            qcs_by_block: BTreeMap::new(),
            blocks_by_1qc: BTreeMap::new(),
            greatest_1qc: None,
        }
    }
    
    /// Add a vote to the state
    /// Returns true if we have reached the quorum size for a QC
    pub fn add_vote(&mut self, vote: Vote, quorum_size: usize) -> bool {
        let key = (vote.vote_type, vote.block_hash.clone());
        
        // Add vote
        self.votes
            .entry(key.clone())
            .or_insert_with(Vec::new)
            .push(vote);
            
        // Check if we have reached the quorum size for a QC
        let votes = self.votes.get(&key).unwrap();
        
        votes.len() >= quorum_size
    }
    
    /// Add a QC to the state
    pub fn add_qc(&mut self, qc: QC) {
        // Skip if we already have this QC
        let qc_key = (qc.vote_type, qc.block_hash.clone());
        if self.qcs.contains_key(&qc_key) {
            return;
        }
        
        // Update the greatest 1-QC seen
        if qc.vote_type == VoteType::Vote1 {
            if let Some(ref current_greatest) = self.greatest_1qc {
                if qc.is_greater_than(current_greatest) {
                    self.greatest_1qc = Some(qc.clone());
                }
            } else {
                self.greatest_1qc = Some(qc.clone());
            }
        }
        
        // Store the QC
        self.qcs.insert(qc_key, qc.clone());
        
        // Add to qcs_by_block index
        self.qcs_by_block
            .entry(qc.block_hash.clone())
            .or_insert_with(Vec::new)
            .push(qc.clone());
        
        // Update latest QC for this (block_type, author, slot)
        let key = (qc.block_type, qc.author, qc.slot);
        if let Some(existing_qc) = self.latest_qcs.get(&key) {
            // Only update if this QC has a higher vote type
            let existing_vote_val = existing_qc.vote_type.value();
            let new_vote_val = qc.vote_type.value();
            
            if new_vote_val > existing_vote_val {
                self.latest_qcs.insert(key, qc.clone());
            }
        } else {
            self.latest_qcs.insert(key, qc.clone());
        }
        
        // Mark as finalized if it's a 2-QC
        if qc.vote_type == VoteType::Vote2 {
            self.finalized_blocks.insert(qc.block_hash.clone());
        }
    }
    
    /// Check if a process has voted for a block
    pub fn has_voted(&self, vote_type: VoteType, block_type: BlockType, slot: Slot, author: ProcessId) -> bool {
        let key = VotedKey { vote_type, block_type, slot, author };
        self.voted.contains(&key)
    }
    
    /// Mark that a process has voted for a block
    pub fn mark_voted(&mut self, vote_type: VoteType, block_type: BlockType, slot: Slot, author: ProcessId) {
        let key = VotedKey { vote_type, block_type, slot, author };
        self.voted.insert(key);
    }
    
    /// Get QC for a block if available
    pub fn get_qc(&self, vote_type: VoteType, block_hash: &Hash) -> Option<&QC> {
        self.qcs.get(&(vote_type, block_hash.clone()))
    }
    
    /// Get QC with highest vote type for a block
    pub fn get_highest_qc(&self, block_hash: &Hash) -> Option<QC> {
        // Check in order from highest to lowest vote type
        for vote_type in [VoteType::Vote2, VoteType::Vote1, VoteType::Vote0].iter() {
            if let Some(qc) = self.get_qc(*vote_type, block_hash) {
                return Some(qc.clone());
            }
        }
        None
    }
    
    /// Check if a block is finalized
    pub fn is_finalized(&self, block_hash: &Hash) -> bool {
        self.finalized_blocks.contains(block_hash)
    }
    
    /// Prune old state (from views earlier than min_view)
    pub fn prune_old_state(&mut self, min_view: View, blocks: &BTreeSet<Hash>) {
        // Only keep QCs for blocks that still exist after pruning
        self.qcs.retain(|(_, hash), _| blocks.contains(hash));
        
        // Prune votes for old blocks
        self.votes.retain(|(_, hash), _| blocks.contains(hash));
        
        // Prune latest_qcs
        self.latest_qcs.retain(|(_, _, _), qc| qc.view >= min_view);
        
        // Prune finalized_blocks
        self.finalized_blocks.retain(|hash| blocks.contains(hash));
        
        // Prune qcs_by_block
        self.qcs_by_block.retain(|hash, _| blocks.contains(hash));
        
        // Prune blocks_by_1qc
        self.blocks_by_1qc.retain(|_, hashes| {
            hashes.iter().any(|hash| blocks.contains(hash))
        });
        for (_, hashes) in self.blocks_by_1qc.iter_mut() {
            hashes.retain(|hash| blocks.contains(hash));
        }
        
        // Update greatest_1qc if needed
        if let Some(ref qc) = self.greatest_1qc {
            if qc.view < min_view {
                // Find the new greatest 1-QC
                self.greatest_1qc = self.qcs.iter()
                    .filter(|((vote_type, _), qc)| *vote_type == VoteType::Vote1 && qc.view >= min_view)
                    .map(|(_, qc)| qc.clone())
                    .max_by(|a, b| a.compare(b));
            }
        }
    }
}

/// Process a vote
pub fn process_vote(
    state: &mut MorpheusState,
    vote: Vote,
    dispatcher: &mut Dispatcher,
) {
    debug!(
        "Process {}: Processing vote type={:?} for block type={:?} view={} height={} author={} from {}",
        state.process_id,
        vote.vote_type,
        vote.block_type,
        vote.view,
        vote.height,
        vote.author,
        vote.signer
    );
    
    // Add vote to state, see if we have a quorum
    let quorum_size = state.num_processes - state.f;
    let have_quorum = state.vote_state.add_vote(vote.clone(), quorum_size);
    
    // Clone the block_hash once at the beginning
    let block_hash = vote.block_hash.clone();
    
    // Form a QC if we have enough votes
    if have_quorum {
        dispatcher.dispatch(MorpheusAction::Voting(VotingAction::FormQC {
            vote_type: vote.vote_type,
            block_hash: block_hash.clone(),
        }));
    }
    
    // For 1-votes of transaction blocks, check if we need to send 2-votes
    if vote.vote_type == VoteType::Vote1 &&
       vote.block_type == BlockType::Transaction &&
       have_quorum {
        
        let qc_key = (vote.vote_type, block_hash.clone());
        if let Some(_qc) = state.vote_state.qcs.get(&qc_key) {
            // Check if this QC's block is a single tip
            let is_single_tip = state.block_state.is_single_tip(&block_hash);
            
            if is_single_tip &&
               !state.vote_state.has_voted(VoteType::Vote2, BlockType::Transaction, vote.slot, vote.author) {
                
                // Check if there's no block with greater height
                let no_higher_blocks = !state.block_state.blocks.iter().any(|(_, b)| {
                    b.height.0 > vote.height.0
                });
                
                if no_higher_blocks {
                    // Send 2-vote
                    dispatcher.dispatch(MorpheusAction::Voting(VotingAction::SendVote {
                        vote_type: VoteType::Vote2,
                        block_type: vote.block_type,
                        view: vote.view,
                        height: vote.height,
                        author: vote.author,
                        slot: vote.slot,
                        block_hash: block_hash,
                    }));
                    
                    // Set phase to Low (1)
                    state.phase = ThroughputPhase::Low;
                }
            }
        }
    }
    
    // For 1-votes of leader blocks, check if we need to send 2-votes
    if vote.vote_type == VoteType::Vote1 &&
       vote.block_type == BlockType::Leader &&
       have_quorum &&
       state.phase == ThroughputPhase::High {
        
        // Check conditions for sending 2-votes (lines 52-54 in pseudocode)
        if !state.vote_state.has_voted(VoteType::Vote2, BlockType::Leader, vote.slot, vote.author) {
            // Send 2-vote
            dispatcher.dispatch(MorpheusAction::Voting(VotingAction::SendVote {
                vote_type: VoteType::Vote2,
                block_type: vote.block_type,
                view: vote.view,
                height: vote.height,
                author: vote.author,
                slot: vote.slot,
                block_hash: vote.block_hash.clone(),
            }));
        }
    }
}

/// Process a QC
pub fn process_qc(
    state: &mut MorpheusState,
    qc: QC,
    dispatcher: &mut Dispatcher,
) {
    debug!(
        "Process {}: Processing QC type={:?} for block type={:?} view={} height={} author={}",
        state.process_id,
        qc.vote_type,
        qc.block_type,
        qc.view,
        qc.height,
        qc.author
    );
    
    // Add QC to state
    state.vote_state.add_qc(qc.clone());
    
    // For own 0-QCs from high throughput phase, broadcast to all
    if qc.vote_type == VoteType::Vote0 &&
       qc.author == state.process_id {
        
        dispatcher.dispatch_effect(NetworkAction::BroadcastQC {
            qc: qc.clone(),
            on_success: callback!(|qc: QC| MorpheusAction::Voting(VotingAction::ProcessQC { qc })),
            on_error: callback!(|(view: View, error: String)| MorpheusAction::ViewChange(
                ViewChangeAction::SendEndView { view }
            )),
        });
    }
}

/// Form a QC from votes
pub fn form_qc(
    state: &mut MorpheusState,
    vote_type: VoteType,
    block_hash: Hash,
    dispatcher: &mut Dispatcher,
) {
    // Get the votes and extract all necessary information first
    let key = (vote_type, block_hash.clone());
    
    // Check if we have votes and extract all needed information
    let qc_option = {
        let votes = state.vote_state.votes.get(&key);
        if votes.is_none() || votes.as_ref().unwrap().is_empty() {
            None
        } else {
            let votes = votes.unwrap();
            let first_vote = &votes[0];
            
            // Create the QC
            Some(QC {
                vote_type,
                block_type: first_vote.block_type,
                view: first_vote.view,
                height: first_vote.height,
                author: first_vote.author,
                slot: first_vote.slot,
                block_hash: block_hash.clone(),
                signatures: ThresholdSignature(vec![]), // placeholder
            })
        }
    };
    
    // If we have a QC, add it to state and possibly broadcast
    if let Some(qc) = qc_option {
        // Store author and process_id before adding QC
        let author = qc.author;
        let process_id = state.process_id;
        
        // Add QC to state
        state.vote_state.add_qc(qc.clone());
        
        // For 0-QCs, broadcast to all if this is our block
        if vote_type == VoteType::Vote0 && author == process_id {
            dispatcher.dispatch_effect(NetworkAction::BroadcastQC {
                qc: qc.clone(),
                on_success: callback!(|qc: QC| MorpheusAction::Voting(VotingAction::ProcessQC { qc })),
                on_error: callback!(|(view: View, error: String)| MorpheusAction::ViewChange(
                    ViewChangeAction::SendEndView { view }
                )),
            });
        }
    }
}

/// Check if a block is eligible for voting
pub fn check_vote_eligibility(
    state: &mut MorpheusState,
    block: Block,
    block_hash: Hash,
    dispatcher: &mut Dispatcher,
) {
    // Always send 0-votes for valid blocks (as per lines 24-25 in the pseudocode)
    if !state.vote_state.has_voted(VoteType::Vote0, block.block_type, block.slot, block.author.unwrap_or(ProcessId(0))) {
        // Send 0-vote to block creator
        dispatcher.dispatch(MorpheusAction::Voting(VotingAction::SendVote {
            vote_type: VoteType::Vote0,
            block_type: block.block_type,
            view: block.view,
            height: block.height,
            author: block.author.unwrap_or(ProcessId(0)),
            slot: block.slot,
            block_hash: block_hash.clone(),
        }));
    }
    
    // For transaction blocks in low throughput phase:
    if block.block_type == BlockType::Transaction && 
       block.view == state.current_view {
        
        // Check conditions from lines 36-40 in the pseudocode
        let finalized_leader_exists = state.block_state.blocks.iter().any(|(hash, b)| {
            b.block_type == BlockType::Leader && 
            b.view == state.current_view && 
            state.vote_state.is_finalized(hash)
        });
        
        let no_unfinalized_leaders = !state.block_state.blocks.iter().any(|(hash, b)| {
            b.block_type == BlockType::Leader && 
            b.view == state.current_view && 
            !state.vote_state.is_finalized(hash)
        });
        
        if finalized_leader_exists && no_unfinalized_leaders {
            // Check if this is a single tip
            if state.block_state.is_single_tip(&block_hash) {
                debug!(
                    "Block {} is a single tip. Checking 1-QC conditions.",
                    block_hash
                );
                
                // Check if block's 1-QC is >= all 1-QCs we've seen
                if let Some(greatest_1qc) = &state.vote_state.greatest_1qc {
                    let is_1qc_valid = greatest_1qc.is_less_than_or_equal(&block.qc);
                    debug!(
                        "1-QC comparison: greatest={:?}, block's={:?}, valid={}",
                        greatest_1qc,
                        block.qc,
                        is_1qc_valid
                    );
                    
                    if !is_1qc_valid {
                        debug!("Not voting for block {} due to invalid 1-QC", block_hash);
                        return;
                    }
                }
                
                // Check if we've already voted
                if !state.vote_state.has_voted(VoteType::Vote1, BlockType::Transaction, block.slot, block.author.unwrap_or(ProcessId(0))) {
                    // Send 1-vote
                    dispatcher.dispatch(MorpheusAction::Voting(VotingAction::SendVote {
                        vote_type: VoteType::Vote1,
                        block_type: block.block_type,
                        view: block.view,
                        height: block.height,
                        author: block.author.unwrap_or(ProcessId(0)),
                        slot: block.slot,
                        block_hash: block_hash.clone(),
                    }));
                    
                    // Set phase to Low
                    state.phase = ThroughputPhase::Low;
                }
            }
        }
    }
    
    // For leader blocks:
    if block.block_type == BlockType::Leader && 
       block.view == state.current_view && 
       state.phase == ThroughputPhase::High {
        
        // Send 1-vote for leader block (lines 49-51 in pseudocode)
        if !state.vote_state.has_voted(VoteType::Vote1, BlockType::Leader, block.slot, block.author.unwrap_or(ProcessId(0))) {
            dispatcher.dispatch(MorpheusAction::Voting(VotingAction::SendVote {
                vote_type: VoteType::Vote1,
                block_type: block.block_type,
                view: block.view,
                height: block.height,
                author: block.author.unwrap_or(ProcessId(0)),
                slot: block.slot,
                block_hash: block_hash.clone(),
            }));
        }
    }
}

/// Send a vote
pub fn send_vote(
    state: &mut MorpheusState,
    vote_type: VoteType,
    block_type: BlockType,
    view: View,
    height: Height,
    author: ProcessId,
    slot: Slot,
    block_hash: Hash,
    dispatcher: &mut Dispatcher,
) {
    // Create the vote
    let vote = Vote {
        vote_type,
        block_type,
        view,
        height,
        author,
        slot,
        block_hash: block_hash.clone(),
        signer: state.process_id,
        signature: Signature(vec![]), // placeholder
    };
    
    // Mark as voted
    state.vote_state.mark_voted(vote_type, block_type, slot, author);
    
    // For 0-votes, send only to block creator
    if vote_type == VoteType::Vote0 {
        dispatcher.dispatch_effect(NetworkAction::SendVoteToProcess {
            vote: vote.clone(),
            recipient: author,
            on_success: callback!(|vote: Vote| MorpheusAction::Voting(VotingAction::ProcessVote { vote })),
            on_error: callback!(|(view: View, error: String)| MorpheusAction::ViewChange(
                ViewChangeAction::SendEndView { view }
            )),
        });
    } else {
        // For 1-votes and 2-votes, broadcast to all
        dispatcher.dispatch_effect(NetworkAction::BroadcastVote {
            vote: vote.clone(),
            on_success: callback!(|vote: Vote| MorpheusAction::Voting(VotingAction::ProcessVote { vote })),
            on_error: callback!(|(view: View, error: String)| MorpheusAction::ViewChange(
                ViewChangeAction::SendEndView { view }
            )),
        });
    }
}

/// Process a voting action
pub fn process_voting_action(
    state: &mut MorpheusState,
    action: VotingAction,
    dispatcher: &mut Dispatcher,
) {
    match action {
        VotingAction::ProcessVote { vote } => {
            process_vote(state, vote, dispatcher);
        },
        VotingAction::ProcessQC { qc } => {
            process_qc(state, qc, dispatcher);
        },
        VotingAction::FormQC { vote_type, block_hash } => {
            form_qc(state, vote_type, block_hash, dispatcher);
        },
        VotingAction::CheckVoteEligibility { block, block_hash } => {
            check_vote_eligibility(state, block, block_hash, dispatcher);
        },
        VotingAction::SendVote { vote_type, block_type, view, height, author, slot, block_hash } => {
            send_vote(state, vote_type, block_type, view, height, author, slot, block_hash, dispatcher);
        },
    }
}