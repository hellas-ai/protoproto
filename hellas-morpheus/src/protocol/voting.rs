use std::cmp::Ordering;

use crate::types::{BlockId, Message, MorpheusProcess, QcId, QuorumCertificate, Vote, BlockType, ViewNum, VoteKind, Phase};

impl MorpheusProcess {
    /// Handle voting operations during a protocol step.
    ///
    /// This function:
    /// 1. Sends 0-votes for blocks
    /// 2. Creates 0-QCs for blocks with enough votes
    /// 3. Handles voting for leader blocks
    /// 4. Handles voting for transaction blocks
    ///
    /// # Arguments
    ///
    /// * `messages_to_send` - Collection of messages to be sent, which will be updated
    pub fn handle_voting(&mut self, messages_to_send: &mut Vec<Message>) -> bool {
        // According to the pseudocode, we should execute only one transition 
        // at a time, in order of priority
        
        // 1. Try to send a 0-vote for a block
        if self.handle_send_zero_vote(messages_to_send) {
            return true;
        }
        
        // 2. Try to create a 0-QC for a block
        if self.handle_create_zero_qc(messages_to_send) {
            return true;
        }
        
        // 3. Try to handle leader block voting
        if self.handle_leader_block_voting(messages_to_send) {
            return true;
        }
        
        // 4. Try to handle transaction block voting
        if self.handle_transaction_block_voting(messages_to_send) {
            return true;
        }
        
        false
    }
    
    /// Handle sending 0-votes for blocks. Returns true if a vote was sent.
    fn handle_send_zero_vote(&mut self, messages_to_send: &mut Vec<Message>) -> bool {
        // Find a block that needs a 0-vote
        for message in &self.m_i {
            if let Message::Block(b) = message {
                // Skip voting for Genesis blocks and blocks created by this process
                if b.id.block_type != BlockType::Genesis && 
                   b.id.auth != self.id && 
                   !self.has_voted(VoteKind::Zero, b.id) {
                    // Send a 0-vote for block to block's author
                    let vote = Vote {
                        vote_num: VoteKind::Zero,
                        block_id: b.id,
                        voter: self.id,
                    };
                    
                    messages_to_send.push(Message::Vote(vote));
                    self.set_voted(VoteKind::Zero, b.id);
                    return true; // Return after handling one vote
                }
            }
        }
        
        false // No vote was sent
    }
    
    /// Handle creating 0-QCs for blocks with enough votes. Returns true if a QC was created.
    fn handle_create_zero_qc(&mut self, messages_to_send: &mut Vec<Message>) -> bool {
        // Find a block that has a quorum for a 0-QC
        for message in &self.m_i {
            if let Message::Block(b) = message {
                if b.id.block_type != BlockType::Genesis && 
                   self.has_zero_quorum(b.id) && 
                   !self.sent_zero_qc.contains(&b.id) {
                    // Create a 0-QC
                    let qc_id = QcId {
                        block_id: b.id,
                    };
                    
                    let qc = QuorumCertificate {
                        id: qc_id,
                        height: b.height,
                    };
                    
                    messages_to_send.push(Message::QC(qc.clone()));
                    self.q_i.insert(qc.clone());
                    self.qcs.insert(qc_id, qc);
                    self.sent_zero_qc.insert(b.id);
                    return true; // Return after handling one QC
                }
            }
        }
        
        false // No QC was created
    }

    /// Handle voting for leader blocks.
    ///
    /// # Arguments
    ///
    /// * `messages_to_send` - Collection of messages to be sent, which will be updated
    fn handle_leader_block_voting(&mut self, messages_to_send: &mut Vec<Message>) -> bool {
        if self.get_phase(self.view_i) == Phase::High {
            // Look for a leader block to vote for
            for message in &self.m_i {
                if let Message::Block(b) = message {
                    if b.id.block_type == crate::types::BlockType::Lead && 
                       b.id.view == self.view_i && 
                       b.id.auth != self.id &&  // Skip voting for our own blocks
                       !self.has_voted(VoteKind::One, b.id) {
                        
                        // Send a 1-vote for the leader block
                        let vote = Vote {
                            vote_num: VoteKind::One,
                            block_id: b.id,
                            voter: self.id,
                        };
                        
                        messages_to_send.push(Message::Vote(vote));
                        self.set_voted(VoteKind::One, b.id);
                        return true; // Return after handling one vote
                    }
                }
            }
            
            // Look for a 1-QC for a leader block to send a 2-vote
            for qc in &self.q_i {
                if qc.id.block_id.block_type == crate::types::BlockType::Lead && 
                   qc.id.block_id.view == self.view_i && 
                   qc.id.block_id.auth != self.id &&  // Skip voting for our own blocks
                   !self.has_voted(VoteKind::Two, qc.id.block_id) {
                    
                    // Send a 2-vote for the block
                    let vote = Vote {
                        vote_num: VoteKind::Two,
                        block_id: qc.id.block_id,
                        voter: self.id,
                    };
                    
                    messages_to_send.push(Message::Vote(vote));
                    self.set_voted(VoteKind::Two, qc.id.block_id);
                    return true; // Return after handling one vote
                }
            }
        }
        
        false // No vote was sent
    }

