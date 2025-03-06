use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use crate::*;
use serde::{Deserialize, Serialize};
use time::UtcDateTime;

/// MorpheusProcess represents a single process (p_i) in the Morpheus protocol
///
/// This struct implements the Algorithm 1 from the Morpheus pseudocode,
/// maintaining all state required for processing messages, voting, and
/// producing blocks according to the protocol specification.
#[derive(Serialize, Deserialize)]
pub struct MorpheusProcess {
    /// Identity of this process (equivalent to p_i in the pseudocode)
    pub id: Identity,

    // === Core protocol variables from pseudocode ===
    /// Current view number (view_i in pseudocode)
    /// "Initially 0, represents the present view"
    pub view_i: ViewNum,

    /// Current slot for leader blocks (slot_i(lead) in pseudocode)
    /// "Initially 0, represents present slot" for leader blocks
    pub slot_i_lead: SlotNum,

    /// Current slot for transaction blocks (slot_i(Tr) in pseudocode)
    /// "Initially 0, represents present slot" for transaction blocks
    pub slot_i_tr: SlotNum,

    /// Tracks which blocks this process has already voted for (voted_i(z,x,s,p_j) in pseudocode)
    /// "Initially 0" for all combinations of z, x, s, p_j
    /// Used to ensure process votes only once for each (z,x,s,p_j) combination
    pub voted_i: BTreeSet<(u8, BlockType, SlotNum, Identity)>,

    /// Tracks the phase within each view (phase_i(v) in pseudocode)
    /// "Initially 0" for each view, represents high throughput (0) or low throughput (1) phase
    pub phase_i: BTreeMap<ViewNum, Phase>,

    /// Total number of processes in the system
    pub n: usize,

    /// Maximum number of faulty processes tolerated (n-f is the quorum size)
    pub f: usize,

    /// Network delay parameter (Δ in pseudocode)
    /// Used for timeouts in the protocol (6Δ and 12Δ)
    pub delta: u128,

    // === Implementation-specific auxiliary variables ===
    /// Tracks end-view messages for view changes
    /// Used to form (v+1)-certificates when f+1 end-view v messages are collected
    pub end_views: VoteTrack<ViewNum>,

    /// Tracks which 0-QCs have been sent to avoid duplicates
    /// Implements "p_i has not previously sent a 0-QC for b to other processors"
    pub zero_qcs_sent: BTreeSet<BlockKey>,

    /// Tracks which QCs we've already complained about to the leader
    /// Implements "Send q to lead(view_i) if not previously sent"
    pub complained_qcs: BTreeSet<VoteData>,

    /// Time when this process entered the current view
    /// Used for timeout calculations (6Δ and 12Δ since entering view)
    pub view_entry_time: u128,

    /// Current logical time
    pub current_time: u128,

    // === State tracking fields (corresponding to M_i and Q_i in pseudocode) ===
    /// Tracks votes for each VoteData to form quorums
    /// Part of M_i in pseudocode - "the set of all received messages"
    pub vote_tracker: VoteTrack<VoteData>,

    /// Tracks view change messages
    /// Used to collect view v messages with 1-QCs sent to the leader
    pub start_views: BTreeMap<ViewNum, Vec<Signed<StartView>>>,

    /// Stores QCs indexed by their VoteData
    /// Part of Q_i in pseudocode - "stores at most one z-QC for each block"
    pub qcs: BTreeMap<VoteData, ThreshSigned<VoteData>>,

    /// Tracks the maximum view seen and its associated VoteData
    pub max_view: (ViewNum, VoteData),

    /// Tracks the maximum height block seen and its key
    /// Used for identifying the tallest block in the DAG
    pub max_height: (usize, BlockKey),

    /// Stores the maximum 1-QC seen by this process
    /// Used when entering a new view: "Send (v, q') signed by p_i to lead(v),
    /// where q' is a maximal amongst 1-QCs seen by p_i"
    pub max_1qc: ThreshSigned<VoteData>,

    /// Stores the current tips of the block DAG
    /// "The tips of Q_i are those q ∈ Q_i such that there does not exist q' ∈ Q_i with q' ≻ q"
    pub tips: Vec<VoteData>,

