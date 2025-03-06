theory Morpheus
  imports
    Morpheus_Types
    Morpheus_Relations
    Morpheus_Procedures
    Morpheus_State
    Morpheus_F_Function
begin

(* Main theory imports all components *)

(* Example: Create an initial process state *)
definition example_init_state :: "nat \<Rightarrow> unit process_local_state" where
  "example_init_state n = init_process_state n"

(* Example: Run several steps of the protocol *)
definition run_protocol :: "nat \<Rightarrow> nat \<Rightarrow> nat \<Rightarrow> unit process_local_state \<Rightarrow> unit process_local_state" where
  "run_protocol n Delta steps init_state =
   (let rec = (\<lambda>state i. if i = 0 then state 
                         else rec (morpheus_step n Delta [] state) (i-1))
    in rec init_state steps)"

(* Example: Check if a block is finalized *)
definition is_block_finalized :: "'a block \<Rightarrow> 'a process_local_state \<Rightarrow> bool" where
  "is_block_finalized b state = (\<exists>q \<in> Qi state. q_z q = 2 \<and> q_b q = b)"

(* Example: Extract the finalized transaction sequence *)
definition extract_finalized_txs :: "'tx process_local_state \<Rightarrow> 'tx list" where
  "extract_finalized_txs state = F {(p, b) | p b. (p, b) \<in> set_mset (Mi state)}"

(* Define lemmas to support consistency and liveness *)

(* Lemma for QC uniqueness at same level *)
lemma qc_uniqueness:
  assumes "q1 \<in> Q" "q2 \<in> Q"
  assumes "q_z q1 = q_z q2" "q_type q1 = q_type q2" 
  assumes "q_view q1 = q_view q2" "q_h q1 = q_h q2"
  assumes "q_auth q1 = q_auth q2" "q_slot q1 = q_slot q2"
  shows "q_b q1 = q_b q2"
  sorry (* Needs proof *)

(* Lemma: If a transaction block is finalized, it will be in the extracted sequence *)
lemma finalized_block_in_sequence:
  assumes "is_block_finalized b state"
  assumes "b_type b = Transaction_Block"
  shows "set (b_txs b) \<subseteq> set (extract_finalized_txs state)"
  sorry (* Needs proof *)

(* Theorem 6.2: Consistency *)
theorem morpheus_consistency:
  assumes "M1 \<subseteq> M2" "M2 \<subseteq> M"
  shows "F M1 \<sqsubseteq> F M2"
  sorry (* Needs proof *)

(* Theorem 6.3: Liveness *)
theorem morpheus_liveness:
  "\<forall>b. b_type b = Transaction_Block \<and> b_auth b = correct_process \<longrightarrow>
       (\<exists>t. set(b_txs b) \<subseteq> set(F(M(t))))"
  sorry (* Needs proof *)

end