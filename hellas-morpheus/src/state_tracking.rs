use std::{
    cmp::Ordering,
    collections::{BTreeSet, VecDeque},
    sync::Arc,
};

use crate::*;

impl MorpheusProcess {
    pub fn record_qc(
        &mut self,
        qc: ThreshSigned<VoteData>,
        to_send: &mut Vec<(Message, Option<Identity>)>,
    ) {
        if self.qcs.contains_key(&qc.data) {
            return;
        }
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
            // TODO: can this be a simple observes_in_one_step?
            // relying on monotonicty (and density?) of tips...
            if self.observes(qc.data.clone(), tip) {
                tips_to_yeet.insert(tip.clone());
            }
        }
        if !tips_to_yeet.is_empty() {
            // this qc is a new tip because it observes some existing tips
            self.tips.retain(|tip| !tips_to_yeet.contains(tip));
            self.tips.push(qc.data.clone());
        } else {
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