    /// Maps block keys to blocks (part of M_i in pseudocode)
    /// Implements part of "the set of all received messages"
    pub blocks: BTreeMap<BlockKey, Signed<Arc<Block>>>,

    /// Tracks which blocks point to which other blocks
    /// Used to efficiently determine the DAG structure
    pub block_pointed_by: BTreeMap<BlockKey, BTreeSet<BlockKey>>,

    /// Tracks unfinalized blocks with 2-QC
    /// Used to identify blocks that have 2-QC but are not yet finalized
    pub unfinalized_2qc: BTreeSet<VoteData>,

    /// Maps block keys to their finalization status
    /// Used to track which blocks have been finalized
    pub finalized: BTreeMap<BlockKey, bool>,

    /// Maps block keys to their unfinalized QCs
    /// Used to track which QCs are not yet finalized
    pub unfinalized: BTreeMap<BlockKey, BTreeSet<VoteData>>,

    /// Tracks whether we've seen a leader block for each view
    /// Used to implement logic that depends on leader blocks within a view
    pub contains_lead_by_view: BTreeMap<ViewNum, bool>,

    /// Maps views to sets of unfinalized leader blocks
    /// Tracks which leader blocks are not yet finalized by view
    pub unfinalized_lead_by_view: BTreeMap<ViewNum, BTreeSet<BlockKey>>,

    // === Performance optimization indexes ===
    /// Quick index to QCs by block type, author, and slot
    /// Enables O(1) lookup of QCs
    pub qc_index: BTreeMap<(BlockType, Identity, SlotNum), ThreshSigned<VoteData>>,

    /// Maps (type, author, view) to QCs for efficient retrieval
    /// Used to find QCs for a specific block type, author, and view
    pub qc_by_view: BTreeMap<(BlockType, Identity, ViewNum), Vec<ThreshSigned<VoteData>>>,

    /// Maps (type, view, author) to blocks for efficient retrieval
    /// Used to find blocks of a specific type, view, and author
    pub block_index: BTreeMap<(BlockType, ViewNum, Identity), Vec<Signed<Arc<Block>>>>,

    /// Tracks whether we've produced a leader block in each view
    /// Used for leader logic to avoid producing multiple leader blocks in same view
    pub produced_lead_in_view: BTreeMap<ViewNum, bool>,
}

#[derive(Serialize, Deserialize)]
/// Tracks votes for a particular data type and helps form quorums
///
/// This is an implementation helper that tracks votes from different processes
/// and determines when a quorum (n-f votes) has been reached.
/// Used for implementing the collection of votes in the protocol.
pub struct VoteTrack<T: Ord> {
    /// Maps vote data to a map of (voter identity -> signed vote)
    /// Ensures we only count one vote per process and track when we reach a quorum
    pub votes: BTreeMap<T, BTreeMap<Identity, Signed<T>>>,
}

/// Error when attempting to record a duplicate vote from the same process
pub struct Duplicate;

