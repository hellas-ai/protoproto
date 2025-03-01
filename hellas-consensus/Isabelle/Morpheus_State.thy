theory Morpheus_State
  imports Morpheus_Procedures
begin

(* 
 * Complete state transitions matching Algorithm 1
 *)

(* Update view based on end-view messages and certificates *)
definition handle_view_update :: "nat ⇒ 'tx process_local_state ⇒ 'tx process_local_state × bool" where
  "handle_view_update n state = 
  (let view_i = view_i state;
       M_i = Mi state;
       Q_i = Qi state;
       
       (* Check for f+1 end-view messages *)
       end_view_msgs = {v | v p b. (p, b) ∈ set_mset M_i ∧ 
                              b_type b = End_View_Message ∧ v ≥ view_i};
       max_end_view_v = (if end_view_msgs = {} then 0 
                         else Max end_view_msgs);
       
       (* Form (v+1)-certificate if needed *)
       form_cert = (max_end_view_v ≥ view_i);
       
       (* Check for view certificate or QC with higher view *)
       cert_views = {v | v p b. (p, b) ∈ set_mset M_i ∧ 
                           b_type b = View_Message ∧ v > view_i};
       qc_views = {q_view q | q. q ∈ Q_i ∧ q_view q > view_i};
       
       max_cert_view = (if cert_views = {} then 0 else Max cert_views);
       max_qc_view = (if qc_views = {} then 0 else Max qc_views);
       max_view = max max_cert_view max_qc_view;
       
       (* Update view if needed *)
       update_view = max_view > view_i;
       new_view = (if update_view then max_view else view_i);
       
       (* Update state *)
       new_state = (if update_view then 
                      state\<lparr>view_i := new_view\<rparr> 
                    else 
                      state)
   in (new_state, form_cert ∨ update_view))"

(* Send 0-votes for blocks *)
definition send_0_votes :: "nat ⇒ 'tx process_local_state ⇒ 'tx process_local_state × bool" where
  "send_0_votes n state = 
  (let M_i = Mi state;
       voted_i = voted_i state;
       
       (* Find blocks that need 0-votes *)
       need_vote_blocks = {(pid, b) ∈ set_mset M_i | 
                           ¬voted_i 0 (b_type b) (b_slot b) (b_auth b)};
       
       (* Update voted function *)
       new_voted = (if need_vote_blocks = {} then 
                     voted_i 
                   else 
                     (λz x s p. if z = 0 ∧ x = b_type b ∧ s = b_slot b ∧ p = b_auth b ∧
                                (SOME (pid, b) ∈ need_vote_blocks) 
                                then True 
                                else voted_i z x s p));
       
       (* Update state *)
       new_state = state\<lparr>voted_i := new_voted\<rparr>
   in (new_state, need_vote_blocks ≠ {}))"

(* Send 0-QCs for blocks *)
definition send_0_QCs :: "nat ⇒ 'tx process_local_state ⇒ 'tx process_local_state × bool" where
  "send_0_QCs n state = 
  (let M_i = Mi state;
       Q_i = Qi state;
       
       (* Find blocks with 0-quorums where auth = p_i *)
       blocks_with_quorums = {b | b pid. (pid, b) ∈ set_mset M_i ∧ 
                                    b_auth b = n ∧
                                    (∃qs. card qs ≥ n - (n div 3) ∧
                                         (∀q ∈ qs. q_z q = 0 ∧ q_b q = b))};
       
       (* Check if 0-QC already sent *)
       need_qc = {b ∈ blocks_with_quorums | ¬(∃q ∈ Q_i. q_z q = 0 ∧ q_b q = b)};
       
       (* Update QCs *)
       new_QCs = (if need_qc = {} then 
                   Q_i 
                 else
                   Q_i ∪ {QC 0 (b_type b) (b_view b) (b_h b) (b_auth b) (b_slot b) b | 
                          b. b ∈ need_qc});
       
       (* Update state *)
       new_state = state\<lparr>Qi := new_QCs\<rparr>
   in (new_state, need_qc ≠ {}))"

