use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use crate::{
    debug_impls::{format_block_key, format_message, format_vote_data},
    *,
};
use serde::{Deserialize, Serialize};

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

    /// Stores all 1-QCs seen by this process
    pub all_1qc: BTreeSet<ThreshSigned<VoteData>>,

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

    /// All messages received by this process
    pub received_messages: BTreeSet<Message>,
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
        // Track process creation with tracing
        crate::tracing_setup::register_process(&id, n, f);

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
            author: Identity(u64::MAX),
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
            max_view: (ViewNum(-1), genesis_qc.data.clone()),
            max_height: (0, GEN_BLOCK_KEY),
            max_1qc: genesis_qc.clone(),
            all_1qc: BTreeSet::new(),
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

            qc_index: BTreeMap::from([(
                (BlockType::Genesis, Identity(u64::MAX), SlotNum(0)),
                genesis_qc.clone(),
            )]),
            qc_by_view: BTreeMap::from([(
                (BlockType::Genesis, Identity(u64::MAX), ViewNum(-1)),
                vec![genesis_qc.clone()],
            )]),
            block_index: {
                let mut map = BTreeMap::new();
                map.insert(
                    (BlockType::Genesis, ViewNum(-1), Identity(u64::MAX)),
                    vec![genesis_block.clone()],
                );
                map
            },
            produced_lead_in_view: {
                let mut map = BTreeMap::new();
                map.insert(ViewNum(0), false);
                map
            },
            received_messages: BTreeSet::from([
                Message::Block(genesis_block),
                Message::QC(genesis_qc),
            ]),
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
        sender: Identity,
        to_send: &mut Vec<(Message, Option<Identity>)>,
    ) -> bool {
        // Record message receipt for visualization
        crate::tracing_setup::message_received(
            &self.id,
            &sender,
            "process_message",
            format_message(&message, true),
        );

        // Check if we've seen this message before (duplicate detection)
        if cfg!(debug_assertions) {
            if self.received_messages.contains(&message) {
                tracing::debug!(
                    process_id = ?self.id,
                    message = format_message(&message, true),
                    "Ignoring duplicate message"
                );
                return false;
            }
        }

        // Record that we've received this message
        self.received_messages.insert(message.clone());
        match message {
            Message::Block(block) => {
                // Only process block if it's valid
                if !self.block_valid(&block) {
                    tracing::warn!(
                        process_id = ?self.id,
                        block = ?block.data.key,
                        "Received invalid block"
                    );
                    return false;
                }
                self.try_vote(
                    0,
                    &block.data.key,
                    Some(block.data.key.author.clone().expect("validated")),
                    to_send,
                );
                tracing::debug!(
                    process_id = ?self.id,
                    block_key = ?block.data.key,
                    block_type = ?block.data.key.type_,
                    view = ?block.data.key.view,
                    "Processing block message"
                );
                self.record_block(block.clone(), to_send);
                if self.phase_i.entry(self.view_i).or_insert(Phase::High) == &Phase::High {
                    // If ∃𝑏 ∈𝑀𝑖 with 𝑏.type= lead, 𝑏.view= view𝑖 , voted𝑖 (1,lead,𝑏.slot,𝑏.auth)= 0 then:
                    if block.data.key.type_ == BlockType::Lead && block.data.key.view == self.view_i
                    {
                        self.try_vote(1, &block.data.key, None, to_send);
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
                    && self // FIXME: compare only against max_1qc? keep a tips of 1-QCs?
                        .qcs
                        .values()
                        .filter(|qc| qc.data.z == 1)
                        .all(|qc| block.data.one.data.compare_qc(&qc.data) != Ordering::Less)
                {
                    if self.try_vote(1, &block.data.key, None, to_send) {
                        self.phase_i.insert(self.view_i, Phase::Low);
                    }
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
                self.record_vote(&vote_data, to_send);
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
        // Record view change with tracing
        crate::tracing_setup::protocol_transition(
            &self.id,
            "view_change",
            self.view_i,
            new_view,
            Some(&format_message(&cause, false)),
        );

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

    pub fn try_vote(
        &mut self,
        z: u8,
        block: &BlockKey,
        target: Option<Identity>,
        to_send: &mut Vec<(Message, Option<Identity>)>,
    ) -> bool {
        let author = block.author.clone().expect("validated");

        if !self
            .voted_i
            .contains(&(z, block.type_, block.slot, author.clone()))
        {
            self.voted_i
                .insert((z, block.type_, block.slot, author.clone()));

            let voted = Signed {
                data: VoteData {
                    z,
                    for_which: block.clone(),
                },
                author: self.id.clone(),
                signature: Signature {},
            };
            self.record_vote(&voted, to_send);
            to_send.push((Message::NewVote(voted), target));
            true
        } else {
            false
        }
    }
}
