use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use ark_serialize::{CanonicalDeserialize, CanonicalSerialize, Valid};
use serde::{Deserialize, Serialize};

use crate::*;

/// Tracks votes for a particular data type and helps form quorums
///
/// This is an implementation helper that tracks votes from different processes
/// and determines when a quorum (n-f votes) has been reached.
/// Used for implementing the collection of votes in the protocol.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QuorumTrack<
    T: Ord
        + CanonicalSerialize
        + CanonicalDeserialize
        + Valid
        + Serialize
        + for<'d> Deserialize<'d>
        + 'static,
> {
    /// Maps vote data to a map of (voter identity -> signed vote)
    /// Ensures we only count one vote per process and track when we reach a quorum
    #[serde(with = "serde_json_any_key::any_key_map")]
    pub votes: BTreeMap<T, BTreeMap<Identity, Arc<ThreshPartial<T>>>>,
}

/// Error when attempting to record a duplicate vote from the same process
#[derive(Debug, Serialize, Deserialize)]
pub struct Duplicate;

impl<
    T: Ord
        + Clone
        + CanonicalSerialize
        + CanonicalDeserialize
        + Valid
        + Serialize
        + for<'d> Deserialize<'d>
        + 'static,
> QuorumTrack<T>
{
    /// Creates a new, empty QuorumTrack
    pub fn new() -> Self {
        QuorumTrack {
            votes: BTreeMap::new(),
        }
    }
    
    /// Records a new vote and returns the number of votes collected for this data
    ///
    /// This helps implement the quorum formation logic from the pseudocode:
    /// "A z-quorum for b is a set of n-f z-votes for b, each signed by a different process in Π"
    /// Returns Err(Duplicate) if this process has already voted for this data.
    pub fn record_vote(&mut self, vote: Arc<ThreshPartial<T>>) -> Result<usize, Duplicate> {
        let votes_now = self
            .votes
            .entry(vote.data.clone())
            .or_insert(BTreeMap::new());

        // Ensure each process only votes once (for safety)
        if votes_now.contains_key(&vote.author) {
            return Err(Duplicate);
        }

        // Record the vote and return the current count
        votes_now.insert(vote.author.clone(), vote);
        Ok(votes_now.len())
    }
}

/// Voting-related functionality for the Morpheus process
impl MorpheusProcess {
    /// Attempts to issue a vote for a block at a specified level
    /// 
    /// This implements the voting logic from the protocol:
    /// - Each process votes only once for a (z,x,s,p_j) combination
    /// - Votes are signed and broadcast to all processes or a specific target
    /// 
    /// Arguments:
    /// - z: The vote level (0, 1, or 2)
    /// - block: The block key being voted for
    /// - target: Optional target process to send the vote to (None = all processes)
    /// - to_send: Output parameter for messages to be sent
    /// 
    /// Returns true if a new vote was cast, false if the process has already voted for this block.
    pub fn try_vote(
        &mut self,
        z: u8,
        block: &BlockKey,
        target: Option<Identity>,
        to_send: &mut Vec<(Message, Option<Identity>)>,
    ) -> bool {
        tracing::debug!(target: "try_vote", z = z, block = ?block, target = ?target);
        let author = block.author.clone().expect("not voting for genesis block");

        // Check if we've already voted for this (z,x,s,p_j) combination
        if !self
            .voted_i
            .contains(&(z, block.type_, block.slot, author.clone()))
        {
            // Record that we're voting for this combination
            self.voted_i
                .insert((z, block.type_, block.slot, author.clone()));

            // Create and sign the vote
            let voted = Arc::new(ThreshPartial::from_data(
                VoteData {
                    z,
                    for_which: block.clone(),
                },
                &self.kb,
            ));
            
            // Send the vote to the target or broadcast it
            self.send_msg(to_send, (Message::NewVote(voted.clone()), target));
            true
        } else {
            false
        }
    }

