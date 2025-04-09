use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::Arc,
};

use serde::{Deserialize, Serialize};

use crate::*;

#[derive(Serialize, Deserialize, Default)]
pub struct PendingVotes {
    pub tr_1: BTreeMap<BlockKey, bool>,
    pub tr_2: BTreeMap<BlockKey, bool>,
    pub lead_1: BTreeMap<BlockKey, bool>,
    pub lead_2: BTreeMap<BlockKey, bool>,
    pub dirty: bool,
}

impl MorpheusProcess {
    pub fn try_vote(
        &mut self,
        z: u8,
        block: &BlockKey,
        target: Option<Identity>,
        to_send: &mut Vec<(Message, Option<Identity>)>,
    ) -> bool {
        let author = block.author.clone().expect("not voting for genesis block");

        if !self
            .voted_i
            .contains(&(z, block.type_, block.slot, author.clone()))
        {
            self.voted_i
                .insert((z, block.type_, block.slot, author.clone()));

            let voted = Arc::new(Signed {
                data: VoteData {
                    z,
                    for_which: block.clone(),
                },
                author: self.id.clone(),
                signature: Signature {},
            });
            self.record_vote(&voted, to_send);
            self.send_msg(to_send, (Message::NewVote(voted.clone()), target));
            true
        } else {
            false
        }
    }

    /// Returns false if the vote is a duplicate (sender already voted there)
    pub fn record_vote(
        &mut self,
        vote_data: &Arc<Signed<VoteData>>,
        to_send: &mut Vec<(Message, Option<Identity>)>,
    ) -> bool {
        match self.vote_tracker.record_vote(vote_data.clone()) {
            Ok(num_votes) => {
                if num_votes == self.n - self.f {
                    // TODO: real crypto
                    let quorum_formed = Arc::new(ThreshSigned {
                        data: vote_data.data.clone(),
                        signature: ThreshSignature {},
                    });
                    if vote_data.data.z == 0
                        && vote_data.data.for_which.author.as_ref() == Some(&self.id)
                        && !self.zero_qcs_sent.contains(&vote_data.data.for_which)
                    {
                        self.zero_qcs_sent.insert(vote_data.data.for_which.clone());
                        self.send_msg(to_send, (Message::QC(quorum_formed.clone()), None));
                    }
                    self.record_qc(&quorum_formed);
                }
                true
            }
            Err(Duplicate) => {
                tracing::warn!(
                    "Duplicate vote for {:?} from {:?}",
                    vote_data.data,
                    vote_data.author
                );
                false
            }
        }
    }

