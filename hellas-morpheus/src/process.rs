use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use crate::state_tracking::{PendingVotes, StateIndex};
use crate::{format::format_message, *};
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize, Valid};
use serde::{Deserialize, Serialize};

const COMPLAIN_TIMEOUT: u128 = 6;
const END_VIEW_TIMEOUT: u128 = 12;

/// MorpheusProcess represents a single process (p_i) in the Morpheus protocol
///
/// This struct implements the Algorithm 1 from the Morpheus pseudocode,
/// maintaining all state required for processing messages, voting, and
/// producing blocks according to the protocol specification.
#[derive(Clone, Serialize, Deserialize)]
pub struct MorpheusProcess {
    pub kb: KeyBook,

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
    #[serde(with = "serde_json_any_key::any_key_map")]
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
    pub end_views: QuorumTrack<ViewNum>,

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
    pub vote_tracker: QuorumTrack<VoteData>,

    /// Tracks view change messages
    /// Used to collect view v messages with 1-QCs sent to the leader
    #[serde(with = "serde_json_any_key::any_key_map")]
    pub start_views: BTreeMap<ViewNum, Vec<Arc<Signed<StartView>>>>,

    pub index: StateIndex,

    /// Tracks whether we've produced a leader block in each view
    /// Used for leader logic to avoid producing multiple leader blocks in same view
    #[serde(with = "serde_json_any_key::any_key_map")]
    pub produced_lead_in_view: BTreeMap<ViewNum, bool>,

    /// All messages received by this process
    pub received_messages: BTreeSet<Message>,

    pub genesis: Arc<Signed<Block>>,
    pub genesis_qc: Arc<ThreshSigned<VoteData>>,
    pub ready_transactions: Vec<Transaction>,

    pub pending_votes: BTreeMap<ViewNum, PendingVotes>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
/// Tracks votes for a particular data type and helps form quorums
///
/// This is an implementation helper that tracks votes from different processes
/// and determines when a quorum (n-f votes) has been reached.
/// Used for implementing the collection of votes in the protocol.
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

impl MorpheusProcess {
    pub fn new(keybook: KeyBook, id: Identity, n: usize, f: usize) -> Self {
        // Track process creation with tracing
        crate::tracing_setup::register_process(&id, n, f);

        // Create genesis block and its 1-QC
        let genesis_block = Arc::new(Signed {
            data: Block {
                key: GEN_BLOCK_KEY,
                prev: Vec::new(),
                one: ThreshSigned {
                    data: VoteData {
                        z: 1,
                        for_which: GEN_BLOCK_KEY,
                    },
                    signature: hints::Signature::default(),
                },
                data: BlockData::Genesis,
            },
            author: Identity(u64::MAX),
            signature: hints::PartialSignature::default(),
        });

        let genesis_qc = Arc::new(ThreshSigned {
            data: VoteData {
                z: 1,
                for_which: GEN_BLOCK_KEY,
            },
            signature: hints::Signature::default(),
        });

        // Initialize with a recommended default timeout
        MorpheusProcess {
            kb: keybook,
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
            end_views: QuorumTrack {
                votes: BTreeMap::new(),
            },
            zero_qcs_sent: BTreeSet::new(),
            complained_qcs: BTreeSet::new(),
            view_entry_time: 0,
            current_time: 0,

            vote_tracker: QuorumTrack {
                votes: BTreeMap::new(),
            },
            start_views: BTreeMap::new(),
            index: StateIndex::new(genesis_qc.clone(), genesis_block.clone()),
            produced_lead_in_view: {
                let mut map = BTreeMap::new();
                map.insert(ViewNum(0), false);
                map
            },
            received_messages: BTreeSet::from([
                Message::Block(genesis_block.clone()),
                Message::QC(genesis_qc.clone()),
            ]),

            genesis: genesis_block,
            genesis_qc: genesis_qc.clone(),
            ready_transactions: Vec::new(),
            pending_votes: BTreeMap::new(),
        }
    }

    pub(crate) fn send_msg(
        &mut self,
        to_send: &mut Vec<(Message, Option<Identity>)>,
        message: (Message, Option<Identity>),
    ) {
        if message.1.is_none() || message.1.as_ref().unwrap() == &self.id {
            // IMPORTANT: implements note from page 8:
            // In what follows, we suppose that, when a correct process sends a
            // message to ‘all processes’, it regards that message as
            // immediately received by itself
            self.process_message(message.0.clone(), self.id.clone(), to_send);
        }
        to_send.push(message);
    }

    pub fn set_now(&mut self, now: u128) {
        self.current_time = now;
    }

    pub fn verify_leader(&self, author: Identity, view: ViewNum) -> bool {
        author.0 as usize == 1 + (view.0 as usize % self.n)
    }

    pub fn lead(&self, view: ViewNum) -> Identity {
        Identity((view.0 as u64 % self.n as u64) + 1) // identities are 1-indexed... ok
    }

