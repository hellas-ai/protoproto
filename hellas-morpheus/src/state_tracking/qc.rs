use std::{cmp::Ordering, collections::BTreeSet};
use std::sync::Arc;
use crate::*;

impl MorpheusProcess {
    /// Record a quorum certificate (QC) into the state index
    pub fn record_qc(&mut self, qc: Arc<ThreshSigned<VoteData>>) {
        if self.index.qcs.contains_key(&qc.data) {
            return;
        }
        // index by slot and view
        if let Some(author) = &qc.data.for_which.author {
            self.index.qc_by_slot.insert(
                (qc.data.for_which.type_, author.clone(), qc.data.for_which.slot),
                qc.clone(),
            );
            self.index.qc_by_view.entry((
                qc.data.for_which.type_, author.clone(), qc.data.for_which.view
            )).or_default().push(qc.clone());
        }
        // mark unfinalized
        self.index.unfinalized.entry(qc.data.for_which.clone()).or_default().insert(qc.data.clone());
        if qc.data.z == 1 {
            self.index.all_1qc.insert(qc.clone());
            // update max 1-QC
            if self.index.max_1qc.data.compare_qc(&qc.data) != Ordering::Greater {
                tracing_setup::protocol_transition(&self.id, "updating max 1-QC", &self.index.max_1qc.data, &qc.data, Some("new qc is greater than current max 1-QC"));
                self.index.max_1qc = qc.clone();
            }
        }
        // update max view
        if qc.data.for_which.view > self.index.max_view.0 {
            self.index.max_view = (qc.data.for_which.view, qc.data.clone());
        }
        // maintain tips (maximal antichain)
        let mut to_remove = BTreeSet::new();
        for tip in &self.index.tips {
            if self.observes(qc.data.clone(), tip) {
                to_remove.insert(tip.clone());
                tracing::info!(target: "yeet_tip", new_tip = ?qc.data, old_tip = ?tip);
            }
        }
        if !to_remove.is_empty() {
            self.index.tips.retain(|t| !to_remove.contains(t));
            self.index.tips.push(qc.data.clone());
            tracing::info!(target: "new_tip", qc = ?qc.data);
        } else if !self.index.tips.iter().any(|tip| self.observes(tip.clone(), &qc.data)) {
            self.index.tips.push(qc.data.clone());
            tracing::info!(target: "new_tip", qc = ?qc.data);
        }
        self.index.qcs.insert(qc.data.clone(), qc.clone());
        // finalize any observed 2-QCs
        let finalized_here: Vec<_> = self.index.unfinalized_2qc.iter()
            .cloned()
            .filter(|q| self.observes(qc.data.clone(), q))
            .collect();
        if qc.data.z == 2 {
            self.index.unfinalized_2qc.insert(qc.data.clone());
        }
        for q in &finalized_here {
            self.index.unfinalized_2qc.remove(q);
            self.index.finalized.insert(q.for_which.clone(), true);
            self.index.unfinalized_lead_by_view.entry(q.for_which.view).or_default().remove(&q.for_which);
            self.index.unfinalized.remove(&q.for_which);
            self.pending_votes.entry(q.for_which.view).or_default().dirty = true;
            tracing::debug!(target: "finalized", qc = ?q);
        }
        // watch for 2-votes on 1-QCs
        if qc.data.z == 1 {
            let pending = self.pending_votes.entry(qc.data.for_which.view).or_default();
            pending.dirty = true;
            match qc.data.for_which.type_ {
                BlockType::Lead => { pending.lead_2.insert(qc.data.for_which.clone(), true); }
                BlockType::Tr => { pending.tr_2.insert(qc.data.for_which.clone(), true); }
                BlockType::Genesis => unreachable!(),
            }
        }
    }
}