    /// Records a new quorum certificate in this process's state
    ///
    /// This implements the automatic updating of Q_i from the pseudocode:
    /// "For z ∈ {0,1,2}, if p_i receives a z-quorum or a z-QC for b,
    /// and if Q_i does not contain a z-QC for b, then p_i automatically
    /// enumerates a z-QC for b into Q_i"
    pub fn record_qc(&mut self, qc: &Arc<ThreshSigned<VoteData>>) {
        crate::tracing_setup::qc_formed(&self.id, qc.data.z, &qc.data);

        if self.qcs.contains_key(&qc.data) {
            tracing::warn!("recording duplicate qc for {:?}", qc.data);
            return;
        }

        // maintain the (type, author, {slot,view}) -> qc index
        if let Some(author) = &qc.data.for_which.author {
            self.qc_by_slot.insert(
                (
                    qc.data.for_which.type_,
                    author.clone(),
                    qc.data.for_which.slot,
                ),
                qc.clone(),
            );

            self.qc_by_view
                .entry((
                    qc.data.for_which.type_,
                    author.clone(),
                    qc.data.for_which.view,
                ))
                .or_insert_with(Vec::new)
                .push(qc.clone());
        }

        // all new qcs are unfinalized until proven otherwise
        self.unfinalized
            .entry(qc.data.for_which.clone())
            .or_default()
            .insert(qc.data.clone());

        if qc.data.z == 1 {
            self.all_1qc.insert(qc.clone());

            // FIXME: should we compare against tips? all_1qc? max_1qc should be a chain,
            // but maybe
            if self.max_1qc.data.compare_qc(&qc.data) != Ordering::Less {
                tracing_setup::protocol_transition(
                    &self.id,
                    "updating max 1-QC",
                    &self.max_1qc.data,
                    &qc.data,
                    Some("new qc is greater than current max 1-QC"),
                );
                self.max_1qc = qc.clone();
            }
        }

        if qc.data.for_which.view > self.max_view.0 {
            self.max_view = (qc.data.for_which.view, qc.data.clone());
        }

        // TODO: don't do this _every_ time a qc is formed,
        //       batch up the changes and do some more efficient
        //       checking when we next need the tips? (isn't this right away?)

        let mut tips_to_yeet = BTreeSet::new();
        for tip in &self.tips {
            // if the qc observes some existing tip, then that tip gets yoinked
            // in favor of the new qc
            if self.observes(qc.data.clone(), tip) {
                tips_to_yeet.insert(tip.clone());
            }
        }
        if !tips_to_yeet.is_empty() {
            // this qc is a new tip because it observes some existing tips
            self.tips.retain(|tip| !tips_to_yeet.contains(tip));
            self.tips.push(qc.data.clone());
        } else {
            // this qc still might be a new tip if none of the existing tips observe it
            if !self
                .tips
                .iter()
                .cloned()
                .any(|tip| self.observes(tip, &qc.data))
            {
                self.tips.push(qc.data.clone());
            }
        }

        self.qcs.insert(qc.data.clone(), qc.clone());

        // now find all the waiting 2-qcs that this qc can finalize
        // TODO: justify why this is correct according to the paper.

        let finalized_here = self
            .unfinalized_2qc
            .iter()
            .cloned()
            .filter(|unfinalized_2qc| self.observes(qc.data.clone(), unfinalized_2qc))
            .collect::<BTreeSet<_>>();

        if qc.data.z == 2 {
            // IMPORTANT: QC observes itself, so make sure we add it AFTER we scan,
            // otherwise this block will finalize itself.
            self.unfinalized_2qc.insert(qc.data.clone());
        }

        self.unfinalized_2qc
            .retain(|unfinalized_2qc| !finalized_here.contains(unfinalized_2qc));

        for finalized in finalized_here {
            self.unfinalized_lead_by_view
                .entry(finalized.for_which.view)
                .or_default()
                .remove(&finalized.for_which);
            self.unfinalized.remove(&finalized.for_which);
            self.finalized.insert(finalized.for_which.clone(), true);

            self.pending_votes
                .entry(finalized.for_which.view)
                .or_default()
                .dirty = true;
        }

        if qc.data.z == 1 {
            let pending = self
                .pending_votes
                .entry(qc.data.for_which.view)
                .or_default();
            pending.dirty = true;
            match qc.data.for_which.type_ {
                BlockType::Lead => pending.lead_2.insert(qc.data.for_which.clone(), true),
                BlockType::Tr => pending.tr_2.insert(qc.data.for_which.clone(), true),
                BlockType::Genesis => unreachable!(),
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
    pub fn record_block(&mut self, block: &Arc<Signed<Block>>) {
        if block.data.key.height > self.max_height.0 {
            self.max_height = (block.data.key.height, block.data.key.clone());
        }
        if let Some(author) = &block.data.key.author {
            self.block_index
                .entry((block.data.key.type_, block.data.key.view, author.clone()))
                .or_insert_with(Vec::new)
                .push(block.clone());

            if block.data.key.type_ == BlockType::Lead && author == &self.id {
                self.produced_lead_in_view.insert(block.data.key.view, true);
            }
        }

        let block_key = block.data.key.clone();
        assert_eq!(self.finalized.insert(block_key.clone(), false), None);
        assert_eq!(self.blocks.insert(block_key.clone(), block.clone()), None);

        let pending = self.pending_votes.entry(block.data.key.view).or_default();
        match block.data.key.type_ {
            BlockType::Lead => {
                self.contains_lead_by_view.insert(block.data.key.view, true);
                self.unfinalized_lead_by_view
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

        for qc in &block.data.prev {
            self.block_pointed_by
                .entry(qc.data.for_which.clone())
                .or_default()
                .insert(block_key.clone());
        }
        for qc in block
            .data
            .prev
            .iter()
            .chain(Some(&block.data.one).into_iter())
        {
            // TODO: these Arc are temporary....
            self.record_qc(&Arc::new(qc.clone()));
        }
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
        // do a BFS from root to see if it observes needle
        let mut observed = false;
        let mut to_visit: VecDeque<VoteData> = vec![root].into();
        while !to_visit.is_empty() {
            let node = to_visit.pop_front().unwrap();
            if self.directly_observes(&node, needle) {
                observed = true;
                break;
            }
            if let Some(block) = self.blocks.get(&node.for_which) {
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
        if let Some(block) = self.blocks.get(&looks.for_which) {
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

    pub fn set_phase(&mut self, phase: Phase) {
        self.phase_i.insert(self.view_i, phase);
    }

    /// Re-evaluate all pending votes based on current state
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
            .contains_lead_by_view
            .get(&current_view)
            .copied()
            .unwrap_or(false);
        let unfinalized_lead_empty = self
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

    fn block_is_single_tip(&self, block_key: &BlockKey) -> bool {
        if self.tips.len() != 1 {
            return false;
        }
        match self.tips.get(0) {
            Some(tip) => self
                .block_pointed_by
                .get(&tip.for_which)
                .map_or(false, |parents| {
                    parents.len() == 1 && parents.first().unwrap() == block_key
                }),
            None => false,
        }
    }

    pub(crate) fn is_eligible_for_tr_1_vote(&self, block_key: &BlockKey) -> bool {
        let has_single_tip = self.block_is_single_tip(block_key);

        if !has_single_tip || !self.blocks.contains_key(block_key) {
            return false;
        }

        let block = self.blocks.get(block_key).unwrap();
        self.all_1qc
            .iter()
            .all(|qc| block.data.one.data.compare_qc(&qc.data) != Ordering::Less)
    }

    pub(crate) fn is_eligible_for_tr_2_vote(&self, block_key: &BlockKey) -> bool {
        let has_single_tip = self.tips.len() == 1
            && self
                .tips
                .get(0)
                .map_or(false, |tip| tip.z == 1 && tip.for_which.eq(block_key));

        let no_higher_blocks = self.max_height.0 <= block_key.height;

        has_single_tip && no_higher_blocks
    }
}