    #[tracing::instrument(skip(self, sender, to_send), fields(process_id = ?self.id))]
    pub fn process_message(
        &mut self,
        message: Message,
        sender: Identity,
        to_send: &mut Vec<(Message, Option<Identity>)>,
    ) -> bool {
        // Check if we've seen this message before (duplicate detection)
        if cfg!(debug_assertions) {
            if self.received_messages.contains(&message) {
                tracing::error!(
                    target: "duplicate_message",
                    sender = ?sender,
                    full_message = format_message(&message, true),
                    "Ignoring duplicate message: why did we receive it?"
                );
                return false;
            }
        }

        // Record that we've received this message
        self.received_messages.insert(message.clone());
        tracing::debug!("received a message");

        match message {
            Message::Block(block) => {
                if let Err(error) = self.block_valid(&block) {
                    tracing::error!(
                        target: "invalid_block",
                        process_id = ?self.id,
                        block_key = ?block.data.key,
                        error = ?error,
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
                    target: "valid_block",
                    block_key = ?block.data.key,
                );
                self.record_block(&block);
            }
            Message::NewVote(vote_data) => {
                if !vote_data.valid_signature(&self.kb) {
                    tracing::error!(
                        target: "invalid_vote",
                        process_id = ?self.id,
                        vote_data = ?vote_data,
                    );
                    return false;
                }
                self.record_vote(&vote_data, to_send);
            }
            Message::QC(qc) => {
                if !qc.valid_signature(&self.kb) {
                    tracing::error!(
                        target: "invalid_qc",
                        process_id = ?self.id,
                        qc = ?qc,
                    );
                    return false;
                }
                self.record_qc(&qc);
                if self.index.max_view.0 > self.view_i {
                    self.end_view(
                        Message::QC(self.index.qcs.get(&self.index.max_view.1).cloned().unwrap()),
                        self.index.max_view.0,
                        to_send,
                    );
                }
            }
            Message::EndView(end_view) => {
                if !end_view.valid_signature(&self.kb) {
                    tracing::error!(
                        target: "invalid_end_view",
                        process_id = ?self.id,
                        end_view = ?end_view,
                    );
                    return false;
                }
                match self.end_views.record_vote(end_view.clone()) {
                    Ok(num_votes) => {
                        if end_view.data >= self.view_i && num_votes >= self.f + 1 {
                            let votes_now = self
                                .end_views
                                .votes
                                .get(&end_view.data)
                                .unwrap()
                                .values()
                                .map(|v| (v.author.0 as usize - 1, v.signature.clone()))
                                .collect::<Vec<_>>();
                            let agg = self.kb.hints_setup.aggregator();
                            let mut data = Vec::new();
                            end_view.data.serialize_compressed(&mut data).unwrap();
                            let signed = hints::sign_aggregate(
                                &agg,
                                hints::F::from((self.f + 1) as u64),
                                &votes_now,
                                &data,
                            )
                            .unwrap();
                            self.send_msg(
                                to_send,
                                (
                                    Message::EndViewCert(Arc::new(ThreshSigned {
                                        data: end_view.data,
                                        signature: signed,
                                    })),
                                    None,
                                ),
                            );
                        }
                    }
                    Err(Duplicate) => return false,
                }
            }
            Message::EndViewCert(end_view_cert) => {
                if !end_view_cert.valid_signature(&self.kb) {
                    tracing::error!(
                        target: "invalid_end_view_cert",
                        process_id = ?self.id,
                        end_view_cert = ?end_view_cert,
                    );
                    return false;
                }
                let view = end_view_cert.data.incr();
                if view >= self.view_i {
                    self.end_view(Message::EndViewCert(end_view_cert), view, to_send);
                }
            }
            Message::StartView(start_view) => {
                if !start_view.valid_signature(&self.kb) {
                    tracing::error!(
                        target: "invalid_start_view",
                        process_id = ?self.id,
                        start_view = ?start_view,
                    );
                    return false;
                }
                if start_view.data.qc.data.z != 1 {
                    return false;
                }
                self.record_qc(&Arc::new(start_view.data.qc.clone()));
                self.start_views
                    .entry(start_view.data.view)
                    .or_insert(Vec::new())
                    .push(start_view);
            }
        }

        if cfg!(debug_assertions) {
            let violations = self.check_invariants();
            assert!(
                violations.is_empty(),
                "Process {} has invariant violations: {:?}",
                self.id.0,
                violations
            );
        }

        // Re-evaluate any pending voting decisions
        self.reevaluate_pending_votes(to_send);

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

        assert!(self.view_i <= new_view);

        self.view_i = new_view;
        self.view_entry_time = self.current_time;
        self.phase_i.insert(new_view, Phase::High);

        // View changed, we need to re-evaluate pending votes
        self.pending_votes.entry(new_view).or_default().dirty = true;

        self.send_msg(to_send, (cause, None));

        // Send all tips we've created to the new leader
        // "Send all tips q' of Q_i such that q'.auth = p_i to lead(v)"
        for tip in self.index.tips.clone() {
            if tip.for_which.author == Some(self.id.clone()) {
                self.send_msg(
                    to_send,
                    (
                        Message::QC(self.index.qcs.get(&tip).unwrap().clone()),
                        Some(self.lead(new_view)),
                    ),
                );
            }
        }
        self.send_msg(
            to_send,
            (
                Message::StartView(Arc::new(Signed::from_data(
                    StartView {
                        view: new_view,
                        qc: ThreshSigned::clone(&self.index.max_1qc),
                    },
                    &self.kb,
                ))),
                Some(self.lead(new_view)),
            ),
        );

        // Re-evaluate any pending voting decisions after view change
        self.reevaluate_pending_votes(to_send);
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

        if time_in_view >= self.delta * COMPLAIN_TIMEOUT {
            let maximal_unfinalized = self
                .index
                .unfinalized
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
                if !self.complained_qcs.insert(qc_data.clone()) {
                    self.send_msg(
                        to_send,
                        (
                            Message::QC(self.index.qcs.get(&qc_data).cloned().unwrap()),
                            Some(self.lead(self.view_i)),
                        ),
                    );
                }
            }
        }

        // Second timeout - 12Δ, send end-view message
        if time_in_view >= self.delta * END_VIEW_TIMEOUT && !self.index.unfinalized.is_empty() {
            self.send_msg(
                to_send,
                (
                    Message::EndView(Arc::new(ThreshPartial::from_data(self.view_i, &self.kb))),
                    None,
                ),
            );
        }
    }
}
