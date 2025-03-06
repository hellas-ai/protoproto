use crate::types::{Block, BlockId, BlockType, Message, MorpheusProcess, Phase, QcId, SlotNum};

impl MorpheusProcess {
    /// Handle block creation during a protocol step.
    ///
    /// This function:
    /// 1. Creates new transaction blocks if ready
    /// 2. Creates new leader blocks if ready
    ///
    /// # Arguments
    ///
    /// * `messages_to_send` - Collection of messages to be sent, which will be updated
    pub fn handle_block_creation(&mut self, messages_to_send: &mut Vec<Message>) -> bool {
        // 1. Create new transaction blocks
        if self.payload_ready() {
            let block = self.make_tr_block();
            messages_to_send.push(Message::Block(block));
            return true;
        }
        
        // 2. Create new leader blocks
        if self.id == self.lead(self.view_i) && 
           self.leader_ready() && 
           self.get_phase(self.view_i) == Phase::High && 
           !self.q_i.iter().any(|q| self.is_single_tip(q)) {
            let block = self.make_leader_block();
            messages_to_send.push(Message::Block(block));
            return true;
        }
        
        false
    }

    /// Find a block by its ID.
    ///
    /// # Arguments
    ///
    /// * `block_id` - The ID of the block to find
    ///
    /// # Returns
    ///
    /// The block with the specified ID, if found
    pub fn find_block(&self, block_id: BlockId) -> Option<Block> {
        self.blocks.get(&block_id).cloned()
    }

    /// Check if the leader is ready to produce a block.
    ///
    /// # Returns
    ///
    /// `true` if the leader is ready to produce a block, `false` otherwise
    pub fn leader_ready(&self) -> bool {
        let v = self.view_i;
        
        // Check if we are the leader
        if self.id != self.lead(v) {
            return false;
        }
        
        // Case 1: First leader block of the view
        let has_produced_leader_block = self.m_i.iter().any(|m| {
            if let Message::Block(b) = m {
                b.id.auth == self.id && b.id.block_type == BlockType::Lead && b.id.view == v
            } else {
                false
            }
        });
        
        if !has_produced_leader_block {
            // Count view messages
            let view_messages_count = self.m_i.iter().filter(|m| {
                matches!(m, Message::ViewMsg(vm) if vm.view == v)
            }).count();
            
            // Check if we have enough view messages and either slot is 0 or we have the previous leader block's QC
            return view_messages_count >= self.n - self.f && 
                (self.slot_i[&BlockType::Lead].is_initial() || 
                 self.q_i.iter().any(|q| {
                     q.id.block_id.auth == self.id && 
                     q.id.block_id.block_type == BlockType::Lead && 
                     q.id.block_id.slot.is_pred(self.slot_i[&BlockType::Lead])
                 }));
        }
        
        // Case 2: Subsequent leader blocks in the view
        self.q_i.iter().any(|q| {
            q.id.block_id.auth == self.id && 
            q.id.block_id.block_type == BlockType::Lead && 
            q.id.block_id.slot.is_pred(self.slot_i[&BlockType::Lead])
        })
    }

    /// Check if the process is ready to produce a transaction block.
    ///
    /// # Returns
    ///
    /// `true` if the process is ready to produce a transaction block, `false` otherwise
    pub fn payload_ready(&self) -> bool {
        let s = self.slot_i[&BlockType::Tr];
        
        // If slot is 0, we can produce a transaction block
        if s.is_initial() {
            return true;
        }
        
        // Check if we have a QC for our previous transaction block
        self.q_i.iter().any(|q| {
            q.id.block_id.auth == self.id && 
            q.id.block_id.block_type == BlockType::Tr && 
            q.id.block_id.slot.is_pred(s)
        })
    }

