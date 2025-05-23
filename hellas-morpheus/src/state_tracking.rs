use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::Arc,
};

use serde::{Deserialize, Serialize};

use crate::*;

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct PendingVotes {
    pub tr_1: BTreeMap<BlockKey, bool>,
    pub tr_2: BTreeMap<BlockKey, bool>,
    pub lead_1: BTreeMap<BlockKey, bool>,
    pub lead_2: BTreeMap<BlockKey, bool>,
    pub dirty: bool,
}

/// Tracks all structural state
#[derive(Clone, Serialize, Deserialize)]
pub struct StateIndex<Tr: Transaction> {
    /// Stores the current tips of the block DAG
    /// "The tips of Q_i are those q ∈ Q_i such that there does not exist q' ∈ Q_i with q' ≻ q"
    pub tips: Vec<FinishedQC>,

    /// Maps block keys to signed blocks (part of M_i in pseudocode)
    /// Implements part of "the set of all received messages"
    pub blocks: BTreeMap<BlockKey, Arc<Signed<Block<Tr>>>>,

    // === Performance optimization indexes ===
    /// Tracks which blocks point to which other blocks
    /// Used to efficiently determine the DAG structure
    pub block_pointed_by: BTreeMap<BlockKey, BTreeSet<BlockKey>>,

    /// Tracks the maximum view seen and its associated VoteData
    pub max_view: (ViewNum, FinishedQC),

    /// Tracks the maximum height block seen and its key
    /// Used for identifying the tallest block in the DAG
    pub max_height: (usize, BlockKey),

    /// Stores the maximum 1-QC seen by this process
    /// Used when entering a new view: "Send (v, q') signed by p_i to lead(v),
    /// where q' is a maximal amongst 1-QCs seen by p_i"
    pub max_1qc: FinishedQC,

    /// 1-QC for the leader block we produced in our previous slot
    pub latest_leader_1qc: Option<FinishedQC>,

    /// z-QC for the leader block we produced in our previous slot
    pub latest_leader_qc: Option<FinishedQC>,

    /// z-QC for the transaction block we produced in our previous slot
    pub latest_tr_qc: Option<FinishedQC>,

    /// Tracks unfinalized blocks with 2-QC
    /// Used to identify blocks that have 2-QC but are not yet finalized
    pub unfinalized_2qc: BTreeSet<FinishedQC>,

    /// Maps block keys to their finalization status
    /// Used to track which blocks have been finalized
    pub finalized: BTreeSet<BlockKey>,

    /// Maps block keys to their unfinalized QCs
    /// Used to track which QCs are not yet finalized
    pub unfinalized: BTreeMap<BlockKey, BTreeSet<FinishedQC>>,

    /// Tracks whether we've seen a leader block for each view
    /// Used to implement logic that depends on leader blocks within a view
    pub contains_lead_by_view: BTreeMap<ViewNum, bool>,

    /// Maps views to sets of unfinalized leader blocks
    /// Tracks which leader blocks are not yet finalized by view
    pub unfinalized_lead_by_view: BTreeMap<ViewNum, BTreeSet<BlockKey>>,
}

impl<Tr: Transaction> StateIndex<Tr> {
    pub fn new(genesis_qc: FinishedQC, genesis_block: Arc<Signed<Block<Tr>>>) -> Self {
        Self {
            max_view: (ViewNum(-1), genesis_qc.clone()),
            max_height: (0, GEN_BLOCK_KEY),
            max_1qc: genesis_qc.clone(),
            latest_leader_1qc: None,
            latest_leader_qc: None,
            latest_tr_qc: None,
            tips: vec![genesis_qc.clone()],
            blocks: {
                let mut map = BTreeMap::new();
                map.insert(GEN_BLOCK_KEY, genesis_block.clone());
                map
            },
            block_pointed_by: BTreeMap::new(),
            unfinalized_2qc: BTreeSet::new(),
            finalized: BTreeSet::from([GEN_BLOCK_KEY]),
            unfinalized: BTreeMap::new(),
            contains_lead_by_view: BTreeMap::new(),
            unfinalized_lead_by_view: BTreeMap::new(),
        }
    }
}

