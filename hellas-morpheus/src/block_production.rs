use std::sync::Arc;

use crate::*;

// Now implement the helper functions using efficient lookups
impl MorpheusProcess {
    // Implementation of the try_produce_blocks method
    pub fn try_produce_blocks(&mut self, to_send: &mut Vec<(Message, Option<Identity>)>) {
        // Check if ready to produce a transaction block
        if self.payload_ready() {
            self.make_tr_block(to_send);
        }

        // Check if this process is the leader for current view and ready to produce a leader block
        if self.id == self.lead(self.view_i)
            && self.leader_ready()
            && self.phase_i.get(&self.view_i).unwrap_or(&Phase::High) == &Phase::High
            && self.tips.len() > 1
        {
            // Only produce leader block if there's no single tip
            self.make_leader_block(to_send);
        }
    }

    // PayloadReady - Efficiently determines if ready to produce a transaction block
    fn payload_ready(&self) -> bool {
        // Check if we have transactions to process
        let has_transactions = true; // Simplified for this implementation

        // If this is not the first transaction block, ensure we have a QC for our previous block
        if self.slot_i_tr > SlotNum(0) {
            // Use our index to efficiently check for previous block QC
            let has_prev_qc = self.qc_index.contains_key(&(
                BlockType::Tr,
                self.id.clone(),
                SlotNum(self.slot_i_tr.0 - 1),
            ));

            return has_transactions && has_prev_qc;
        }

        // For the first block, we just need transactions
        has_transactions
    }

    // MakeTrBlock - Efficiently creates a transaction block
    fn make_tr_block(&mut self, to_send: &mut Vec<(Message, Option<Identity>)>) {
        // 1. Initialize block properties
        let slot = self.slot_i_tr;
        let mut prev_qcs = Vec::new();

        // 2. Set block's previous pointer - efficiently lookup previous QC
        if slot.0 > 0 {
            // Use our index for direct lookup
            if let Some(prev_qc) =
                self.qc_index
                    .get(&(BlockType::Tr, self.id.clone(), SlotNum(slot.0 - 1)))
            {
                prev_qcs.push(prev_qc.clone());
            }
        } else {
            // For the first block, use genesis QC (should be in a known location)
            for (vote_data, qc) in &self.qcs {
                if vote_data.for_which.type_ == BlockType::Genesis {
                    prev_qcs.push(qc.clone());
                    break;
                }
            }
        }

        // 3. If there's a single tip, point to it as well
        if self.tips.len() == 1 {
            let tip = &self.tips[0];
            let tip_qc = self.qcs.get(tip).unwrap();

            // Don't add duplicate QC
            if !prev_qcs
                .iter()
                .any(|qc| qc.data.for_which == tip_qc.data.for_which)
            {
                prev_qcs.push(tip_qc.clone());
            }
        }

        // 4. Calculate block height
        let height = prev_qcs
            .iter()
            .map(|qc| qc.data.for_which.height)
            .max()
            .unwrap_or(0)
            + 1;

        // 5. Use max_1qc which is already efficiently tracked
        let max_1qc = self.max_1qc.clone();

        // Create block key
        let block_key = BlockKey {
            type_: BlockType::Tr,
            view: self.view_i,
            height,
            author: Some(self.id.clone()),
            slot,
            hash: Some(BlockHash(self.slot_i_tr.0)), // Simplified hash generation
        };

        // Create block with sample transaction
        let block = Arc::new(Block {
            key: block_key.clone(),
            prev: prev_qcs,
            one: max_1qc,
            data: BlockData::Tr {
                transactions: vec![Transaction::Opaque(vec![0, 1, 2])], // Sample transaction
            },
        });

        // 6. Sign and send block
        let signed_block = Signed {
            data: block,
            author: self.id.clone(),
            signature: Signature {},
        };

        to_send.push((Message::Block(signed_block), None));

        // 7. Update transaction slot
        self.slot_i_tr = SlotNum(self.slot_i_tr.0 + 1);
    }