(* Handle transaction block creation *)
definition handle_transaction :: "nat ⇒ 'tx list ⇒ 'tx process_local_state ⇒ 'tx process_local_state × bool" where
  "handle_transaction n txs state =
  (let ready = payload_ready n state in
   if ready then
     let (new_block, temp_state) = make_tr_block n txs state;
         new_M_i = add_mset (n, new_block) (Mi temp_state)
     in (temp_state\<lparr>Mi := new_M_i\<rparr>, True)
   else
     (state, False))"

(* Handle leader block creation *)
definition handle_leader :: "nat ⇒ 'tx process_local_state ⇒ 'tx process_local_state × bool" where
  "handle_leader n state =
  (let view_i = view_i state;
       phase_i = phase_i state;
       Q_i = Qi state;
       ready = leader_ready n state;
       is_leader = (n = lead n view_i);
       phase_0 = ¬(phase_i view_i);
       has_single_tip = (∃q ∈ Q_i. is_single_tip_qc Q_i (Mi state) q)
   in
   if is_leader ∧ ready ∧ phase_0 ∧ ¬has_single_tip then
     let (new_block, temp_state) = make_leader_block n state;
         new_M_i = add_mset (n, new_block) (Mi temp_state)
     in (temp_state\<lparr>Mi := new_M_i\<rparr>, True)
   else
     (state, False))"

(* Send 1-votes and 2-votes for transaction blocks *)
definition vote_for_tx_blocks :: "nat ⇒ 'tx process_local_state ⇒ 'tx process_local_state × bool" where
  "vote_for_tx_blocks n state =
  (let view_i = view_i state;
       M_i = Mi state;
       Q_i = Qi state;
       voted_i = voted_i state;
       phase_i = phase_i state;
       
       (* Check for finalized leader block in current view *)
       has_finalized_leader = (∃p b. (p, b) ∈ set_mset M_i ∧ 
                              b_type b = Leader_Block ∧ b_view b = view_i ∧
                              (∃q ∈ Q_i. q_z q = 2 ∧ q_b q = b));
       
       (* Check for unfinalized leader block in current view *)
       has_unfinalized_leader = (∃p b. (p, b) ∈ set_mset M_i ∧
                                b_type b = Leader_Block ∧ b_view b = view_i ∧
                                ¬(∃q ∈ Q_i. q_z q = 2 ∧ q_b q = b));
       
       (* Process 1-votes for transaction blocks *)
       tx_needing_1vote = {(pid, b) | pid b. (pid, b) ∈ set_mset M_i ∧
                           b_type b = Transaction_Block ∧ b_view b = view_i ∧
                           is_single_tip_block M_i Q_i b ∧
                           (∀q ∈ Q_i. q_z q = 1 ⟶ q ≤qc the(b_1QC b)) ∧
                           ¬voted_i 1 Transaction_Block (b_slot b) (b_auth b)};
       
       (* Process 2-votes for transaction blocks *)
       tx_needing_2vote = {q ∈ Q_i |
                           q_z q = 1 ∧ q_type q = Transaction_Block ∧
                           is_single_tip_qc Q_i M_i q ∧
                           ¬voted_i 2 Transaction_Block (q_slot q) (q_auth q) ∧
                           ¬(∃p b. (p, b) ∈ set_mset M_i ∧ b_h b > q_h q)};
       
       (* Update voted function for 1-votes *)
       voted_after_1 = (if tx_needing_1vote = {} then 
                         voted_i 
                       else
                         (λz x s p. if z = 1 ∧ x = Transaction_Block ∧
                                    (∃pid b. (pid, b) ∈ tx_needing_1vote ∧ s = b_slot b ∧ p = b_auth b)
                                    then True 
                                    else voted_i z x s p));
       
       (* Update voted function for 2-votes *)
       voted_after_2 = (if tx_needing_2vote = {} then 
                         voted_after_1
                       else
                         (λz x s p. if z = 2 ∧ x = Transaction_Block ∧
                                    (∃q ∈ tx_needing_2vote. s = q_slot q ∧ p = q_auth q)
                                    then True 
                                    else voted_after_1 z x s p));
       
       (* Update phase - enters phase 1 if any votes were sent *)
       new_phase = (if tx_needing_1vote ≠ {} ∨ tx_needing_2vote ≠ {} then
                     (λv. if v = view_i then True else phase_i v)
                   else
                     phase_i);
       
       (* Update state *)
       new_state = state\<lparr>voted_i := voted_after_2, phase_i := new_phase\<rparr>;
       action_taken = tx_needing_1vote ≠ {} ∨ tx_needing_2vote ≠ {}
   in
   if has_finalized_leader ∧ ¬has_unfinalized_leader then
     (new_state, action_taken)
   else
     (state, False))"

