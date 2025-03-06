theory Morpheus_Procedures
  imports Morpheus_Relations
begin

(* Leader of view v *)
definition lead :: "nat \<Rightarrow> nat \<Rightarrow> nat" where
  "lead n v = v mod n"

(* Find the greatest 1-QC in a set of QCs *)
definition greatest_1QC :: "'tx qc_store \<Rightarrow> 'tx QC option" where
  "greatest_1QC Q = (
    let q1QCs = ffilter (\<lambda>q. q_z q = 1) Q in
    if q1QCs = {||} then None
    else Some (fMax q1QCs) 
  )"

(* MakeTrBlock procedure - fully implementing the pseudocode *)
definition make_tr_block :: "nat \<Rightarrow> 'a list \<Rightarrow> 'tx process_local_state \<Rightarrow> 'tx block \<times> 'tx process_local_state" where
  "make_tr_block n txs state = 
   (let view_i = view_i state;
       slot_i = slot_tr_i state;
       Q_i = Qi state;
       M_i = Mi state;
       
       (* Step 2: Find previous transaction block QC or genesis QC *)
       q1 = (if slot_i > 0 then
              (SOME q. q \<in> Q_i \<and> q_auth q = n \<and> q_type q = Transaction_Block \<and> q_slot q = slot_i - 1)
            else
              (SOME q. q \<in> Q_i \<and> q_b q = genesis_block \<and> q_z q = 1));
       
       (* Step 3: Check for single tip *)
       prev_set = (if \<exists>q2 \<in> Q_i. is_single_tip_qc Q_i M_i q2 
                  then insert (SOME q2. q2 \<in> Q_i \<and> is_single_tip_qc Q_i M_i q2) {q1}
                  else {q1});
       
       (* Step 4: Calculate max height in prev_set *)
       h' = Max {q_h q | q. q \<in> prev_set};
       
       (* Step 5: Get greatest 1-QC *)
       q_greatest = (SOME q. q \<in> Q_i \<and> q_z q = 1 \<and> (\<forall>q' \<in> Q_i. q_z q' = 1 \<longrightarrow> q' \<le>qc q));
       
       (* Create the block *)
       new_block = Block 
         Transaction_Block
         view_i
         (h' + 1)
         n
         slot_i
         txs
         prev_set
         (Some q_greatest)
         {};
       
       (* Step 7: Update state *)
       new_state = state\<lparr>slot_tr_i := slot_i + 1\<rparr>
   in (new_block, new_state))"

(* LeaderReady - exactly matching the pseudocode *)
definition leader_ready :: "nat \<Rightarrow> 'tx process_local_state \<Rightarrow> bool" where
  "leader_ready n state = 
   (let v = view_i state;
        lead_v = lead n v;
        M_i = Mi state;
        Q_i = Qi state;
        slot_lead = slot_lead_i state
    in
    n = lead_v \<and>
    (
      (* Case 1: First leader block of the view *)
      (\<not>(\<exists>p b. (p, b) \<in> set_mset M_i \<and> b_type b = Leader_Block \<and> b_view b = v \<and> b_auth b = n) \<and>
       (* a. Received enough view messages *)
       (card {vm | vm p b. (p, b) \<in> set_mset M_i \<and> vm \<in> b_just b \<and> vm_view vm = v} \<ge> n - (n div 3)) \<and>
       (* b. Has QC for previous leader block or at slot 0 *)
       (slot_lead = 0 \<or> 
        (\<exists>q \<in> Q_i. b_type (q_b q) = Leader_Block \<and> b_auth (q_b q) = n \<and> b_slot (q_b q) = slot_lead - 1))
      )
      \<or>
      (* Case 2: Subsequent leader blocks in the view *)
      ((\<exists>p b. (p, b) \<in> set_mset M_i \<and> b_type b = Leader_Block \<and> b_view b = v \<and> b_auth b = n) \<and>
       (\<exists>q \<in> Q_i. q_z q = 1 \<and> b_type (q_b q) = Leader_Block \<and> 
                 b_auth (q_b q) = n \<and> b_slot (q_b q) = slot_lead - 1))
    ))"

(* MakeLeaderBlock procedure - precisely matching the pseudocode *)
definition make_leader_block :: "nat \<Rightarrow> 'tx process_local_state \<Rightarrow> 'tx block \<times> 'tx process_local_state" where
  "make_leader_block n state = 
   (let view_i = view_i state;
        slot_lead = slot_lead_i state;
        Q_i = Qi state;
        M_i = Mi state;
        
        (* Step 2: Set prev to be the tips of Q_i *)
        tips = {q \<in> Q_i | is_tip Q_i M_i q};
        
        (* Step 3: Add pointer to previous leader block if needed *)
        prev_set = (if slot_lead > 0 then
                     (let q_prev = (SOME q. q \<in> Q_i \<and> q_auth q = n \<and> 
                                        q_type q = Leader_Block \<and> q_slot q = slot_lead - 1)
                      in if q_prev \<in> tips then tips else insert q_prev tips)
                    else tips);
        
        (* Step 4: Calculate max height in prev *)
        h' = Max {q_h q | q. q \<in> prev_set};
        
        (* Check if first leader block in this view *)
        first_in_view = \<not>(\<exists>p b. (p, b) \<in> set_mset M_i \<and> 
                           b_type b = Leader_Block \<and> b_view b = view_i \<and> b_auth b = n);
        
        (* Step 5-6: Handle first vs subsequent leader blocks *)
        just_set = (if first_in_view then
                     (* First leader block - find view messages from n-f processes *)
                     {vm | vm p b. (p, b) \<in> set_mset M_i \<and> vm \<in> b_just b \<and> vm_view vm = view_i}
                   else 
                     {});
        
        one_qc = (if first_in_view then
                   (* First leader block - find 1-QC that's >= all 1-QCs in justification *)
                   (SOME q. q \<in> Q_i \<and> q_z q = 1 \<and> 
                          (\<forall>vm \<in> just_set. vm_q vm \<le>qc q))
                 else
                   (* Subsequent leader block - use 1-QC for previous leader block *)
                   (SOME q. q \<in> Q_i \<and> q_z q = 1 \<and> b_type (q_b q) = Leader_Block \<and>
                          b_auth (q_b q) = n \<and> b_slot (q_b q) = slot_lead - 1));
        
        (* Create the block *)
        new_block = Block
          Leader_Block
          view_i
          (h' + 1)
          n
          slot_lead
          []
          prev_set
          (Some one_qc)
          just_set;
        
        (* Step 8: Update state *)
        new_state = state\<lparr>slot_lead_i := slot_lead + 1\<rparr>
   in (new_block, new_state))"

(* Payload Ready - external function with constraints *)
definition payload_ready :: "nat \<Rightarrow> 'tx process_local_state \<Rightarrow> bool" where
  "payload_ready n state = 
   (let slot_tr = slot_tr_i state;
        Q_i = Qi state
    in
    (* External condition that would be set to True by outside mechanisms *)
    (* For formal verification, we must ensure the prerequisite is satisfied *)
    (slot_tr = 0 \<or> 
     (\<exists>q \<in> Q_i. q_auth q = n \<and> q_type q = Transaction_Block \<and> q_slot q = slot_tr - 1)))"

end