    /// Handle voting for transaction blocks.
    ///
    /// # Arguments
    ///
    /// * `messages_to_send` - Collection of messages to be sent, which will be updated
    fn handle_transaction_block_voting(&mut self, messages_to_send: &mut Vec<Message>) -> bool {
        // First check if we have a lead block for the current view
        let has_lead_block = self.m_i.iter().any(|m| {
            if let Message::Block(b) = m {
                b.id.block_type == crate::types::BlockType::Lead && b.id.view == self.view_i
            } else {
                false
            }
        });
        
        // If we have a lead block, we can vote for transaction blocks
        if has_lead_block {
            // Check if there's an unfinalized leader block
            let has_unfinalized_lead = self.m_i.iter().any(|m| {
                if let Message::Block(b) = m {
                    b.id.block_type == crate::types::BlockType::Lead && b.id.view == self.view_i && 
                    self.count_votes(VoteKind::Two, b.id) < self.n - self.f
                } else {
                    false
                }
            });
            
            // If there's no unfinalized leader block
            if !has_unfinalized_lead {
                // Look for a transaction block to vote for
                for message in &self.m_i {
                    if let Message::Block(b) = message {
                        if b.id.block_type == crate::types::BlockType::Tr && 
                           b.id.view == self.view_i &&
                           b.id.auth != self.id { // Skip voting for our own blocks
                            // Check if it's a single tip
                            let mut is_single_tip = true;
                            for other_msg in &self.m_i {
                                if let Message::Block(other_b) = other_msg {
                                    if other_b.id != b.id && 
                                       other_b.id.block_type == crate::types::BlockType::Tr && 
                                       other_b.id.view == self.view_i && 
                                       other_b.height >= b.height {
                                        is_single_tip = false;
                                        break;
                                    }
                                }
                            }
                            
                            if is_single_tip && !self.has_voted(VoteKind::One, b.id) {
                                // Check if its 1-QC is greater than or equal to every 1-QC in Q_i
                                if let Some(one_qc_id) = b.one_qc {
                                    if let Some(one_qc) = self.find_qc_by_id(&one_qc_id) {
                                        let is_greatest = self.q_i.iter().all(|q| {
                                            self.compare_qcs(&one_qc, q) != Ordering::Less
                                        });
                                        
                                        if is_greatest {
                                            // Send a 1-vote for the transaction block
                                            let vote = Vote {
                                                vote_num: VoteKind::One,
                                                block_id: b.id,
                                                voter: self.id,
                                            };
                                            
                                            messages_to_send.push(Message::Vote(vote));
                                            self.set_voted(VoteKind::One, b.id);
                                            self.set_phase(self.view_i, Phase::Low);
                                            return true; // Return after handling one vote
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                
                // Check for a 1-QC that is a single tip of Q_i
                for qc in &self.q_i {
                    if qc.id.block_id.block_type == crate::types::BlockType::Tr && 
                       qc.id.block_id.auth != self.id && // Skip voting for our own blocks
                       self.is_single_tip(&qc) && 
                       !self.has_voted(VoteKind::Two, qc.id.block_id) {
                        
                        // Check if there's no block with greater height
                        let no_greater_height_block = self.m_i.iter().all(|m| {
                            if let Message::Block(b) = m {
                                b.height <= qc.height
                            } else {
                                true
                            }
                        });
                        
                        if no_greater_height_block {
                            // Send a 2-vote for the block
                            let vote = Vote {
                                vote_num: VoteKind::Two,
                                block_id: qc.id.block_id,
                                voter: self.id,
                            };
                            
                            messages_to_send.push(Message::Vote(vote));
                            self.set_voted(VoteKind::Two, qc.id.block_id);
                            self.set_phase(self.view_i, Phase::Low);
                            return true; // Return after handling one vote
                        }
                    }
                }
            }
        }
        
        false // No vote was sent
    }

    /// Check if the process has voted for a block.
    ///
    /// # Arguments
    ///
    /// * `vote_num` - The vote number (0, 1, or 2)
    /// * `block_id` - The ID of the block to check
    ///
    /// # Returns
    ///
    /// `true` if the process has voted for the block, `false` otherwise
    pub fn has_voted(&self, vote_num: VoteKind, block_id: BlockId) -> bool {
        *self.voted_i.get(&(vote_num, block_id)).unwrap_or(&false)
    }

    /// Mark that the process has voted for a block.
    ///
    /// # Arguments
    ///
    /// * `vote_num` - The vote number (0, 1, or 2)
    /// * `block_id` - The ID of the block voted for
    pub fn set_voted(&mut self, vote_num: VoteKind, block_id: BlockId) {
        self.voted_i.insert((vote_num, block_id), true);
    }

    /// Get the phase within a view.
    ///
    /// # Arguments
    ///
    /// * `view` - The view to get the phase for
    ///
    /// # Returns
    ///
    /// The current phase number within the specified view
    pub fn get_phase(&self, view: ViewNum) -> Phase {
        *self.phase_i.get(&view).unwrap_or(&Phase::High)
    }

    /// Set the phase within a view.
    ///
    /// # Arguments
    ///
    /// * `view` - The view to set the phase for
    /// * `phase` - The new phase number
    pub fn set_phase(&mut self, view: ViewNum, phase: Phase) {
        self.phase_i.insert(view, phase);
    }

    /// Count the number of votes for a block.
    ///
    /// # Arguments
    ///
    /// * `vote_num` - The vote number (0, 1, or 2)
    /// * `block_id` - The ID of the block to count votes for
    ///
    /// # Returns
    ///
    /// The number of votes of the specified type for the specified block
    pub fn count_votes(&self, vote_num: VoteKind, block_id: BlockId) -> usize {
        self.m_i.iter().filter(|m| {
            matches!(m, Message::Vote(vote) if vote.vote_num == vote_num && vote.block_id == block_id)
        }).count()
    }

    /// Check if a block has a 0-quorum (at least f+1 0-votes).
    ///
    /// # Arguments
    ///
    /// * `block_id` - The ID of the block to check
    ///
    /// # Returns
    ///
    /// `true` if the block has a 0-quorum, `false` otherwise
    pub fn has_zero_quorum(&self, block_id: BlockId) -> bool {
        self.count_votes(VoteKind::Zero, block_id) >= self.f + 1
    }

    /// Check if a QC is a single tip of Q_i.
    ///
    /// A QC is a single tip if no other QC in Q_i is greater according to the ordering relation.
    ///
    /// # Arguments
    ///
    /// * `q` - The QC to check
    ///
    /// # Returns
    ///
    /// `true` if the QC is a single tip, `false` otherwise
    pub fn is_single_tip(&self, q: &QuorumCertificate) -> bool {
        self.q_i.iter().all(|q_prime| {
            self.compare_qcs(q, q_prime) != Ordering::Less
        })
    }

    /// Get the greatest QC in Q_i according to the ordering relation.
    ///
    /// # Returns
    ///
    /// The greatest QC in Q_i, if any
    pub fn get_greatest_qc(&self) -> Option<QuorumCertificate> {
        self.q_i.iter().fold(None, |acc, q| {
            match acc {
                None => Some(q.clone()),
                Some(ref curr) if self.compare_qcs(curr, q) == Ordering::Less => Some(q.clone()),
                Some(curr) => Some(curr),
            }
        })
    }

    /// Get all tips of Q_i.
    ///
    /// A tip is a QC that is not less than any other QC according to the ordering relation.
    ///
    /// # Returns
    ///
    /// A vector of all tips in Q_i
    pub fn get_tips(&self) -> Vec<QuorumCertificate> {
        let mut tips = Vec::new();
        
        'outer: for q in &self.q_i {
            for q_prime in &self.q_i {
                if q != q_prime && self.compare_qcs(q, q_prime) == Ordering::Less {
                    continue 'outer;
                }
            }
            tips.push(q.clone());
        }
        
        tips
    }

    /// Find a QC by its ID.
    ///
    /// # Arguments
    ///
    /// * `qc_id` - The ID of the QC to find
    ///
    /// # Returns
    ///
    /// The QC with the specified ID, if found
    pub fn find_qc_by_id(&self, qc_id: &QcId) -> Option<QuorumCertificate> {
        self.qcs.get(qc_id).cloned()
    }
} 