(* Vote for leader blocks *)
definition vote_for_leader_blocks :: "nat ⇒ 'tx process_local_state ⇒ 'tx process_local_state × bool" where
  "vote_for_leader_blocks n state =
  (let view_i = view_i state;
       M_i = Mi state;
       Q_i = Qi state;
       voted_i = voted_i state;
       phase_i = phase_i state;
       
       (* Only vote if in phase 0 *)
       in_phase_0 = ¬(phase_i view_i);
       
       (* Process 1-votes for leader blocks *)
       lead_needing_1vote = {(pid, b) | pid b. (pid, b) ∈ set_mset M_i ∧
                             b_type b = Leader_Block ∧ b_view b = view_i ∧
                             ¬voted_i 1 Leader_Block (b_slot b) (b_auth b)};
       
       (* Process 2-votes for leader blocks *)
       lead_needing_2vote = {q ∈ Q_i |
                             q_z q = 1 ∧ q_type q = Leader_Block ∧ q_view q = view_i ∧
                             ¬voted_i 2 Leader_Block (q_slot q) (q_auth q)};
       
       (* Update voted function for 1-votes *)
       voted_after_1 = (if lead_needing_1vote = {} then 
                         voted_i 
                       else
                         (λz x s p. if z = 1 ∧ x = Leader_Block ∧
                                    (∃pid b. (pid, b) ∈ lead_needing_1vote ∧ s = b_slot b ∧ p = b_auth b)
                                    then True 
                                    else voted_i z x s p));
       
       (* Update voted function for 2-votes *)
       voted_after_2 = (if lead_needing_2vote = {} then 
                         voted_after_1
                       else
                         (λz x s p. if z = 2 ∧ x = Leader_Block ∧
                                    (∃q ∈ lead_needing_2vote. s = q_slot q ∧ p = q_auth q)
                                    then True 
                                    else voted_after_1 z x s p));
       
       (* Update state *)
       new_state = state\<lparr>voted_i := voted_after_2\<rparr>;
       action_taken = lead_needing_1vote ≠ {} ∨ lead_needing_2vote ≠ {}
   in
   if in_phase_0 then
     (new_state, action_taken)
   else
     (state, False))"

(* Complain if blocks not finalized *)
definition handle_complaints :: "nat ⇒ nat ⇒ 'tx process_local_state ⇒ 'tx process_local_state × bool" where
  "handle_complaints n Delta state =
  (let view_i = view_i state;
       Q_i = Qi state;
       time_since_view = 0; (* Simplified - would need time tracking *)
       
       (* Find unfinalized QCs *)
       unfinalized_QCs = {q ∈ Q_i | ¬(∃q' ∈ Q_i. q_z q' = 2 ∧ q_b q = q_b q')};
       
       (* QCs not finalized for 6Δ *)
       complain_QCs = {q ∈ unfinalized_QCs | time_since_view ≥ 6 * Delta}
   in
   if complain_QCs ≠ {} then
     (* Create end-view message *)
     let end_view_msg = Block End_View_Message view_i 0 n 0 [] {} None {};
         new_M_i = add_mset (n, end_view_msg) (Mi state)
     in (state\<lparr>Mi := new_M_i\<rparr>, True)
   else
     (state, False))"

(* Complete protocol step *)
definition morpheus_step :: "nat ⇒ nat ⇒ 'tx list ⇒ 'tx process_local_state ⇒ 'tx process_local_state" where
  "morpheus_step n Delta txs state =
  (let (_state1, updated1) = handle_view_update n state;
       (_state2, updated2) = send_0_votes n _state1;
       (_state3, updated3) = send_0_QCs n _state2;
       (_state4, updated4) = handle_transaction n txs _state3;
       (_state5, updated5) = handle_leader n _state4;
       (_state6, updated6) = vote_for_tx_blocks n _state5;
       (_state7, updated7) = vote_for_leader_blocks n _state6;
       (_state8, updated8) = handle_complaints n Delta _state7
   in _state8)"

end