    // LeaderReady - Efficiently determines if leader is ready to produce a block
    fn leader_ready(&self) -> bool {
        let view = self.view_i;
        let slot = self.slot_i_lead;

        // Case 1: First leader block of the view - efficient lookup using our index
        let has_produced_lead_block = self
            .produced_lead_in_view
            .get(&view)
            .copied()
            .unwrap_or(false);

        if !has_produced_lead_block {
            // Check if we have received enough view messages
            let has_enough_view_messages = self
                .start_views
                .get(&view)
                .map(|msgs| msgs.len() >= self.n - self.f)
                .unwrap_or(false);

            // Check for previous leader block QC if not at slot 0
            let has_prev_qc = if slot.0 > 0 {
                self.qc_index
                    .contains_key(&(BlockType::Lead, self.id.clone(), SlotNum(slot.0 - 1)))
            } else {
                true
            };

            return has_enough_view_messages && has_prev_qc;
        }

        // Case 2: Subsequent leader blocks in the view
        if has_produced_lead_block && slot.0 > 0 {
            // Efficiently lookup 1-QC for previous leader block using our index
            let prev_qcs = self
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

    // MakeLeaderBlock - Efficiently creates a leader block
    fn make_leader_block(&mut self, to_send: &mut Vec<(Message, Option<Identity>)>) {
        // 1. Initialize block properties
        let slot = self.slot_i_lead;
        let view = self.view_i;

        // 2. Initially set prev to tips - already efficiently managed
        let mut prev_qcs: Vec<ThreshSigned<VoteData>> = self
            .tips
            .iter()
            .filter_map(|tip| self.qcs.get(tip).cloned())
            .collect();

        // 3. Add pointer to previous leader block if applicable - efficient lookup
        if slot.0 > 0 {
            if let Some(prev_qc) =
                self.qc_index
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

        // 4. Calculate block height
        let height = prev_qcs
            .iter()
            .map(|qc| qc.data.for_which.height)
            .max()
            .unwrap_or(0)
            + 1;

        // Check if this is the first leader block for this view - efficient lookup
        let has_produced_lead_block = self
            .produced_lead_in_view
            .get(&view)
            .copied()
            .unwrap_or(false);

        let (one_qc, justification) = if !has_produced_lead_block {
            // 5a. For first leader block, include justification and set 1-QC
            let view_messages = self.start_views.get(&view).cloned().unwrap_or_default();

            // Find max 1-QC from view messages
            let max_qc = view_messages
                .iter()
                .map(|msg| &msg.data.qc)
                .max_by(|a, b| a.data.compare_qc(&b.data))
                .cloned()
                .unwrap_or(self.max_1qc.clone());

            (max_qc, view_messages)
        } else {
            // 6. For subsequent blocks, efficiently lookup 1-QC
            let prev_qcs = self
                .qc_by_view
                .get(&(BlockType::Lead, self.id.clone(), view));
            let prev_leader_qc = prev_qcs
                .and_then(|qcs| {
                    qcs.iter()
                        .find(|qc| qc.data.z == 1 && qc.data.for_which.slot == SlotNum(slot.0 - 1))
                })
                .cloned()
                .unwrap_or(self.max_1qc.clone());

            (prev_leader_qc, vec![])
        };

        // Create block key
        let block_key = BlockKey {
            type_: BlockType::Lead,
            view,
            height,
            author: Some(self.id.clone()),
            slot,
            hash: Some(BlockHash(self.slot_i_lead.0)), // Simplified hash generation
        };

        // Create block
        let block = Arc::new(Block {
            key: block_key.clone(),
            prev: prev_qcs,
            one: one_qc,
            data: BlockData::Lead { justification },
        });

        // 7. Sign and send block
        let signed_block = Signed {
            data: block,
            author: self.id.clone(),
            signature: Signature {},
        };

        to_send.push((Message::Block(signed_block), None));

        // 8. Update leader slot
        self.slot_i_lead = SlotNum(self.slot_i_lead.0 + 1);
    }
}
