use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use serde::{Deserialize, Serialize};
use crate::*;

#[derive(Serialize, Deserialize)]
pub struct MorpheusProcess {
    pub id: Identity,

    // from the pseudocode
    pub view_i: ViewNum,
    pub slot_i_lead: SlotNum,
    pub slot_i_tr: SlotNum,
    pub voted_i: BTreeSet<(u8, BlockType, SlotNum, Identity)>,
    pub phase_i: BTreeMap<ViewNum, Phase>,
    pub n: usize,
    pub f: usize,
    pub delta: std::time::Duration,

    // auxiliary
    pub end_views: VoteTrack<ViewNum>,
    pub zero_qcs_sent: BTreeSet<BlockKey>,
    pub complained_qcs: BTreeSet<VoteData>,
    pub view_entry_time: std::time::Instant,
    pub current_time: std::time::Instant,

    pub vote_tracker: VoteTrack<VoteData>,
    pub start_views: BTreeMap<ViewNum, Vec<Signed<StartView>>>,
    pub qcs: BTreeMap<VoteData, ThreshSigned<VoteData>>,
    pub max_view: (ViewNum, VoteData),
    pub max_height: (usize, BlockKey),
    pub max_1qc: ThreshSigned<VoteData>,
    pub tips: Vec<VoteData>,
    pub blocks: BTreeMap<BlockKey, Signed<Arc<Block>>>,
    pub block_pointed_by: BTreeMap<BlockKey, BTreeSet<BlockKey>>,
    pub unfinalized_2qc: BTreeSet<VoteData>,
    pub finalized: BTreeMap<BlockKey, bool>,
    pub unfinalized: BTreeMap<BlockKey, BTreeSet<VoteData>>,
    pub contains_lead_by_view: BTreeMap<ViewNum, bool>,
    pub unfinalized_lead_by_view: BTreeMap<ViewNum, BTreeSet<BlockKey>>,

    pub qc_index: BTreeMap<(BlockType, Identity, SlotNum), ThreshSigned<VoteData>>,
    pub qc_by_view: BTreeMap<(BlockType, Identity, ViewNum), Vec<ThreshSigned<VoteData>>>,
    pub block_index: BTreeMap<(BlockType, ViewNum, Identity), Vec<Signed<Arc<Block>>>>,
    pub produced_lead_in_view: BTreeMap<ViewNum, bool>,
}

pub struct VoteTrack<T> {
    pub votes: BTreeMap<T, BTreeMap<Identity, Signed<T>>>,
}

pub struct Duplicate;

impl<T: Ord + Clone> VoteTrack<T> {
    pub fn record_vote(&mut self, vote: Signed<T>) -> Result<usize, Duplicate> {
        let votes_now = self
            .votes
            .entry(vote.data.clone())
            .or_insert(BTreeMap::new());
        if votes_now.contains_key(&vote.author) {
            return Err(Duplicate);
        }
        votes_now.insert(vote.author.clone(), vote);
        Ok(votes_now.len())
    }
}

impl MorpheusProcess {
    pub fn new(id: Identity, n: usize, f: usize) -> Self {
        let now = std::time::Instant::now();

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
        let delta = std::time::Duration::from_secs(10);

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
            delta,

            // Auxiliary fields
            end_views: VoteTrack {
                votes: BTreeMap::new(),
            },
            zero_qcs_sent: BTreeSet::new(),
            complained_qcs: BTreeSet::new(),
            view_entry_time: now,
            current_time: now,

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

    pub fn set_now(&mut self, now: std::time::Instant) {
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
                        .all(|j| block.one.data.compare_qc(&j.data.max_1_qc.data) != Ordering::Less)
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
                if start_view.data.max_1_qc.data.z != 1 {
                    return false;
                }
                self.record_qc(start_view.data.max_1_qc.clone(), to_send);
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
                    max_1_qc: self.max_1qc.clone(),
                },
                author: self.id.clone(),
                signature: Signature {},
            }),
            Some(self.lead(new_view)),
        ));
    }

    pub fn check_timeouts(&mut self, to_send: &mut Vec<(Message, Option<Identity>)>) {
        let time_in_view = self.current_time.duration_since(self.view_entry_time);

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
}
