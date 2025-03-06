use crate::types::{Message, MorpheusProcess, ViewMessage, EndViewMessage, QuorumCertificate};

impl MorpheusProcess {
    /// Handle view updates during a protocol step.
    ///
    /// This function:
    /// 1. Checks for view changes based on end-view messages
    /// 2. Checks for view changes based on certificates
    /// 3. Updates the process's view state as needed
    ///
    /// # Arguments
    ///
    /// * `messages_to_send` - Collection of messages to be sent, which will be updated
    pub fn handle_view_updates(&mut self, messages_to_send: &mut Vec<Message>) -> bool {
        // 1. Check for enough end-view messages to trigger a view change
        if let Some(v) = self.find_greatest_view_with_enough_end_view_messages() {
            // Form a (v + 1)-certificate and send it to all processes
            let view_message = ViewMessage {
                view: v + 1,
                qc_id: None,
                sender: self.id,
            };
            
            messages_to_send.push(Message::ViewMsg(view_message));
            return true;
        }
        
        // 2. Check for certificates for a greater view
        if let Some((v, q)) = self.find_greatest_view_with_certificate() {
            // Update view
            self.view_i = v;
            self.view_entry_time = std::time::Instant::now();
            
            // Send q to all processes
            messages_to_send.push(Message::QC(q.clone()));
            
            // Send all tips q' of Q_i such that q'.auth = p_i to lead(v)
            for q_prime in self.get_tips() {
                if q_prime.id.block_id.auth == self.id {
                    messages_to_send.push(Message::QC(q_prime.clone()));
                }
            }
            
            // Send (v, q') signed by p_i to lead(v), where q' is maximal amongst 1-QCs seen by p_i
            if let Some(q_prime) = self.get_greatest_qc() {
                let view_message = ViewMessage {
                    view: v,
                    qc_id: Some(q_prime.id),
                    sender: self.id,
                };
                
                messages_to_send.push(Message::ViewMsg(view_message));
            }
            
            return true;
        }
        
        false
    }

    /// Handle complaints during a protocol step.
    ///
    /// This function:
    /// 1. Checks if the process has been in the current view too long
    /// 2. Sends appropriate complaint messages if needed
    ///
    /// # Arguments
    ///
    /// * `messages_to_send` - Collection of messages to be sent, which will be updated
    pub fn handle_complaints(&mut self, messages_to_send: &mut Vec<Message>) -> bool {
        // Check for QCs that have not been finalized for too long
        let elapsed = self.view_entry_time.elapsed();
        let delta = std::time::Duration::from_millis(100); // Arbitrary value for testing
        
        // Check for QCs that have not been finalized for time 6Δ
        if elapsed >= delta.mul_f32(6.0) {
            if let Some(q) = self.find_maximal_unfinalized_qc() {
                let lead = self.lead(self.view_i);
                
                // Send q to lead(view_i) if not previously sent
                if lead != self.id {
                    messages_to_send.push(Message::QC(q.clone()));
                    return true;
                }
            }
        }
        
        // Check for QCs that have not been finalized for time 12Δ
        if elapsed >= delta.mul_f32(12.0) {
            let end_view = EndViewMessage {
                view: self.view_i,
                sender: self.id,
            };
            
            messages_to_send.push(Message::EndViewMsg(end_view));
            return true;
        }
        
        false
    }

    /// Find the greatest view with enough end-view messages.
    ///
    /// # Returns
    ///
    /// The greatest view with at least f+1 end-view messages, if any
    pub fn find_greatest_view_with_enough_end_view_messages(&self) -> Option<usize> {
        let mut views_with_enough_messages = Vec::new();
        
        for message in &self.m_i {
            if let Message::EndViewMsg(msg) = message {
                if self.count_end_view_messages(msg.view) >= self.f + 1 && msg.view >= self.view_i {
                    views_with_enough_messages.push(msg.view);
                }
            }
        }
        
        views_with_enough_messages.into_iter().max()
    }

    /// Find the greatest view with a certificate.
    ///
    /// # Returns
    ///
    /// The greatest view (greater than the current view) with a certificate, along with the certificate
    pub fn find_greatest_view_with_certificate(&self) -> Option<(usize, crate::types::QuorumCertificate)> {
        let mut greatest_view = self.view_i;
        let mut cert = None;
        
        // Check M_i for certificates
        for message in &self.m_i {
            if let Message::QC(qc) = message {
                if qc.id.block_id.view > greatest_view {
                    greatest_view = qc.id.block_id.view;
                    cert = Some(qc.clone());
                }
            }
        }
        
        // Check Q_i for certificates
        for qc in &self.q_i {
            if qc.id.block_id.view > greatest_view {
                greatest_view = qc.id.block_id.view;
                cert = Some(qc.clone());
            }
        }
        
        if greatest_view > self.view_i {
            cert.map(|c| (greatest_view, c))
        } else {
            None
        }
    }

    /// Count the number of end-view messages for a view.
    ///
    /// # Arguments
    ///
    /// * `view` - The view to count end-view messages for
    ///
    /// # Returns
    ///
    /// The number of end-view messages for the specified view
    pub fn count_end_view_messages(&self, view: usize) -> usize {
        self.m_i.iter().filter(|m| {
            matches!(m, Message::EndViewMsg(msg) if msg.view == view)
        }).count()
    }

    /// Find the maximal QC that has not been finalized.
    ///
    /// # Returns
    ///
    /// The maximal QC that has not been finalized, if any
    fn find_maximal_unfinalized_qc(&self) -> Option<&QuorumCertificate> {
        // Find maximal QC according to the ordering relation
        self.q_i.iter().max_by(|q1, q2| self.compare_qcs(q1, q2))
    }
} 