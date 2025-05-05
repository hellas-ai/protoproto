use std::sync::Arc;
use crate::*;

impl MorpheusProcess {
    /// Check if payload (transactions and previous QC) is ready
    fn payload_ready(&self) -> bool {
        let has_tx = !self.ready_transactions.is_empty();
        if self.slot_i_tr.0 > 0 {
            let prev = (BlockType::Tr, self.id.clone(), SlotNum(self.slot_i_tr.0 - 1));
            return has_tx && self.index.qc_by_slot.contains_key(&prev);
        }
        has_tx
    }

    /// Produce and broadcast a transaction block
    fn make_tr_block(&mut self, to_send: &mut Vec<(Message, Option<Identity>)>) {
        let slot = self.slot_i_tr;
        let mut prev_qcs = Vec::new();
        if slot.0 > 0 {
            let key = (BlockType::Tr, self.id.clone(), SlotNum(slot.0 - 1));
            if let Some(qc) = self.index.qc_by_slot.get(&key) {
                prev_qcs.push(qc.clone());
            }
        } else {
            prev_qcs.push(self.genesis_qc.clone());
        }
        if self.index.tips.len() == 1 {
            let tip = &self.index.tips[0];
            if let Some(tip_qc) = self.index.qcs.get(tip) {
                if !prev_qcs.iter().any(|qc| qc.data.for_which == tip_qc.data.for_which) {
                    prev_qcs.push(tip_qc.clone());
                }
            }
        }
        let height = prev_qcs.iter().map(|qc| qc.data.for_which.height).max().unwrap_or(0) + 1;
        let block_key = BlockKey { type_: BlockType::Tr, view: self.view_i, height, author: Some(self.id.clone()), slot, hash: Some(BlockHash(self.id.0 * 0x100 + slot.0)) };
        let block = Block {
            key: block_key.clone(),
            prev: prev_qcs.iter().cloned().collect(),
            one: self.index.max_1qc.clone(),
            data: BlockData::Tr { transactions: std::mem::take(&mut self.ready_transactions) },
        };
        crate::tracing_setup::block_created(&self.id, "transaction", &block.key);
        let signed = Arc::new(Signed::from_data(block, &self.kb));
        self.slot_i_tr = SlotNum(slot.0 + 1);
        self.send_msg(to_send, (Message::Block(signed), None));
    }
}