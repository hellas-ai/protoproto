use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use crate::*;

#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
/// Tracks votes for a particular data type and helps form quorums
///
/// This is an implementation helper that tracks votes from different processes
/// and determines when a quorum (n-f votes) has been reached.
/// Used for implementing the collection of votes in the protocol.
pub struct QuorumTrack<T: Ord + CanonicalSerialize + CanonicalDeserialize + Valid + 'static> {
    /// Maps vote data to a map of (voter identity -> signed vote)
    /// Ensures we only count one vote per process and track when we reach a quorum
    //#[serde(with = "serde_json_any_key::any_key_map")]
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
    /// Records a new vote and returns the number of votes collected for this data
    ///
    /// This helps implement the quorum formation logic from the pseudocode:
    /// "A z-quorum for b is a set of n-f z-votes for b, each signed by a different process in Π"
    /// Returns Err(Duplicate) if this process has already voted for this data.
    pub fn record_vote(&mut self, vote: Arc<ThreshPartial<T>>) -> Result<usize, Duplicate> {
        let votes_now = self
            .votes
            .entry(vote.data.clone())
            .or_default();

        // Ensure each process only votes once (for safety)
        if votes_now.contains_key(&vote.author) {
            return Err(Duplicate);
        }

        // Record the vote and return the current count
        votes_now.insert(vote.author.clone(), vote);
        Ok(votes_now.len())
    }
}

/// Manages voting state and quorum tracking
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoteManager {
    /// Tracks votes for each VoteData to form quorums
    pub vote_tracker: QuorumTrack<VoteData>,

    /// Tracks which blocks this process has already voted for
    /// (voted_i(z,x,s,p_j) in pseudocode)
    pub voted: BTreeSet<(u8, BlockType, SlotNum, Identity)>,

    /// Tracks which 0-QCs have been sent to avoid duplicates
    pub zero_qcs_sent: BTreeSet<BlockKey>,

    /// Tracks end-view messages for view changes
    pub end_views: QuorumTrack<ViewNum>,

    /// Tracks pending votes organized by view
    pub pending_votes: BTreeMap<ViewNum, PendingVotes>,
}

impl Default for VoteManager {
    fn default() -> Self {
        Self::new()
    }
}

impl VoteManager {
    pub fn new() -> Self {
        Self {
            vote_tracker: QuorumTrack {
                votes: BTreeMap::new(),
            },
            voted: BTreeSet::new(),
            zero_qcs_sent: BTreeSet::new(),
            end_views: QuorumTrack {
                votes: BTreeMap::new(),
            },
            pending_votes: BTreeMap::new(),
        }
    }

    /// Check if we've already voted for a specific (z, type, slot, author) combination
    pub fn has_voted(&self, z: u8, block_type: BlockType, slot: SlotNum, author: Identity) -> bool {
        self.voted.contains(&(z, block_type, slot, author))
    }

    /// Record that we've voted for a specific (z, type, slot, author) combination
    pub fn record_vote_sent(
        &mut self,
        z: u8,
        block_type: BlockType,
        slot: SlotNum,
        author: Identity,
    ) {
        self.voted.insert((z, block_type, slot, author));
    }

    /// Check if we've already sent a 0-QC for this block
    pub fn has_sent_zero_qc(&self, block_key: &BlockKey) -> bool {
        self.zero_qcs_sent.contains(block_key)
    }

    /// Record that we've sent a 0-QC for this block
    pub fn record_zero_qc_sent(&mut self, block_key: BlockKey) {
        self.zero_qcs_sent.insert(block_key);
    }

    /// Get or create pending votes for a view
    pub fn get_pending_votes_mut(&mut self, view: ViewNum) -> &mut PendingVotes {
        self.pending_votes.entry(view).or_default()
    }

    /// Mark pending votes for a view as dirty (needs re-evaluation)
    pub fn mark_pending_votes_dirty(&mut self, view: ViewNum) {
        self.pending_votes.entry(view).or_default().dirty = true;
    }

    /// Track a block for potential voting
    pub fn track_block_for_voting(&mut self, block: &BlockKey) {
        let pending = self.pending_votes.entry(block.view).or_default();
        match block.type_ {
            BlockType::Lead => {
                pending.lead_1.insert(block.clone(), true);
                pending.dirty = true;
            }
            BlockType::Tr => {
                pending.tr_1.insert(block.clone(), true);
                pending.dirty = true;
            }
            BlockType::Genesis => {} // Don't track genesis blocks
        }
    }

    /// Track a QC for potential 2-voting
    pub fn track_qc_for_voting(&mut self, qc: &FinishedQC) {
        if qc.data.z == 1 {
            let pending = self
                .pending_votes
                .entry(qc.data.for_which.view)
                .or_default();
            pending.dirty = true;
            match qc.data.for_which.type_ {
                BlockType::Lead => pending.lead_2.insert(qc.data.for_which.clone(), true),
                BlockType::Tr => pending.tr_2.insert(qc.data.for_which.clone(), true),
                BlockType::Genesis => None,
            };
        }
    }
}