impl<Tr: Transaction> MorpheusProcess<Tr> {
    /// Records a new quorum certificate in this process's state
    ///
    /// This implements the automatic updating of Q_i from the pseudocode:
    /// "For z ∈ {0,1,2}, if p_i receives a z-quorum or a z-QC for b,
    /// and if Q_i does not contain a z-QC for b, then p_i automatically
    /// enumerates a z-QC for b into Q_i"
    pub fn record_qc(&mut self, qc: FinishedQC) {
        // TODO: prove that record_qc is temporally idempotent,
        // ie, calling it again later with an old qc will never
        // break anything

        // otherwise, we will need to use storage and filter out
        // any QCs we've already seen
        if !self.qcs.insert(qc.clone()) {
            return;
        }

        if qc.data.for_which.type_ == BlockType::Genesis {
            return;
        }

        // maintain the (type, author, {slot,view}) -> qc index
        if let Some(author) = &qc.data.for_which.author {
            if author == &self.id
                && qc.data.for_which.type_ == BlockType::Lead
                && qc.data.for_which.slot.is_pred(self.slot_i_lead)
            {
                self.index.latest_leader_qc = Some(qc.clone());
                if qc.data.z == 1 {
                    self.index.latest_leader_1qc = Some(qc.clone());
                }
            }

            if author == &self.id
                && qc.data.for_which.type_ == BlockType::Tr
                && qc.data.for_which.slot.is_pred(self.slot_i_tr)
            {
                self.index.latest_tr_qc = Some(qc.clone());
            }
        }

        // all new qcs are unfinalized until proven otherwise
        self.index
            .unfinalized
            .entry(qc.data.for_which.clone())
            .or_default()
            .insert(qc.clone());

        if qc.data.z == 1 {
            // FIXME: should we compare against tips? successive max_1qc
            // should form a chain, but maybe switching between them is
            // sometimes helpful even when they're equal?
            if self.index.max_1qc.data.compare_qc(&qc.data) != Ordering::Greater {
                tracing_setup::protocol_transition(
                    &self.id,
                    "updating max 1-QC",
                    &self.index.max_1qc.data,
                    &qc.data,
                    Some("new qc is greater than current max 1-QC"),
                );
                self.index.max_1qc = qc.clone();
            }
        }

        if qc.data.for_which.view > self.index.max_view.0 {
            self.index.max_view = (qc.data.for_which.view, qc.clone());
        }

        // TODO: don't do this _every_ time a qc is formed,
        //       batch up the changes and do some more efficient
        //       checking when we next need the tips? (isn't this right away?)

        // incrementally maintain the tips, which is the maximal antichain of all blocks.

        let mut tips_to_yeet = BTreeSet::new();
        for tip in &self.index.tips {
            // if the qc observes some existing tip, then that tip gets yoinked
            // in favor of the new qc
            if self.observes(qc.data.clone(), &tip.data) {
                tips_to_yeet.insert(tip.clone());
                tracing::info!(target: "yeet_tip", new_tip = ?qc.data, old_tip = ?tip.data);
            }
        }
        if !tips_to_yeet.is_empty() {
            // this qc is a new tip because it observes some existing tips
            self.index.tips.retain(|tip| !tips_to_yeet.contains(tip));
            self.index.tips.push(qc.clone());
            tracing::info!(target: "new_tip", qc = ?qc.data);
        } else {
            // this qc still might be a new tip if none of the existing tips observe it
            if !self
                .index
                .tips
                .iter()
                .any(|tip| self.observes(tip.data.clone(), &qc.data))
            {
                self.index.tips.push(qc.clone());
                tracing::info!(target: "new_tip", qc = ?qc.data);
            }
        }

        // now find all the waiting 2-qcs that this qc can finalize

        let finalized_here = self
            .index
            .unfinalized_2qc
            .iter()
            .cloned()
            .filter(|unfinalized_2qc| self.observes(qc.data.clone(), &unfinalized_2qc.data))
            .collect::<BTreeSet<_>>();

        if qc.data.z == 2 {
            // IMPORTANT: a QC observes itself, so make sure we add it AFTER
            // we scan, otherwise this block will incorrectly finalize itself.
            self.index.unfinalized_2qc.insert(qc.clone());
        }

        self.index
            .unfinalized_2qc
            .retain(|unfinalized_2qc| !finalized_here.contains(unfinalized_2qc));

        // finalize the blocks
        for finalized in finalized_here {
            tracing::debug!(target: "finalized", qc = ?finalized);
            self.index
                .unfinalized_lead_by_view
                .entry(finalized.data.for_which.view)
                .or_default()
                .remove(&finalized.data.for_which);
            self.index.unfinalized.remove(&finalized.data.for_which);
            self.index
                .finalized
                .insert(finalized.data.for_which.clone());

            // re-evaluate the pending votes for this view
            self.pending_votes
                .entry(finalized.data.for_which.view)
                .or_default()
                .dirty = true;
        }

        // start watching for 2-votes
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

    /// Records a new block in this process's state
    ///
    /// This implements part of the automatic updating of M_i from the pseudocode:
    /// "Each process p_i maintains a local variable M_i, which is automatically
    /// updated and specifies the set of all received messages."
    ///
    /// It will also record any QCs that are used as pointers in the block.
    pub fn record_block(&mut self, block: &Arc<Signed<Block<Tr>>>) {
        if self.index.blocks.contains_key(&block.data.key) {
            tracing::warn!(target: "duplicate_block", key = ?block.data.key);
            return;
        }

        // max_height is needed for is_eligible_for_tr_2_vote
        if block.data.key.height > self.index.max_height.0 {
            tracing::debug!(target: "new_max_height", height = block.data.key.height, key = ?block.data.key);
            self.index.max_height = (block.data.key.height, block.data.key.clone());
        }

        if let Some(author) = &block.data.key.author {
            // produced_lead_in_view is needed for leader_ready
            if block.data.key.type_ == BlockType::Lead && author == &self.id {
                self.produced_lead_in_view.insert(block.data.key.view, true);
            }
        }

        let block_key = block.data.key.clone();
        assert_eq!(
            self.index.blocks.insert(block_key.clone(), block.clone()),
            None
        );

        // track the voting status for this block
        let pending = self.pending_votes.entry(block.data.key.view).or_default();
        match block.data.key.type_ {
            BlockType::Lead => {
                self.index
                    .contains_lead_by_view
                    .insert(block.data.key.view, true);
                self.index
                    .unfinalized_lead_by_view
                    .entry(block.data.key.view)
                    .or_default()
                    .insert(block.data.key.clone());
                pending.lead_1.insert(block.data.key.clone(), true);
                pending.dirty = true;
            }
            BlockType::Tr => {
                pending.tr_1.insert(block.data.key.clone(), true);
                pending.dirty = true;
            }
            BlockType::Genesis => panic!("Why are we recording the genesis block?"),
        }

        // track the points-to relationship for block_is_single_tip
        for qc in &block.data.prev {
            self.index
                .block_pointed_by
                .entry(qc.data.for_which.clone())
                .or_default()
                .insert(block_key.clone());
        }

        // record any QCs that are used as pointers in the block
        for qc in &block.data.prev {
            self.record_qc(qc.clone())
        }
        self.record_qc(block.data.one.clone());
    }

    /// Determines if one QC observes another according to the observes relation ⪰
    ///
    /// Implements the observes relation from the pseudocode:
    /// "We define the 'observes' relation ⪰ on Q_i to be the minimal preordering satisfying (transitivity and):
    /// • If q,q' ∈ Q_i, q.type = q'.type, q.auth = q'.auth and q.slot > q'.slot, then q ⪰ q'.
    /// • If q,q' ∈ Q_i, q.type = q'.type, q.auth = q'.auth, q.slot = q'.slot, and q.z ≥ q'.z, then q ⪰ q'."
    /// • If q,q' ∈ Q_i, q.b = b, q'.b = b', b ∈ M_i and b points to b', then q ⪰ q'."
    ///
    /// Implemented as a BFS on the points-to graph combined with a direct
    /// observation check.
    pub fn observes(&self, root: VoteData, needle: &VoteData) -> bool {
        let mut observed = false;
        let mut to_visit: VecDeque<VoteData> = vec![root].into();
        while !to_visit.is_empty() {
            let node = to_visit.pop_front().unwrap();
            if self.directly_observes(&node, needle) {
                observed = true;
                break;
            }
            if let Some(block) = self.index.blocks.get(&node.for_which) {
                for prev in &block.data.prev {
                    to_visit.push_back(prev.data.clone());
                }
            } else {
                tracing::warn!("Block not found for {:?}", node.for_which);
            }
        }
        observed
    }

    /// Determines if one QC directly observes another (without transitivity)
    ///
    /// Implements the direct observation component of the observes relation ⪰
    pub fn directly_observes(&self, looks: &VoteData, seen: &VoteData) -> bool {
        if looks.for_which.type_ == seen.for_which.type_
            && looks.for_which.author == seen.for_which.author
            && looks.for_which.slot > seen.for_which.slot
        {
            return true;
        }
        if looks.for_which.type_ == seen.for_which.type_
            && looks.for_which.author == seen.for_which.author
            && looks.for_which.slot == seen.for_which.slot
            && looks.z >= seen.z
        {
            return true;
        }
        if let Some(block) = self.index.blocks.get(&looks.for_which) {
            if block
                .data
                .prev
                .iter()
                .any(|prev| prev.data.for_which == seen.for_which)
            {
                return true;
            }
        }
        false
    }

    fn block_is_single_tip(&self, block_key: &BlockKey) -> bool {
        if self.index.tips.len() != 1 {
            return false;
        }
        match self.index.tips.get(0) {
            Some(tip) => self
                .index
                .block_pointed_by
                .get(&tip.data.for_which)
                .map_or(false, |parents| {
                    parents.len() == 1 && parents.first().unwrap() == block_key
                }),
            None => false,
        }
    }

    pub(crate) fn is_eligible_for_tr_1_vote(&self, block_key: &BlockKey) -> bool {
        let has_single_tip = self.block_is_single_tip(block_key);

        if !has_single_tip || !self.index.blocks.contains_key(block_key) {
            return false;
        }

        let block = self.index.blocks.get(block_key).unwrap();

        block.data.one.data.compare_qc(&self.index.max_1qc.data) != Ordering::Less
    }

    pub(crate) fn is_eligible_for_tr_2_vote(&self, block_key: &BlockKey) -> bool {
        let has_single_tip = self.index.tips.len() == 1
            && self.index.tips.get(0).map_or(false, |tip| {
                tip.data.z == 1 && tip.data.for_which.eq(block_key)
            });

        let no_higher_blocks = self.index.max_height.0 <= block_key.height;

        has_single_tip && no_higher_blocks
    }
}
