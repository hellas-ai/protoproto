theory Morpheus_Relations
  imports Morpheus_Types
begin

(* QC comparison relation - QCs are preordered first by view, then by type with lead < Tr, then by height *)
fun less_than_qc :: "'tx block QC ⇒ 'tx block QC ⇒ bool" where
  "less_than_qc q1 q2 = (q_view q1 < q_view q2 ∨
                 (q_view q1 = q_view q2 ∧ 
                  ((q_type q1 = Leader_Block ∧ q_type q2 = Transaction_Block) ∨
                   (q_type q1 = q_type q2 ∧ q_h q1 < q_h q2))))"

(* QC less-than-or-equal relation *)
definition qc_leq :: "'tx block QC ⇒ 'tx block QC ⇒ bool" ("_ ≤qc _" [51,51] 50) where
  "qc_leq q1 q2 = ¬ less_than_qc q2 q1"

(* Points to relation for blocks - block b points to block b' if there's a QC for b' in b.prev *)
definition points_to :: "'tx block ⇒ 'tx block ⇒ bool" where
  "points_to b b' = (∃qc ∈ b_prev b. q_b qc = b')"

(* Observes relation for blocks - recursive definition *)
function observes :: "'tx block ⇒ 'tx block ⇒ bool" where
  "observes b b' = (b = b' ∨                   (* A block observes itself *)
                   (b ≠ genesis_block ∧        (* Genesis observes only itself *)
                    (∃b''. points_to b b'' ∧ observes b'' b')))" (* Observes transitively *)
  by pat_completeness auto

(* Termination proof *)
termination
  (* Need to prove that the recursive calls terminate *)
  sorry (* This needs a proper termination proof based on block height *)

(* All blocks observed by a block b *)
definition observed_blocks :: "'tx block ⇒ 'tx block set" ("[_]") where
  "observed_blocks b = {b' | b'. observes b b'}"

(* Blocks conflict if they don't observe each other *)
definition conflicts :: "'tx block ⇒ 'tx block ⇒ bool" where
  "conflicts b b' = (¬ observes b b' ∧ ¬ observes b' b)"

(* Observes relation for QCs *)
definition qc_observes :: "'tx qc_store ⇒ 'tx process_state ⇒ 'tx block QC ⇒ 'tx block QC ⇒ bool" where
  "qc_observes Q M q q' = (
    (q ∈ Q ∧ q' ∈ Q ∧
     ((q_type q = q_type q' ∧ q_auth q = q_auth q' ∧ q_slot q > q_slot q') ∨
      (q_type q = q_type q' ∧ q_auth q = q_auth q' ∧ q_slot q = q_slot q' ∧ q_z q ≥ q_z q') ∨
      (q_b q ∈ {b | p b. (p, b) ∈ set_mset M} ∧ points_to (q_b q) (q_b q'))
     )
    )
  )"

(* QC tip - no other QC strictly observes it *)
definition is_tip :: "'tx qc_store ⇒ 'tx process_state ⇒ 'tx block QC ⇒ bool" where
  "is_tip Q M q = (q ∈ Q ∧ (∀q' ∈ Q. qc_observes Q M q' q ⟶ qc_observes Q M q q'))"

(* QC single tip - observes all other QCs *)
definition is_single_tip_qc :: "'tx qc_store ⇒ 'tx process_state ⇒ 'tx block QC ⇒ bool" where
  "is_single_tip_qc Q M q = (q ∈ Q ∧ (∀q' ∈ Q. qc_observes Q M q q'))"

(* Block single tip *)
definition is_single_tip_block :: "'tx process_state ⇒ 'tx qc_store ⇒ 'tx block ⇒ bool" where
  "is_single_tip_block M Q b = (
    ∃q. is_single_tip_qc Q M q ∧ 
        q_b q = b ∧
        (∀p b'. (p, b') ∈ set_mset M ∧ q_b q = b' ⟶ b = b')
  )"

(* QC is final if there exists q' that observes q and q is a 2-QC *)
definition is_final :: "'tx qc_store ⇒ 'tx process_state ⇒ 'tx block QC ⇒ bool" where
  "is_final Q M q = (∃q' ∈ Q. qc_observes Q M q' q ∧ q_z q = 2)"

end