    /// Records a new vote and potentially forms a quorum certificate
    /// 
    /// This implements the automatic creation of QCs from the pseudocode:
    /// "A z-quorum for b is a set of n-f z-votes for b, each signed by a different process in Π"
    /// When a quorum is reached, a QC is formed and broadcast as appropriate.
    pub fn record_vote(
        &mut self,
        vote_data: &Arc<ThreshPartial<VoteData>>,
        to_send: &mut Vec<(Message, Option<Identity>)>,
    ) -> bool {
        tracing::debug!(target: "record_vote", vote_data = ?vote_data.data);
        
        // Record the vote in our vote tracker
        match self.vote_tracker.record_vote(vote_data.clone()) {
            Ok(num_votes) => {
                // Check if we've reached a quorum (n-f votes)
                if num_votes == self.n - self.f {
                    // Extract all the votes for aggregation
                    let votes_now = self
                        .vote_tracker
                        .votes
                        .get(&vote_data.data)
                        .unwrap()
                        .values()
                        .map(|v| (v.author.0 as usize - 1, v.signature.clone()))
                        .collect::<Vec<_>>();
                    
                    // Create the aggregated signature for the QC
                    let agg = self.kb.hints_setup.aggregator();
                    let mut data = Vec::new();
                    vote_data.data.serialize_compressed(&mut data).unwrap();
                    let signed = hints::sign_aggregate(
                        &agg,
                        hints::F::from((self.f + 1) as u64),
                        &votes_now,
                        &data,
                    )
                    .unwrap();
                    
                    // Create the QC from the aggregated signature
                    let quorum_formed = Arc::new(ThreshSigned {
                        data: vote_data.data.clone(),
                        signature: signed,
                    });

                    // 0-QCs for our own blocks need to be broadcast to everyone
                    if vote_data.data.z == 0
                        && vote_data.data.for_which.author.as_ref() == Some(&self.id)
                        && !self.zero_qcs_sent.contains(&vote_data.data.for_which)
                    {
                        self.zero_qcs_sent.insert(vote_data.data.for_which.clone());
                        crate::tracing_setup::qc_formed(
                            &self.id,
                            vote_data.data.z,
                            &vote_data.data,
                        );
                        self.send_msg(to_send, (Message::QC(quorum_formed.clone()), None));
                    }
                    
                    // Record the QC in our state
                    self.record_qc(quorum_formed);
                }
                true
            }
            Err(Duplicate) => {
                tracing::error!(
                    target: "duplicate_vote",
                    vote_data = ?vote_data.data,
                    author = ?vote_data.author
                );
                false
            }
        }
    }

    /// Re-evaluate all pending votes based on current state
    /// 
    /// This function checks the eligibility of all pending votes and
    /// issues votes for blocks that have become eligible since the last check.
    pub fn reevaluate_pending_votes(&mut self, to_send: &mut Vec<(Message, Option<Identity>)>) {
        // Only process votes for the current view
        let current_view = self.view_i;

        let mut all_pending = std::mem::replace(&mut self.pending_votes, BTreeMap::new());

        let pending = all_pending.entry(current_view).or_default();
        if !pending.dirty {
            return;
        }

        // First check global conditions for the current view
        let contains_lead = self
            .index
            .contains_lead_by_view
            .get(&current_view)
            .copied()
            .unwrap_or(false);
        let unfinalized_lead_empty = self
            .index
            .unfinalized_lead_by_view
            .get(&current_view)
            .map_or(true, |set| set.is_empty());

        // Only process transaction block votes if we have leader blocks and no unfinalized leader blocks
        if contains_lead && unfinalized_lead_empty {
            // Process transaction block votes (1-votes and 2-votes)
            self.process_block_votes(
                1,
                &mut pending.tr_1,
                |this, block_key| this.is_eligible_for_tr_1_vote(block_key),
                Some("1-voted for a transaction block"),
                to_send,
            );

            self.process_block_votes(
                2,
                &mut pending.tr_2,
                |this, block_key| this.is_eligible_for_tr_2_vote(block_key),
                Some("2-voted for a transaction block"),
                to_send,
            );
        }

        // Process leader block votes if we're still in high throughput phase
        if self.phase_i.get(&current_view).unwrap_or(&Phase::High) == &Phase::High {
            self.process_block_votes(
                1,
                &mut pending.lead_1,
                |_, block_key| block_key.view == current_view,
                None,
                to_send,
            );

            self.process_block_votes(
                2,
                &mut pending.lead_2,
                |_, block_key| block_key.view == current_view,
                None,
                to_send,
            );
        }

        pending.dirty = false;
        self.pending_votes = all_pending;
    }

