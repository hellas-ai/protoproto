theory Morpheus_Types
  imports "HOL-Library.FSet" "HOL-Library.Multiset"
begin

(* Message types *)
datatype msg_type =
   Vote_0
  | Vote_1
  | Vote_2
  | QC_msg
  | End_View_Message
  | View_Message
  | Block_msg

datatype block_type =
    Genesis_Block
  | Transaction_Block
  | Leader_Block

(* Block, QC, and View Message types - mutually recursive datatypes *)
datatype 'tx block = Block
  (b_type: block_type)
  (b_view: nat)
  (b_h: nat) 
  (b_auth: nat) (* Process ID *)
  (b_slot: nat)
  (b_txs: "'tx list")
  (b_prev: "'tx QC fset")
  (b_1QC: "'tx QC option")
  (b_just: "'tx View_Message fset") (* View v messages for leader blocks*)

and 'tx QC = QC
  (q_z: nat)
  (q_type: block_type)
  (q_view: nat)
  (q_h: nat)
  (q_auth: nat)
  (q_slot: nat)
  (q_b: "'tx block")

and 'tx View_Message = View_Message
  (vm_view: nat)
  (vm_q: "'tx QC")


(* Genesis block *)
definition genesis_block :: "'tx block" where
  "genesis_block = Block Genesis_Block 0 0 1 0 [] {||} None {||}"

(* Vote type - representing votes sent by processes *)
datatype 'tx vote = Vote
  (v_z: nat)
  (v_type: msg_type)
  (v_view: nat)
  (v_h: nat)
  (v_auth: nat)
  (v_slot: nat)
  (v_block: "'tx block") (* Implicitly represents H(b) *)

(* Message type - process ID and block *)
type_synonym 'tx message = "nat \<times> 'tx block"

(* Process state - multiset of messages *)
type_synonym 'tx process_state = "'tx message multiset"

(* QC store type *)
type_synonym 'tx qc_store = "'tx QC fset"

(* Vote tracking function types *)
type_synonym 'tx voted_function = "nat \<Rightarrow> msg_type \<Rightarrow> nat \<Rightarrow> nat \<Rightarrow> bool" (* voted(z, x, s, pj) *)
type_synonym phase_function = "nat \<Rightarrow> bool" (* phase(v) *)

(* Process local state record *)
record 'tx process_local_state =
  Mi :: "'tx process_state"
  Qi :: "'tx qc_store"
  view_i :: nat
  slot_lead_i :: nat
  slot_tr_i :: nat
  voted_i :: "'tx voted_function"
  phase_i :: phase_function

(* Initialize process state *)
definition init_process_state :: "nat \<Rightarrow> 'tx process_local_state" where
  "init_process_state n = 
    (let init_1qc = QC 1 Genesis_Block 0 0 1 0 genesis_block;
         init_msgs = {#(1, genesis_block)#};
         init_qcs = {|init_1qc|};
         init_voted = (\<lambda>z x s p. False);
         init_phase = (\<lambda>v. False)
     in
     \<lparr> Mi = init_msgs,
        Qi = init_qcs,
        view_i = 0,
        slot_lead_i = 0,
        slot_tr_i = 0,
        voted_i = init_voted,
        phase_i = init_phase \<rparr>)"

end