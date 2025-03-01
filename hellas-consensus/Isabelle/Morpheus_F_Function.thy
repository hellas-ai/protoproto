theory Morpheus_F_Function
  imports Morpheus_Relations
begin

(* 
 * Functions for extracting the total ordering from blocks
 *)

(* Sequence concatenation *)
definition seq_concat :: "'a list ⇒ 'a list ⇒ 'a list" (infixr "⊕" 65) where
  "xs ⊕ ys = xs @ ys"

(* Extract transactions from a block *)
definition block_txs :: "'tx block ⇒ 'tx list" where
  "block_txs b = b_txs b"

(* Given a set of blocks B, τ†(B) gives a sequence respecting observation relation *)
definition tau_dag :: "'tx block set ⇒ 'tx block list" where
  "tau_dag B = 
   (let sorted_blocks = sorted_list_of_set B;
        (* This is a simplified τ† - would need topological sort based on observations *)
        (* For real implementation, would ensure if b' observes b, then b appears before b' *)
        result = sorted_blocks
    in result)"

(* Recursive definition of τ(b) as specified in Section 4 *)
function tau :: "'tx block ⇒ 'tx block list" where
  "tau b = (if b = genesis_block then [genesis_block]
           else 
             (let q = the(b_1QC b);
                  b' = q_b q;
                  observed_by_b = [b];
                  observed_by_b' = set(tau b');
                  diff = {b} - observed_by_b'
              in tau b' @ tau_dag diff))"
  by pat_completeness auto

(* Termination proof *)
termination
  (* Need to prove that the recursive calls terminate *)
  sorry (* This requires a proper termination proof based on block height *)

(* Extract transactions from a sequence of blocks *)
definition Tr :: "'tx block list ⇒ 'tx list" where
  "Tr bs = concat (map block_txs (filter (λb. b_type b = Transaction_Block) bs))"

(* Define F to extract ordering from messages *)
definition F :: "'tx message set ⇒ 'tx list" where
  "F M = 
   (let blocks = {b | p b. (p, b) ∈ M};
        downward_closed = {b ∈ blocks | ∀b'. b' ∈ observed_blocks b ⟶ b' ∈ blocks};
        
        (* Find the highest 2-QC in messages *)
        all_qcs = {q | q b. b ∈ blocks ∧ 
                   ((∃qc ∈ b_prev b. qc = q) ∨ (b_1QC b = Some q))};
        two_qcs = {q ∈ all_qcs | q_z q = 2 ∧ q_b q ∈ downward_closed};
        
        (* Find the maximal 2-QC *)
        max_2qc_opt = (if two_qcs = {} then None 
                      else Some(SOME q. q ∈ two_qcs ∧ (∀q' ∈ two_qcs. q' ≤qc q)));
        
        result = (case max_2qc_opt of
                    None ⇒ Tr[genesis_block]
                  | Some q ⇒ Tr(tau (q_b q)))
    in result)"

end