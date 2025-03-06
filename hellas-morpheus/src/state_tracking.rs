use std::{
    cmp::Ordering,
    collections::{BTreeSet, VecDeque},
    sync::Arc,
};

use crate::*;

impl MorpheusProcess {
    /// Records a new quorum certificate in this process's state
    ///
    /// This implements the automatic updating of Q_i from the pseudocode:
    /// "For z ∈ {0,1,2}, if p_i receives a z-quorum or a z-QC for b,
    /// and if Q_i does not contain a z-QC for b, then p_i automatically 
    /// enumerates a z-QC for b into Q_i"
    /// 
    /// There is a substantial amount of intricate code here that attempts to
    /// incrementally/lazily compute the appropriate messages to send based on
    /// indices and the current message being processed.
    /// 
    /// It's not clear that this is correct, and it may even be slower than a
    /// more naive approach if the set sizes were kept small.
    pub fn record_qc(
        &mut self,
        qc: ThreshSigned<VoteData>,
        to_send: &mut Vec<(Message, Option<Identity>)>,
    ) {
        if self.qcs.contains_key(&qc.data) {
            return;
        }

        // maintain the (type, author, {slot,view}) -> qc index
        if let Some(author) = &qc.data.for_which.author {
            self.qc_index.insert(
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
            if self.max_1qc.data.compare_qc(&qc.data) != Ordering::Less {
                self.max_1qc = qc.clone();
            }
        }
        if qc.data.for_which.view > self.max_view.0 {
            self.max_view = (qc.data.for_which.view, qc.data.clone());
        }

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
        if qc.data.z == 2 {
            self.unfinalized_2qc.insert(qc.data.clone());
        }

        // now find all the 2-qcs that this qc can finalize
        // TODO: justify why this is correct
        let finalized_here = self
            .unfinalized_2qc
            .iter()
            .cloned()
            .filter(|unfinalized_2qc| self.observes(qc.data.clone(), unfinalized_2qc))
            .collect::<BTreeSet<_>>();
        self.unfinalized_2qc
            .retain(|unfinalized_2qc| !finalized_here.contains(unfinalized_2qc));
        for finalized in finalized_here {
            self.unfinalized_lead_by_view
                .entry(finalized.for_which.view)
                .or_default()
                .remove(&finalized.for_which);
            self.unfinalized.remove(&finalized.for_which);
            self.finalized.insert(finalized.for_which.clone(), true);
        }

        // Check if we need to vote for a leader block
        if self.phase_i.entry(self.view_i).or_insert(Phase::High) == &Phase::High {
            if qc.data.z == 1
                && qc.data.for_which.type_ == BlockType::Lead
                && qc.data.for_which.view == self.view_i
                && !self.voted_i.contains(&(
                    2,
                    BlockType::Lead,
                    qc.data.for_which.slot,
                    qc.data.for_which.author.clone().expect("validated"),
                ))
            {
                self.voted_i.insert((
                    2,
                    BlockType::Lead,
                    qc.data.for_which.slot,
                    qc.data.for_which.author.clone().expect("validated"),
                ));
                to_send.push((
                    Message::NewVote(Signed {
                        data: VoteData {
                            z: 2,
                            for_which: qc.data.for_which.clone(),
                        },
                        author: self.id.clone(),
                        signature: Signature {},
                    }),
                    None,
                ));
            }
        }

        // Check if we need to vote for a transaction block
        if qc.data.z == 1
            && self.tips.len() == 1
            && self.tips[0] == qc.data
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
            && qc.data.for_which.type_ == BlockType::Tr
            && !self.voted_i.contains(&(
                2,
                BlockType::Tr,
                qc.data.for_which.slot,
                qc.data.for_which.author.clone().expect("validated"),
            ))
            && self.max_height.0 <= qc.data.for_which.height
        {
            self.phase_i.insert(self.view_i, Phase::Low);
            self.voted_i.insert((
                2,
                BlockType::Tr,
                qc.data.for_which.slot,
                qc.data.for_which.author.clone().expect("validated"),
            ));
            to_send.push((
                Message::NewVote(Signed {
                    data: VoteData {
                        z: 2,
                        for_which: qc.data.for_which.clone(),
                    },
                    author: self.id.clone(),
                    signature: Signature {},
                }),
                None,
            ));
        }
        {}
    }

    /// Records a new block in this process's state
    ///
    /// This implements part of the automatic updating of M_i from the pseudocode:
    /// "Each process p_i maintains a local variable M_i, which is automatically 
    /// updated and specifies the set of all received messages."
    /// 
    /// It will also record any QCs that are used as pointers in the block.
    pub fn record_block(
        &mut self,
        block: Signed<Arc<Block>>,
        to_send: &mut Vec<(Message, Option<Identity>)>,
    ) {
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
        self.finalized.insert(block_key.clone(), false);
        self.blocks.insert(block_key.clone(), block.clone());

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
            self.record_qc(qc.clone(), to_send);
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
    pub fn directly_observes(&self, qc1: &VoteData, qc2: &VoteData) -> bool {
        if qc1.for_which.type_ == qc2.for_which.type_
            && qc1.for_which.author == qc2.for_which.author
            && qc1.for_which.slot > qc2.for_which.slot
        {
            return true;
        }
        if qc1.for_which.type_ == qc2.for_which.type_
            && qc1.for_which.author == qc2.for_which.author
            && qc1.for_which.slot == qc2.for_which.slot
            && qc1.z >= qc2.z
        {
            return true;
        }
        if let Some(block) = self.blocks.get(&qc1.for_which) {
            if block
                .data
                .prev
                .iter()
                .any(|prev| prev.data.for_which == qc2.for_which)
            {
                return true;
            }
        }
        false
    }
}