    /// Create a new transaction block.
    ///
    /// # Returns
    ///
    /// The newly created transaction block
    pub fn make_tr_block(&mut self) -> Block {
        let s = self.slot_i[&BlockType::Tr];
        let mut prev_qcs = Vec::new();
        
        // Set block's previous pointer
        if !s.is_initial() {
            // Find q_1
            for q in &self.q_i {
                if q.id.block_id.auth == self.id && 
                   q.id.block_id.block_type == BlockType::Tr && 
                   q.id.block_id.slot.is_pred(s) {
                    prev_qcs.push(q.id);
                    break;
                }
            }
        }
        
        // If there's a single tip, point to it as well
        for q in &self.q_i {
            if self.is_single_tip(q) {
                // Only add if not already in prev_qcs
                if !prev_qcs.contains(&q.id) {
                    prev_qcs.push(q.id);
                }
                break;
            }
        }
        
        // Set block height
        let height = prev_qcs.iter()
            .filter_map(|qc_id| self.find_qc_by_id(qc_id))
            .map(|qc| qc.height)
            .max()
            .unwrap_or(0) + 1;
        
        // Set 1-QC to the greatest 1-QC seen
        let one_qc = self.get_greatest_qc().map(|qc| qc.id);
        
        // Create the block ID
        let block_id = BlockId {
            block_type: BlockType::Tr,
            auth: self.id,
            view: self.view_i,
            slot: s,
        };
        
        // Create the block
        let block = Block {
            id: block_id,
            height,
            prev_qcs,
            one_qc,
            justification: Vec::new(),
        };
        
        // Update transaction slot
        self.slot_i.insert(BlockType::Tr, s.incr());
        
        // Store the block
        self.blocks.insert(block_id, block.clone());
        
        // Add block to M_i
        self.m_i.insert(Message::Block(block.clone()));
        
        block
    }

    /// Create a new leader block.
    ///
    /// # Returns
    ///
    /// The newly created leader block
    pub fn make_leader_block(&mut self) -> Block {
        let s = self.slot_i[&BlockType::Lead];
        let v = self.view_i;
        
        // Initially set prev to tips
        let mut prev_qcs = Vec::new();
        for qc in self.get_tips() {
            prev_qcs.push(qc.id);
        }
        
        // Add pointer to previous leader block
        if !s.is_initial() {
            for q in &self.q_i {
                if q.id.block_id.auth == self.id && 
                   q.id.block_id.block_type == BlockType::Lead && 
                   q.id.block_id.slot.is_pred(s) {
                    // Only add if not already in prev_qcs
                    if !prev_qcs.contains(&q.id) {
                        prev_qcs.push(q.id);
                    }
                    break;
                }
            }
        }
        
        // Set block height
        let height = prev_qcs.iter()
            .filter_map(|qc_id| self.find_qc_by_id(qc_id))
            .map(|qc| qc.height)
            .max()
            .unwrap_or(0) + 1;
        
        let has_produced_leader_block = self.m_i.iter().any(|m| {
            if let Message::Block(b) = m {
                b.id.auth == self.id && b.id.block_type == BlockType::Lead && b.id.view == v
            } else {
                false
            }
        });
        
        let mut one_qc = None;
        let mut justification = Vec::new();
        
        // Handle the first leader block of the view differently
        if !has_produced_leader_block {
            // Collect view messages for justification
            for message in &self.m_i {
                if let Message::ViewMsg(vm) = message {
                    if vm.view == v {
                        justification.push((vm.view, vm.sender));
                    }
                }
            }
            
            // Set 1-QC to be greater than or equal to all 1-QCs in justification messages
            let mut max_qc = None;
            
            for message in &self.m_i {
                if let Message::ViewMsg(vm) = message {
                    if vm.view == v {
                        if let Some(ref qc_id) = vm.qc_id {
                            if let Some(qc) = self.find_qc_by_id(qc_id) {
                                match max_qc {
                                    None => max_qc = Some(qc),
                                    Some(ref m) => {
                                        if self.compare_qcs(m, &qc) == std::cmp::Ordering::Less {
                                            max_qc = Some(qc);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            
            one_qc = max_qc.map(|qc| qc.id);
        } else {
            // Handle subsequent leader blocks in the view
            for q in &self.q_i {
                if q.id.block_id.auth == self.id && 
                   q.id.block_id.block_type == BlockType::Lead && 
                   q.id.block_id.slot.is_pred(s) {
                    one_qc = Some(q.id);
                    break;
                }
            }
        }
        
        // Create the block ID
        let block_id = BlockId {
            block_type: BlockType::Lead,
            auth: self.id,
            view: v,
            slot: s,
        };
        
        // Create the block
        let block = Block {
            id: block_id,
            height,
            prev_qcs,
            one_qc,
            justification,
        };
        
        // Update leader slot
        self.slot_i.insert(BlockType::Lead, s.incr());
        
        // Store the block
        self.blocks.insert(block_id, block.clone());
        
        // Add block to M_i
        self.m_i.insert(Message::Block(block.clone()));
        
        block
    }
} 