    /// Generic method to process pending votes for blocks
    ///
    /// This handles both transaction and leader blocks for both 1-votes and 2-votes
    fn process_block_votes<F>(
        &mut self,
        vote_level: u8,
        pending_votes: &mut BTreeMap<BlockKey, bool>,
        eligibility_check: F,
        phase_transition_reason: Option<&str>,
        to_send: &mut Vec<(Message, Option<Identity>)>,
    ) where
        F: Fn(&Self, &BlockKey) -> bool,
    {
        let mut processed_keys = Vec::new();

        for block_key in pending_votes.keys().cloned() {
            if eligibility_check(self, &block_key) {
                if self.try_vote(vote_level, &block_key, None, to_send) {
                    if block_key.type_ == BlockType::Tr && phase_transition_reason.is_some() {
                        // If we voted for a transaction block, transition to low throughput phase
                        crate::tracing_setup::protocol_transition(
                            &self.id,
                            "throughput phase",
                            &Phase::High,
                            &Phase::Low,
                            phase_transition_reason,
                        );
                        self.set_phase(Phase::Low);
                    }
                    processed_keys.push(block_key);
                } else {
                    panic!(
                        "Already {}-voted {:?}, pending votes desync bug",
                        vote_level, block_key
                    );
                }
            }
        }

        pending_votes.retain(|key, _| !processed_keys.contains(&key));
    }

    /// Determines if a transaction block is eligible for a 1-vote
    /// 
    /// Transaction blocks are eligible for 1-votes when:
    /// 1. The block is the single tip in the DAG
    /// 2. The block's one-QC is greater than or equal to all other 1-QCs we've seen
    pub(crate) fn is_eligible_for_tr_1_vote(&self, block_key: &BlockKey) -> bool {
        let has_single_tip = self.block_is_single_tip(block_key);

        if !has_single_tip || !self.index.blocks.contains_key(block_key) {
            return false;
        }

        let block = self.index.blocks.get(block_key).unwrap();
        self.index
            .all_1qc
            .iter()
            .all(|qc| block.data.one.data.compare_qc(&qc.data) != std::cmp::Ordering::Less)
    }

    /// Determines if a transaction block is eligible for a 2-vote
    /// 
    /// Transaction blocks are eligible for 2-votes when:
    /// 1. The block has a 1-QC that is a single tip in the DAG
    /// 2. There are no higher blocks in the DAG
    pub(crate) fn is_eligible_for_tr_2_vote(&self, block_key: &BlockKey) -> bool {
        let has_single_tip = self.index.tips.len() == 1
            && self
                .index
                .tips
                .get(0)
                .map_or(false, |tip| tip.z == 1 && tip.for_which.eq(block_key));

        let no_higher_blocks = self.index.max_height.0 <= block_key.height;

        has_single_tip && no_higher_blocks
    }

    /// Checks if a block is the single tip of the DAG
    /// 
    /// A block is a single tip if:
    /// 1. There is exactly one tip in the DAG
    /// 2. The block points to that tip
    /// 3. No other blocks point to that tip
    fn block_is_single_tip(&self, block_key: &BlockKey) -> bool {
        if self.index.tips.len() != 1 {
            return false;
        }
        match self.index.tips.get(0) {
            Some(tip) => self
                .index
                .block_pointed_by
                .get(&tip.for_which)
                .map_or(false, |parents| {
                    parents.len() == 1 && parents.first().unwrap() == block_key
                }),
            None => false,
        }
    }
}