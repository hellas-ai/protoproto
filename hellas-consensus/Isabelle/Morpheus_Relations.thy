theory Morpheus_Relations
  imports Morpheus_Types
begin

(* QC comparison relation - QCs are preordered first by view, then by type with lead < Tr, then by height *)
fun less_than_qc :: "'tx QC \<Rightarrow> 'tx QC \<Rightarrow> bool" (infix "<qc" 50) where
  "less_than_qc q1 q2 = (q_view q1 < q_view q2 \<or>
                 (q_view q1 = q_view q2 \<and> 
                  ((q_type q1 = Leader_Block \<and> q_type q2 = Transaction_Block) \<or>
                   (q_type q1 = q_type q2 \<and> q_h q1 < q_h q2))))"

(* QC less-than-or-equal relation *)
definition qc_leq :: "'tx QC \<Rightarrow> 'tx QC \<Rightarrow> bool" (infix "\<le>qc" 50) where
  "qc_leq q1 q2 \<equiv> (less_than_qc q2 q1 \<or> (q_view q1 = q_view q2 \<and> q_type q1 = q_type q2 \<and> q_h q1 = q_h q2))"

instantiation QC :: (type) linorder
begin

declare qc_leq_def [simp add]

lemma qc_refl: "q \<le>qc q" by auto

lemma qc_trans: "\<lbrakk> q1 \<le>qc q2; q2 \<le>qc q3 \<rbrakk> \<Longrightarrow> q1 \<le>qc q3"
  by auto

lemma qc_antisym: "\<lbrakk> q1 \<le>qc q2; q2 \<le>qc q1 \<rbrakk> \<Longrightarrow> q1 = q2"
  try

lemma qc_total: "q1 \<le>qc q2 \<or> q2 \<le>qc q1"
  by auto

lemma less_QC_def2[simp]: "q1 <qc q2 \<longleftrightarrow> (q1 \<le>qc q2 \<and> \<not> q2 \<le>qc q1)"
  by auto

end

end
(* Points to relation for blocks - block b points to block b' if there's a QC for b' in b.prev *)
definition points_to :: "'tx block \<Rightarrow> 'tx block \<Rightarrow> bool" where
  "points_to b b' \<equiv> (\<exists>qc |\<in>| b_prev b. q_b qc = b')"

(* Observes relation for blocks - recursive definition *)
function observes :: "'tx block \<Rightarrow> 'tx block \<Rightarrow> bool" where
  "observes b b' = (b = b' \<or>
                   (b \<noteq> genesis_block \<and>
                    (\<exists>b''. points_to b b'' \<and> observes b'' b')))" (* Observes transitively *)
  by pat_completeness auto

(* Termination proof *)
termination
  (* Need to prove that the recursive calls terminate *)
  sorry (* This needs a proper termination proof based on block height *)

(* All blocks observed by a block b *)
definition observed_blocks :: "'tx block \<Rightarrow> 'tx block set" ("[_]") where
  "observed_blocks b = {b' | b'. observes b b'}"

(* Blocks conflict if they don't observe each other *)
definition conflicts :: "'tx block \<Rightarrow> 'tx block \<Rightarrow> bool" where
  "conflicts b b' = (\<not> observes b b' \<and> \<not> observes b' b)"

(* Observes relation for QCs *)
definition qc_observes :: "'tx qc_store \<Rightarrow> 'tx process_state \<Rightarrow> 'tx QC \<Rightarrow> 'tx QC \<Rightarrow> bool" where
  "qc_observes Q M q q' = (
    (q |\<in>| Q \<and> q' |\<in>| Q \<and>
     ((q_type q = q_type q' \<and> q_auth q = q_auth q' \<and> q_slot q > q_slot q') \<or>
      (q_type q = q_type q' \<and> q_auth q = q_auth q' \<and> q_slot q = q_slot q' \<and> q_z q \<ge> q_z q') \<or>
      (q_b q \<in> {b | p b. (p, b) \<in> set_mset M} \<and> points_to (q_b q) (q_b q'))
     )
    )
  )"

(* QC tip - no other QC strictly observes it *)
definition is_tip :: "'tx qc_store \<Rightarrow> 'tx process_state \<Rightarrow> 'tx QC \<Rightarrow> bool" where
  "is_tip Q M q = (q |\<in>| Q \<and> (\<forall>q' |\<in>| Q. qc_observes Q M q' q \<longrightarrow> qc_observes Q M q q'))"

(* QC single tip - observes all other QCs *)
definition is_single_tip_qc :: "'tx qc_store \<Rightarrow> 'tx process_state \<Rightarrow> 'tx QC \<Rightarrow> bool" where
  "is_single_tip_qc Q M q = (q |\<in>| Q \<and> (\<forall>q' |\<in>| Q. qc_observes Q M q q'))"

(* Block single tip *)
definition is_single_tip_block :: "'tx process_state \<Rightarrow> 'tx qc_store \<Rightarrow> 'tx block \<Rightarrow> bool" where
  "is_single_tip_block M Q b = (
    \<exists>q. is_single_tip_qc Q M q \<and> 
        q_b q = b \<and>
        (\<forall>p b'. (p, b') \<in> set_mset M \<and> q_b q = b' \<longrightarrow> b = b')
  )"

(* QC is final if there exists q' that observes q and q is a 2-QC *)
definition is_final :: "'tx qc_store \<Rightarrow> 'tx process_state \<Rightarrow> 'tx QC \<Rightarrow> bool" where
  "is_final Q M q = (\<exists>q' |\<in>| Q. qc_observes Q M q' q \<and> q_z q = 2)"

end