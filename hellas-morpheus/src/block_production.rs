use std::sync::Arc;

use crate::*;

impl MorpheusProcess {
    pub fn try_produce_blocks(&mut self, to_send: &mut Vec<(Message, Option<Identity>)>) {
        if self.payload_ready() {
            self.make_tr_block(to_send);
        }

        if self.id == self.lead(self.view_i)
            && self.leader_ready()
            && self.phase_i.get(&self.view_i).unwrap_or(&Phase::High) == &Phase::High
            && self.index.tips.len() > 1
        {
            self.make_leader_block(to_send);
        }
    }

    fn payload_ready(&self) -> bool {
        let has_transactions = !self.ready_transactions.is_empty();

        if self.slot_i_tr > SlotNum(0) {
            let has_prev_qc = self.index.qc_by_slot.contains_key(&(
                BlockType::Tr,
                self.id.clone(),
                SlotNum(self.slot_i_tr.0 - 1),
            ));

            return has_transactions && has_prev_qc;
        }

        has_transactions
    }

    fn make_tr_block(&mut self, to_send: &mut Vec<(Message, Option<Identity>)>) {
        let slot = self.slot_i_tr;
        let mut prev_qcs = Vec::new();

        if slot.0 > 0 {
            if let Some(prev_qc) =
                self.index
                    .qc_by_slot
                    .get(&(BlockType::Tr, self.id.clone(), SlotNum(slot.0 - 1)))
            {
                prev_qcs.push(prev_qc.clone());
            }
        } else {
            prev_qcs.push(self.genesis_qc.clone());
        }

        // If there's a single tip, point to it as well
        if self.index.tips.len() == 1 {
            let tip = &self.index.tips[0];
            let tip_qc = self.index.qcs.get(tip).unwrap();

            // Don't add duplicate QC
            if !prev_qcs
                .iter()
                .any(|qc| qc.data.for_which == tip_qc.data.for_which)
            {
                prev_qcs.push(tip_qc.clone());
            }
        }

        let height = prev_qcs
            .iter()
            .map(|qc| qc.data.for_which.height)
            .max()
            .unwrap_or(0)
            + 1;

        let max_1qc = self.index.max_1qc.clone();

        let block_key = BlockKey {
            type_: BlockType::Tr,
            view: self.view_i,
            height,
            author: Some(self.id.clone()),
            slot,
            hash: Some(BlockHash(self.id.0 * 0x100 + self.slot_i_tr.0)),
        };

        let block = Block {
            key: block_key.clone(),
            prev: prev_qcs.iter().map(|qc| ThreshSigned::clone(qc)).collect(),
            one: ThreshSigned::clone(&max_1qc),
            data: BlockData::Tr {
                transactions: std::mem::take(&mut self.ready_transactions),
            },
        };

        let signed_block = Arc::new(Signed {
            data: block.clone(),
            author: self.id.clone(),
            signature: Signature {},
        });

        self.slot_i_tr = SlotNum(self.slot_i_tr.0 + 1);
        crate::tracing_setup::block_created(&self.id, "transaction", &block.key);

        self.send_msg(to_send, (Message::Block(signed_block.clone()), None));
    }

    fn leader_ready(&self) -> bool {
        let view = self.view_i;
        let slot = self.slot_i_lead;

        let has_produced_lead_block = self
            .produced_lead_in_view
            .get(&view)
            .copied()
            .unwrap_or(false);

        if !has_produced_lead_block {
            let has_enough_view_messages = self
                .start_views
                .get(&view)
                .map(|msgs| msgs.len() >= self.n - self.f)
                .unwrap_or(false);

            // Check for previous leader block QC if not at slot 0
            let has_prev_qc = if slot.0 > 0 {
                self.index.qc_by_slot.contains_key(&(
                    BlockType::Lead,
                    self.id.clone(),
                    SlotNum(slot.0 - 1),
                ))
            } else {
                true
            };

            return has_enough_view_messages && has_prev_qc;
        }

        if has_produced_lead_block && slot.0 > 0 {
            let prev_qcs = self
                .index
                .qc_by_view
                .get(&(BlockType::Lead, self.id.clone(), view));

            if let Some(prev_qcs) = prev_qcs {
                return prev_qcs
                    .iter()
                    .any(|qc| qc.data.z == 1 && qc.data.for_which.slot == SlotNum(slot.0 - 1));
            }
        }

        false
    }

    fn make_leader_block(&mut self, to_send: &mut Vec<(Message, Option<Identity>)>) {
        let slot = self.slot_i_lead;
        let view = self.view_i;

        let mut prev_qcs: Vec<Arc<ThreshSigned<VoteData>>> = self
            .index
            .tips
            .iter()
            .filter_map(|tip| self.index.qcs.get(tip).cloned())
            .collect();

        if slot.0 > 0 {
            if let Some(prev_qc) =
                self.index
                    .qc_by_slot
                    .get(&(BlockType::Lead, self.id.clone(), SlotNum(slot.0 - 1)))
            {
                if !prev_qcs
                    .iter()
                    .any(|qc| qc.data.for_which == prev_qc.data.for_which)
                {
                    prev_qcs.push(prev_qc.clone());
                }
            }
        }

        let height = prev_qcs
            .iter()
            .map(|qc| qc.data.for_which.height)
            .max()
            .unwrap_or(0)
            + 1;

        let has_produced_lead_block = self
            .produced_lead_in_view
            .get(&view)
            .copied()
            .unwrap_or(false);

        let (one_qc, justification) = if !has_produced_lead_block {
            let view_messages = self.start_views.get(&view).cloned().unwrap_or_default();
            let view_messages = view_messages
                .into_iter()
                .map(|msg| Signed::clone(&msg))
                .collect::<Vec<_>>();

            let max_qc = view_messages
                .iter()
                .map(|msg| &msg.data.qc)
                .max_by(|a, b| a.data.compare_qc(&b.data))
                .cloned()
                .unwrap_or_else(|| ThreshSigned::clone(&self.index.max_1qc));

            (max_qc, view_messages)
        } else {
            let prev_qcs = self
                .index
                .qc_by_view
                .get(&(BlockType::Lead, self.id.clone(), view));
            let prev_leader_qc = prev_qcs
                .and_then(|qcs| {
                    qcs.iter()
                        .find(|qc| qc.data.z == 1 && qc.data.for_which.slot == SlotNum(slot.0 - 1))
                })
                .map(|qc| ThreshSigned::clone(qc))
                .unwrap_or_else(|| ThreshSigned::clone(&self.index.max_1qc));

            (prev_leader_qc, vec![])
        };

        let block_key = BlockKey {
            type_: BlockType::Lead,
            view,
            height,
            author: Some(self.id.clone()),
            slot,
            hash: Some(BlockHash(self.slot_i_lead.0)),
        };

        let block = Block {
            key: block_key.clone(),
            prev: prev_qcs.iter().map(|qc| ThreshSigned::clone(qc)).collect(),
            one: one_qc,
            data: BlockData::Lead { justification },
        };

        crate::tracing_setup::block_created(&self.id, "leader", &block.key);

        let signed_block = Arc::new(Signed {
            data: block,
            author: self.id.clone(),
            signature: Signature {},
        });

        self.send_msg(to_send, (Message::Block(signed_block), None));

        self.slot_i_lead = SlotNum(self.slot_i_lead.0 + 1);
    }
}