impl<T: Ord + Clone> VoteTrack<T> {
    /// Records a new vote and returns the number of votes collected for this data
    ///
    /// This helps implement the quorum formation logic from the pseudocode:
    /// "A z-quorum for b is a set of n-f z-votes for b, each signed by a different process in Π"
    /// Returns Err(Duplicate) if this process has already voted for this data.
    pub fn record_vote(&mut self, vote: Signed<T>) -> Result<usize, Duplicate> {
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

impl MorpheusProcess {
    pub fn new(id: Identity, n: usize, f: usize) -> Self {
        // Create genesis block and its 1-QC
        let genesis_block = Signed {
            data: Arc::new(Block {
                key: GEN_BLOCK_KEY,
                prev: Vec::new(),
                one: ThreshSigned {
                    data: VoteData {
                        z: 1,
                        for_which: GEN_BLOCK_KEY,
                    },
                    signature: ThreshSignature {},
                },
                data: BlockData::Genesis,
            }),
            author: Identity(0),
            signature: Signature {},
        };

        let genesis_qc = ThreshSigned {
            data: VoteData {
                z: 1,
                for_which: GEN_BLOCK_KEY,
            },
            signature: ThreshSignature {},
        };

        // Initialize with a recommended default timeout
        MorpheusProcess {
            id,
            view_i: ViewNum(0),
            slot_i_lead: SlotNum(0),
            slot_i_tr: SlotNum(0),
            voted_i: BTreeSet::new(),
            phase_i: {
                let mut map = BTreeMap::new();
                map.insert(ViewNum(0), Phase::High);
                map
            },
            n,
            f,
            delta: 10, // 10 ... "units"

            // Auxiliary fields
            end_views: VoteTrack {
                votes: BTreeMap::new(),
            },
            zero_qcs_sent: BTreeSet::new(),
            complained_qcs: BTreeSet::new(),
            view_entry_time: 0,
            current_time: 0,

            vote_tracker: VoteTrack {
                votes: BTreeMap::new(),
            },
            start_views: BTreeMap::new(),
            qcs: {
                let mut map = BTreeMap::new();
                map.insert(genesis_qc.data.clone(), genesis_qc.clone());
                map
            },
            max_view: (ViewNum(0), genesis_qc.data.clone()),
            max_height: (0, GEN_BLOCK_KEY),
            max_1qc: genesis_qc.clone(),
            tips: vec![genesis_qc.data.clone()],
            blocks: {
                let mut map = BTreeMap::new();
                map.insert(GEN_BLOCK_KEY, genesis_block.clone());
                map
            },
            block_pointed_by: BTreeMap::new(),
            unfinalized_2qc: BTreeSet::new(),
            finalized: {
                let mut map = BTreeMap::new();
                map.insert(GEN_BLOCK_KEY, true);
                map
            },
            unfinalized: BTreeMap::new(),
            contains_lead_by_view: BTreeMap::new(),
            unfinalized_lead_by_view: BTreeMap::new(),

            qc_index: BTreeMap::new(),
            qc_by_view: BTreeMap::new(),
            block_index: {
                let mut map = BTreeMap::new();
                map.insert(
                    (BlockType::Genesis, ViewNum(0), Identity(0)),
                    vec![genesis_block],
                );
                map
            },
            produced_lead_in_view: {
                let mut map = BTreeMap::new();
                map.insert(ViewNum(0), false);
                map
            },
        }
    }

    pub fn set_now(&mut self, now: u128) {
        self.current_time = now;
    }

    pub fn verify_leader(&self, author: Identity, view: ViewNum) -> bool {
        author.0 as usize == (view.0 as usize % self.n)
    }

    pub fn lead(&self, view: ViewNum) -> Identity {
        Identity(view.0 as u64 % self.n as u64)
    }

    pub fn block_valid(&self, block: &Signed<Arc<Block>>) -> bool {
        if !block.is_valid() {
            return false;
        }
        let block = &block.data;
        let author = if let BlockType::Genesis = block.key.type_ {
            return block.key == GEN_BLOCK_KEY && block.prev.is_empty();
        } else {
            if let Some(auth) = block.key.author.clone() {
                auth
            } else {
                return false;
            }
        };

        if block.prev.is_empty() {
            return false;
        }

        for prev in &block.prev {
            if prev.data.for_which.view > block.key.view
                || prev.data.for_which.height >= block.key.height
            {
                return false;
            }
        }

        if block.one.data.z != 1 || block.one.data.for_which.height >= block.key.height {
            return false;
        }

        match block.prev.iter().max_by_key(|qc| qc.data.for_which.height) {
            None => (),
            Some(qc_max_height) => {
                if block.key.height != qc_max_height.data.for_which.height + 1 {
                    return false;
                }
            }
        }

        match &block.data {
            BlockData::Genesis => {
                if block.key.type_ != BlockType::Genesis {
                    return false;
                }
            }
            BlockData::Tr { transactions } => {
                if block.key.type_ != BlockType::Tr {
                    return false;
                }
                if block.key.slot > SlotNum(0) {
                    if !block.prev.iter().any(|qc| {
                        qc.data.for_which.type_ == BlockType::Tr
                            && qc.data.for_which.author == Some(author.clone())
                            && qc.data.for_which.slot.is_pred(block.key.slot)
                    }) {
                        return false;
                    }
                }
                if transactions.len() == 0 {
                    return false;
                }
            }
            BlockData::Lead { justification } => {
                if block.key.type_ != BlockType::Lead {
                    return false;
                }
                if !self.verify_leader(block.key.author.clone().unwrap(), block.key.view) {
                    return false;
                }
                let prev_leader_for: Vec<&ThreshSigned<VoteData>> = block
                    .prev
                    .iter()
                    .filter(|qc| {
                        qc.data.for_which.type_ == BlockType::Lead
                            && qc.data.for_which.author == Some(author.clone())
                            && qc.data.for_which.slot.is_pred(block.key.slot)
                    })
                    .collect();

                if block.key.slot > SlotNum(0) {
                    if prev_leader_for.len() != 1 {
                        return false;
                    }

                    if prev_leader_for[0].data.for_which.view == block.key.view {
                        if block.one.data.for_which != prev_leader_for[0].data.for_which {
                            return false;
                        }
                    }
                }

                if block.key.slot == SlotNum(0)
                    || prev_leader_for[0].data.for_which.view < block.key.view
                {
                    let mut just: Vec<Signed<StartView>> = justification.clone();
                    just.sort_by(|m1, m2| m1.author.cmp(&m2.author));

                    if just.len() != self.n - self.f {
                        return false;
                    }
                    if !just.iter().all(|j| j.is_valid()) {
                        return false;
                    }
                    if !just
                        .iter()
                        .all(|j| block.one.data.compare_qc(&j.data.qc.data) != Ordering::Less)
                    {
                        return false;
                    }
                }
            }
        }

        true
    }
    pub fn process_message(
        &mut self,
        message: Message,
        to_send: &mut Vec<(Message, Option<Identity>)>,
    ) -> bool {
        match message {
            Message::Block(block) => {
                if !self.block_valid(&block) {
                    return false;
                }
                if !self.voted_i.contains(&(
                    0,
                    block.data.key.type_,
                    block.data.key.slot,
                    block.data.key.author.clone().expect("validated"),
                )) {
                    self.voted_i.insert((
                        0,
                        block.data.key.type_,
                        block.data.key.slot,
                        block.data.key.author.clone().expect("validated"),
                    ));
                    to_send.push((
                        Message::NewVote(Signed {
                            data: VoteData {
                                z: 0,
                                for_which: block.data.key.clone(),
                            },
                            author: self.id.clone(),
                            signature: Signature {},
                        }),
                        Some(block.data.key.author.clone().expect("validated")),
                    ));
                }
                self.record_block(block.clone(), to_send);
                if self.phase_i.entry(self.view_i).or_insert(Phase::High) == &Phase::High {
                    // If ∃𝑏 ∈𝑀𝑖 with 𝑏.type= lead, 𝑏.view= view𝑖 , voted𝑖 (1,lead,𝑏.slot,𝑏.auth)= 0 then:
                    if block.data.key.type_ == BlockType::Lead
                        && block.data.key.view == self.view_i
                        && !self.voted_i.contains(&(
                            1,
                            BlockType::Lead,
                            block.data.key.slot,
                            block.data.key.author.clone().expect("validated"),
                        ))
                    {
                        self.voted_i.insert((
                            1,
                            BlockType::Lead,
                            block.data.key.slot,
                            block.data.key.author.clone().expect("validated"),
                        ));
                        to_send.push((
                            Message::NewVote(Signed {
                                data: VoteData {
                                    z: 1,
                                    for_which: block.data.key.clone(),
                                },
                                author: self.id.clone(),
                                signature: Signature {},
                            }),
                            None,
                        ));
                    }
                }
                if block.data.key.type_ == BlockType::Tr
                    && block.data.key.view == self.view_i
                    && self
                        .contains_lead_by_view
                        .get(&self.view_i)
                        .cloned()
                        .unwrap_or(false)
                    && self
                        .unfinalized_lead_by_view
                        .entry(self.view_i)
                        .or_default()
                        .is_empty()
                    && self.tips.len() == 1
                    && self.tips[0].for_which == block.data.key
                    && self
                        .qcs
                        .values()
                        .filter(|qc| qc.data.z == 1)
                        .all(|qc| block.data.one.data.compare_qc(&qc.data) != Ordering::Less)
                    && !self.voted_i.contains(&(
                        1,
                        BlockType::Tr,
                        block.data.key.slot,
                        block.data.key.author.clone().expect("validated"),
                    ))
                {
                    self.phase_i.insert(self.view_i, Phase::Low);
                    self.voted_i.insert((
                        1,
                        BlockType::Tr,
                        block.data.key.slot,
                        block.data.key.author.clone().expect("validated"),
                    ));
                    to_send.push((
                        Message::NewVote(Signed {
                            data: VoteData {
                                z: 1,
                                for_which: block.data.key.clone(),
                            },
                            author: self.id.clone(),
                            signature: Signature {},
                        }),
                        None,
                    ));
                }
                if block.data.key.type_ == BlockType::Lead {
                    self.contains_lead_by_view.insert(block.data.key.view, true);
                    self.unfinalized_lead_by_view
                        .entry(block.data.key.view)
                        .or_default()
                        .insert(block.data.key.clone());
                }
            }
            Message::NewVote(vote_data) => {
                if !vote_data.is_valid() {
                    return false;
                }
                match self.vote_tracker.record_vote(vote_data.clone()) {
                    Ok(num_votes) => {
                        if num_votes == self.n - self.f {
                            let quorum_formed = ThreshSigned {
                                data: vote_data.data.clone(),
                                signature: ThreshSignature {},
                            };
                            if vote_data.data.z == 0
                                && vote_data.data.for_which.author.as_ref() == Some(&self.id)
                                && !self.zero_qcs_sent.contains(&vote_data.data.for_which)
                            {
                                self.zero_qcs_sent.insert(vote_data.data.for_which.clone());
                                to_send.push((Message::QC(quorum_formed.clone()), None));
                            }
                            self.record_qc(
                                ThreshSigned {
                                    data: vote_data.data,
                                    signature: ThreshSignature {},
                                },
                                to_send,
                            );
                        }
                    }
                    Err(Duplicate) => return false,
                }
            }
            Message::QC(qc) => {
                self.record_qc(qc, to_send);
                if self.max_view.0 > self.view_i {
                    self.end_view(
                        Message::QC(self.qcs.get(&self.max_view.1).cloned().unwrap()),
                        self.max_view.0,
                        to_send,
                    );
                }
            }
            Message::EndView(end_view) => {
                if !end_view.is_valid() {
                    return false;
                }
                match self.end_views.record_vote(end_view.clone()) {
                    Ok(num_votes) => {
                        if end_view.data > self.view_i && num_votes >= self.f + 1 {
                            to_send.push((
                                Message::EndViewCert(ThreshSigned {
                                    data: end_view.data,
                                    signature: ThreshSignature {},
                                }),
                                None,
                            ));
                        }
                    }
                    Err(Duplicate) => return false,
                }
            }
            Message::EndViewCert(end_view_cert) => {
                let view = end_view_cert.data;
                if view >= self.view_i {
                    self.end_view(Message::EndViewCert(end_view_cert), view, to_send);
                }
            }
            Message::StartView(start_view) => {
                if !start_view.is_valid() {
                    return false;
                }
                if start_view.data.qc.data.z != 1 {
                    return false;
                }
                self.record_qc(start_view.data.qc.clone(), to_send);
                self.start_views
                    .entry(start_view.data.view)
                    .or_insert(Vec::new())
                    .push(start_view);
            }
        }

        true
    }

    fn end_view(
        &mut self,
        cause: Message,
        new_view: ViewNum,
        to_send: &mut Vec<(Message, Option<Identity>)>,
    ) {
        self.view_i = new_view;
        self.view_entry_time = self.current_time;

        to_send.push((cause, None));

        // Send all tips we've created to the new leader
        // "Send all tips q' of Q_i such that q'.auth = p_i to lead(v)"
        for tip in &self.tips {
            if tip.for_which.author == Some(self.id.clone()) {
                to_send.push((
                    Message::QC(self.qcs.get(tip).unwrap().clone()),
                    Some(self.lead(new_view)),
                ));
            }
        }
        to_send.push((
            Message::StartView(Signed {
                data: StartView {
                    view: new_view,
                    qc: self.max_1qc.clone(),
                },
                author: self.id.clone(),
                signature: Signature {},
            }),
            Some(self.lead(new_view)),
        ));
    }

    /// Implements the "Complain" section from Algorithm 1
    ///
    /// Checks timeouts and sends complaints:
    /// "If ∃q ∈ Q_i which is maximal according to ⪰ amongst those that have not been finalized for
    ///  time 6Δ since entering view view_i: Send q to lead(view_i) if not previously sent;"
    /// "If ∃q ∈ Q_i which has not been finalized for time 12Δ since entering view view_i:
    ///  Send the end-view message (view_i) signed by p_i to all processes;"
    pub fn check_timeouts(&mut self, to_send: &mut Vec<(Message, Option<Identity>)>) {
        let time_in_view = self.current_time - self.view_entry_time;

        if time_in_view >= self.delta * 6 {
            let maximal_unfinalized =
                self.unfinalized
                    .iter()
                    .flat_map(|(_, qcs)| qcs)
                    .max_by(|&qc1, &qc2| {
                        // FIXME: when the paper says maximal, does it mean unique?
                        if self.observes(qc1.clone(), qc2) {
                            Ordering::Greater
                        } else if self.observes(qc2.clone(), qc1) {
                            Ordering::Less
                        } else {
                            Ordering::Equal
                        }
                    });

            if let Some(qc_data) = maximal_unfinalized {
                self.complained_qcs.insert(qc_data.clone());
                to_send.push((
                    Message::QC(self.qcs.get(&qc_data).cloned().unwrap()),
                    Some(self.lead(self.view_i)),
                ));
            }
        }

        // Second timeout - 12Δ, send end-view message
        if time_in_view >= self.delta * 12 && !self.unfinalized.is_empty() {
            to_send.push((
                Message::EndView(Signed {
                    data: self.view_i,
                    author: self.id.clone(),
                    signature: Signature {},
                }),
                None,
            ));
        }
    }

    /// Checks key protocol invariants and returns a list of invariant violations
    ///
    /// This method is intended for testing purposes to ensure protocol invariants
    /// are maintained throughout execution.
    pub fn check_invariants(&self) -> Vec<String> {
        let mut violations = Vec::new();

        // Check view and phase consistency
        if !self.phase_i.contains_key(&self.view_i) {
            violations.push(format!("Current view {} has no phase entry", self.view_i.0));
        }

        // Check time consistency
        if self.view_entry_time > self.current_time {
            violations.push(format!(
                "View entry time {} is after current time {}",
                self.view_entry_time, self.current_time
            ));
        }

        // Check block DAG consistency
        for (key, block) in &self.blocks {
            // Check that block key matches the block's actual key
            if &block.data.key != key {
                violations.push(format!(
                    "Block key mismatch: index key {:?} doesn't match block key {:?}",
                    key, block.data.key
                ));
            }

            // Check that each block is correctly indexed in block_pointed_by
            for qc in &block.data.prev {
                let pointed_block_key = &qc.data.for_which;
                if let Some(pointed_blocks) = self.block_pointed_by.get(pointed_block_key) {
                    if !pointed_blocks.contains(key) {
                        violations.push(format!(
                            "Block {:?} points to {:?} but not reflected in block_pointed_by",
                            key, pointed_block_key
                        ));
                    }
                } else {
                    violations.push(format!(
                        "Block {:?} points to {:?} which has no block_pointed_by entry",
                        key, pointed_block_key
                    ));
                }
            }
        }

        // Check block_pointed_by consistency
        for (key, pointing_blocks) in &self.block_pointed_by {
            // Verify the key exists in blocks
            if !self.blocks.contains_key(key) && *key != GEN_BLOCK_KEY {
                violations.push(format!(
                    "block_pointed_by contains key {:?} but no such block exists",
                    key
                ));
            }

            // Verify that each pointing block actually points to this block
            for pointing_key in pointing_blocks {
                if let Some(pointing_block) = self.blocks.get(pointing_key) {
                    let points_to_key = pointing_block
                        .data
                        .prev
                        .iter()
                        .any(|qc| &qc.data.for_which == key);

                    if !points_to_key {
                        violations.push(format!(
                            "Block {:?} in block_pointed_by for {:?} but doesn't actually point to it",
                            pointing_key, key
                        ));
                    }
                } else {
                    violations.push(format!(
                        "block_pointed_by for {:?} contains non-existent block {:?}",
                        key, pointing_key
                    ));
                }
            }
        }

        // Check QC consistency
        for (vote_data, qc) in &self.qcs {
            // Check that QC data matches index
            if &qc.data != vote_data {
                violations.push(format!(
                    "QC data mismatch: index data {:?} doesn't match QC data {:?}",
                    vote_data, qc.data
                ));
            }

            // Check that QC is correctly indexed in qc_index
            let index_key = (
                vote_data.for_which.type_,
                vote_data.for_which.author.clone().unwrap_or(Identity(0)),
                vote_data.for_which.slot,
            );
            if let Some(indexed_qc) = self.qc_index.get(&index_key) {
                if &indexed_qc.data != vote_data {
                    violations.push(format!(
                        "QC index mismatch: qc_index data {:?} doesn't match QC data {:?}",
                        indexed_qc.data, vote_data
                    ));
                }
            } else {
                violations.push(format!("QC {:?} not found in qc_index", vote_data));
            }

            // Check that QC is correctly indexed in qc_by_view
            let view_key = (
                vote_data.for_which.type_,
                vote_data.for_which.author.clone().unwrap_or(Identity(0)),
                vote_data.for_which.view,
            );
            if let Some(view_qcs) = self.qc_by_view.get(&view_key) {
                if !view_qcs.iter().any(|q| &q.data == vote_data) {
                    violations.push(format!(
                        "QC {:?} not found in qc_by_view for key {:?}",
                        vote_data, view_key
                    ));
                }
            } else {
                violations.push(format!("QC {:?} not found in qc_by_view", vote_data));
            }
        }

        // Check tips consistency
        for tip in &self.tips {
            // A tip should not have any blocks that observe it
            let tip_key = &tip.for_which;
            if let Some(pointing_blocks) = self.block_pointed_by.get(tip_key) {
                if !pointing_blocks.is_empty() {
                    violations.push(format!(
                        "Tip {:?} has blocks pointing to it: {:?}",
                        tip_key, pointing_blocks
                    ));
                }
            }
        }

        // Check max_height consistency
        let max_height = self.max_height.0;
        let max_height_key = &self.max_height.1;
        let actual_max_height = self.blocks.keys().map(|k| k.height).max().unwrap_or(0);

        if max_height != actual_max_height {
            violations.push(format!(
                "max_height ({}) does not match actual max height ({})",
                max_height, actual_max_height
            ));
        }

        if max_height > 0 && !self.blocks.contains_key(max_height_key) {
            violations.push(format!(
                "max_height_key {:?} does not exist in blocks",
                max_height_key
            ));
        }

        // Check finalization consistency
        for (key, is_finalized) in &self.finalized {
            if *is_finalized {
                // If finalized, it shouldn't be in unfinalized
                if self.unfinalized.contains_key(key) {
                    violations.push(format!(
                        "Block {:?} is marked as finalized but is also in unfinalized",
                        key
                    ));
                }
            }
        }
        // Check that self.finalized is consistent with self.observes.
        // Try recomputing finalized from scratch

        // Check unfinalized_2qc consistency
        for vote_data in &self.unfinalized_2qc {
            if vote_data.z != 2 {
                violations.push(format!(
                    "VoteData {:?} in unfinalized_2qc has z = {} instead of 2",
                    vote_data, vote_data.z
                ));
            }

            // Should be in qcs
            if !self.qcs.contains_key(vote_data) {
                violations.push(format!(
                    "VoteData {:?} in unfinalized_2qc not found in qcs",
                    vote_data
                ));
            }

            // The block should be unfinalized
            let block_key = &vote_data.for_which;
            if !self.unfinalized.contains_key(block_key) {
                violations.push(format!(
                    "Block {:?} for VoteData in unfinalized_2qc not found in unfinalized",
                    block_key
                ));
            }
        }

        // Check max_1qc is actually a 1-QC
        if self.max_1qc.data.z != 1 {
            violations.push(format!(
                "max_1qc has z = {} instead of 1",
                self.max_1qc.data.z
            ));
        }

        // Check view leader consistency
        let leader = self.lead(self.view_i);
        if !self.verify_leader(leader.clone(), self.view_i) {
            violations.push(format!(
                "Current leader {} for view {} fails verification",
                leader.0, self.view_i.0
            ));
        }

        violations
    }
}
