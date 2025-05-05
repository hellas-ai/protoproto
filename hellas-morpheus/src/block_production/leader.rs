use std::sync::Arc;
use crate::*;

impl MorpheusProcess {
    /// Check if leader conditions are met for new leader block
    fn leader_ready(&self) -> bool {
        let view = self.view_i;
        let slot = self.slot_i_lead;
        let produced = *self.produced_lead_in_view.get(&view).unwrap_or(&false);
        if !produced {
            let msgs = self.start_views.get(&view).map_or(0, |v| v.len());
            let prev_ok = if slot.0 > 0 {
                self.index.qc_by_slot.contains_key(&(BlockType::Lead, self.id.clone(), SlotNum(slot.0 - 1)))
            } else { true };
            return msgs >= self.n - self.f && prev_ok;
        }
        if produced && slot.0 > 0 {
            if let Some(qcs) = self.index.qc_by_view.get(&(BlockType::Lead, self.id.clone(), view)) {
                return qcs.iter().any(|qc| qc.data.z == 1 && qc.data.for_which.slot == SlotNum(slot.0 - 1));
            }
        }
        false
    }

    /// Produce and broadcast a leader block
    fn make_leader_block(&mut self, to_send: &mut Vec<(Message, Option<Identity>)>) {
        let view = self.view_i;
        let slot = self.slot_i_lead;
        let mut prev_qcs: Vec<Arc<ThreshSigned<VoteData>>> = self.index.tips.iter().filter_map(|tip| self.index.qcs.get(tip).cloned()).collect();
        if slot.0 > 0 {
            let key = (BlockType::Lead, self.id.clone(), SlotNum(slot.0 - 1));
            if let Some(qc) = self.index.qc_by_slot.get(&key) {
                if !prev_qcs.iter().any(|c| c.data.for_which == qc.data.for_which) {
                    prev_qcs.push(qc.clone());
                }
            }
        }
        let height = prev_qcs.iter().map(|qc| qc.data.for_which.height).max().unwrap_or(0) + 1;
        let (one_qc, justification) = if !*self.produced_lead_in_view.get(&view).unwrap_or(&false) {
            let msgs = self.start_views.get(&view).cloned().unwrap_or_default();
            let max1 = msgs.iter().map(|m| &m.data.qc).max_by(|a, b| a.data.compare_qc(&b.data)).cloned().unwrap_or_else(|| self.index.max_1qc.clone());
            (max1, msgs)
        } else {
            let prev = self.index.qc_by_view.get(&(BlockType::Lead, self.id.clone(), view)).unwrap_or(&Vec::new());
            let prev1 = prev.iter().find(|qc| qc.data.z == 1 && qc.data.for_which.slot == SlotNum(slot.0 - 1)).cloned().unwrap_or_else(|| self.index.max_1qc.clone());
            (prev1, Vec::new())
        };
        let block_key = BlockKey { type_: BlockType::Lead, view, height, author: Some(self.id.clone()), slot, hash: Some(BlockHash(slot.0)) };
        let block = Block { key: block_key.clone(), prev: prev_qcs.iter().cloned().collect(), one: one_qc, data: BlockData::Lead { justification } };
        crate::tracing_setup::block_created(&self.id, "leader", &block.key);
        let signed = Arc::new(Signed::from_data(block, &self.kb));
        self.send_msg(to_send, (Message::Block(signed), None));
        self.slot_i_lead = SlotNum(slot.0 + 1);
    }
}