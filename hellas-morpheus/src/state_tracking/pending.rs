use std::collections::{BTreeMap, BTreeSet};
use crate::*;

impl MorpheusProcess {
    /// Set the protocol phase for current view
    pub fn set_phase(&mut self, phase: Phase) {
        self.phase_i.insert(self.view_i, phase);
    }

    /// Re-evaluate and send pending votes for the current view
    pub fn reevaluate_pending_votes(&mut self, to_send: &mut Vec<(Message, Option<Identity>)>) {
        let v = self.view_i;
        let mut all = std::mem::replace(&mut self.pending_votes, BTreeMap::new());
        let pending = all.entry(v).or_default();
        if !pending.dirty { self.pending_votes = all; return; }
        let has_lead = *self.index.contains_lead_by_view.get(&v).unwrap_or(&false);
        let no_unf = self.index.unfinalized_lead_by_view.get(&v).map_or(true, |s| s.is_empty());
        if has_lead && no_unf {
            self.process_block_votes(1, &mut pending.tr_1, |this, b| this.is_eligible_for_tr_1_vote(b), Some("1-voted for a transaction block"), to_send);
            self.process_block_votes(2, &mut pending.tr_2, |this, b| this.is_eligible_for_tr_2_vote(b), Some("2-voted for a transaction block"), to_send);
        }
        if self.phase_i.get(&v).unwrap_or(&Phase::High) == &Phase::High {
            self.process_block_votes(1, &mut pending.lead_1, |_, b| b.view == v, None, to_send);
            self.process_block_votes(2, &mut pending.lead_2, |_, b| b.view == v, None, to_send);
        }
        pending.dirty = false;
        self.pending_votes = all;
    }

    /// Generic handler for voting on blocks
    fn process_block_votes<F>(
        &mut self,
        level: u8,
        votes: &mut BTreeMap<BlockKey, bool>,
        check: F,
        reason: Option<&str>,
        to_send: &mut Vec<(Message, Option<Identity>)>,
    ) where F: Fn(&Self, &BlockKey) -> bool {
        let mut done = Vec::new();
        for key in votes.keys().cloned() {
            if check(self, &key) {
                if self.try_vote(level, &key, None, to_send) {
                    if key.type_ == BlockType::Tr && reason.is_some() {
                        tracing_setup::protocol_transition(&self.id, "throughput phase", &Phase::High, &Phase::Low, reason);
                        self.set_phase(Phase::Low);
                    }
                    done.push(key);
                }
            }
        }
        for k in done { votes.remove(&k); }
    }

    /// Check if the block DAG has a single tip pointing to block_key
    fn block_is_single_tip(&self, block_key: &BlockKey) -> bool {
        if self.index.tips.len() != 1 { return false; }
        let tip = &self.index.tips[0];
        self.index.block_pointed_by.get(&tip.for_which)
            .map_or(false, |parents| parents.len() == 1 && parents.first().unwrap() == block_key)
    }

    /// Eligibility check for first transaction vote
    pub(crate) fn is_eligible_for_tr_1_vote(&self, block_key: &BlockKey) -> bool {
        if !self.block_is_single_tip(block_key) || !self.index.blocks.contains_key(block_key) {
            return false;
        }
        let one = &self.index.blocks[block_key].data.one.data;
        self.index.all_1qc.iter().all(|qc| one.compare_qc(&qc.data) != std::cmp::Ordering::Less)
    }

    /// Eligibility check for second transaction vote
    pub(crate) fn is_eligible_for_tr_2_vote(&self, block_key: &BlockKey) -> bool {
        let single_tip = self.index.tips.len() == 1 && self.index.tips[0].for_which == *block_key && self.index.tips[0].z == 1;
        let no_higher = self.index.max_height.0 <= block_key.height;
        single_tip && no_higher
    }
}