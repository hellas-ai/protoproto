---- MODULE Morpheus ----
EXTENDS TLC, Sequences, Integers, FiniteSets, Naturals

\* Constants with more precise definitions
CONSTANT Processes,       \* Set of all processes (1..N)
         F,               \* Maximum number of Byzantine processes
         MaxViewChanges,  \* Maximum number of view changes to consider
         MaxBlocks        \* Maximum number of blocks to consider

\* Derived constants
SetOfProcesses == 1..Processes
CorrectProcesses == 1..Processes-F
ByzantineProcesses == Processes-F+1..Processes
QuorumSize == Processes - F

\* Block types
GENESIS  == 0
TR_BLOCK == 1
LEAD_BLOCK == 2
BlockTypes == {GENESIS, TR_BLOCK, LEAD_BLOCK}

\* Vote types
VoteType == {0, 1, 2}

\* Message types
MSG_BLOCK == "block"
MSG_VOTE == "vote"
MSG_QC == "qc"
MSG_VIEW == "view"

\* Variables
VARIABLES
  \* Block-related variables
  blocks,       \* Set of block records (compact representation)
  blockObs,     \* Function mapping block ID -> set of observed block IDs
  nextBlockID,  \* Counter for assigning block IDs

  \* QC-related variables
  qcs,          \* Set of QCs as tuples <<vote_type, blockID>>

  \* Process state (consolidated into a single record per process)
  pState,       \* [view, phase, blocks, qcs, votes]

  \* Pending Messages Queue (Set in this simplified version)
  pendingMessages, \* Set of messages to be processed

  \* System state
  finalized,    \* Set of finalized block IDs
  after_gst    \* Flag indicating if we're after GST

\* Define record types for more compact state representation
BlockRecord == [
  id: 0..MaxBlocks,
  type: BlockTypes,
  view: -1..MaxViewChanges,
  height: 0..MaxBlocks,
  auth: 0..Processes,
  slot: 0..MaxBlocks,
  prev: SUBSET (0..MaxBlocks)
]

MessageRecord == [
  type: {MSG_BLOCK, MSG_VOTE, MSG_QC, MSG_VIEW},
  from: 0..Processes,
  to: 0..Processes,  \* 0 means broadcast
  block: 0..MaxBlocks,
  vote_type: VoteType,
  view: 0..MaxViewChanges
]

ProcessStateRecord == [
  view: 0..MaxViewChanges,
  phase: {0, 1},
  blocks: SUBSET (0..MaxBlocks),
  qcs: SUBSET (VoteType \X (0..MaxBlocks)),
  votes: SUBSET (VoteType \X (0..MaxBlocks))
]

vars == <<blocks, blockObs, nextBlockID, qcs, pState, pendingMessages, finalized, after_gst>>

\* Helper Functions
IsCorrect(p) == p \in CorrectProcesses
Leader(v) == (v % Processes) + 1

\* Create new block
Block(id, type, view, height, auth, slot, prev) ==
  [id |-> id, type |-> type, view |-> view, height |-> height,
   auth |-> auth, slot |-> slot, prev |-> prev]

\* Create QC
QC(z, blockID) == <<z, blockID>>

\* Create message (simplified - direct processing for correct processes)
Message(type, from, to, block, vote_type, view) ==
  [type |-> type, from |-> from, to |-> to, block |-> block,
   vote_type |-> vote_type, view |-> view]

\* GENESIS block is ID 0
GENESIS_BLOCK == 1

\* Get block record by ID (optimized with case handling)
GetBlock(blockID) ==
  IF blockID = GENESIS_BLOCK THEN
    Block(GENESIS_BLOCK, GENESIS, -1, 0, 0, 0, {})
  ELSE
    CHOOSE b \in blocks: b.id = blockID

\* Observes relation (simplified)
Observes(b, a) ==
  \/ b = a
  \/ a \in blockObs[b]

\* Block conflict check
Conflict(b1, b2) ==
  /\ b1 /= b2
  /\ ~Observes(b1, b2)
  /\ ~Observes(b2, b1)

\* Get tips for a process (optimized)
GetTips(p) ==
  LET pBlocks == pState[p].blocks IN
  {b \in pBlocks: \A other \in (pBlocks \ {b}): ~Observes(other, b)}

\* Initial state
Init ==
  \* Block initialization
  /\ blocks = {}  \* No blocks except genesis (handled specially)
  /\ blockObs = <<{1}>>  \* Genesis observes itself
  /\ nextBlockID = 1

  \* QC initialization - Genesis has implicit 1-QC
  /\ qcs = {QC(1, GENESIS_BLOCK)}

  \* Process state initialization
  /\ pState = [p \in 1..Processes |-> [
       view |-> 0,
       phase |-> 0,
       blocks |-> {GENESIS_BLOCK},
       qcs |-> {QC(1, GENESIS_BLOCK)},
       votes |-> {}
     ]]

  \* Pending Messages Initialization
  /\ pendingMessages = {}

  \* System state
  /\ finalized = {}
  /\ after_gst = FALSE  \* Start before GST


\* Enter GST - transition to synchronous network
EnterGST ==
  /\ ~after_gst
  /\ after_gst' = TRUE
  /\ UNCHANGED <<blocks, blockObs, nextBlockID, qcs, pState, pendingMessages, finalized>>

\* Update block observation relation (optimized)
UpdateObservation(newBlockID, prevBlockIDs) ==
  LET
    directObs == prevBlockIDs \union {newBlockID}
    allObs == UNION {blockObs[prev]: prev \in prevBlockIDs} \* Line that might be causing issue
  IN
    blockObs' = Append(blockObs, allObs)

\* Process a message (consolidated action) - NO LONGER DIRECTLY PROCESSING, NOW ADDS TO QUEUE
ProcessMessage(p, msg) ==
  /\ IsCorrect(p)
  /\ CASE msg.type = MSG_BLOCK ->  \* Block message
       /\ msg.block < nextBlockID
       /\ pState' = [pState EXCEPT ![p].blocks = @ \union {msg.block}]
       /\ UNCHANGED <<blocks, blockObs, nextBlockID, qcs, finalized, after_gst>>

     [] msg.type = MSG_VOTE ->  \* Vote message
       /\ LET
            \* Add vote to process state
            newPState == [pState EXCEPT ![p].votes = @ \union {<<msg.vote_type, msg.block>>}]

            \* Check if we have a quorum
            votes == {q \in 1..Processes: <<msg.vote_type, msg.block>> \in newPState[q].votes}
            hasQuorum == Cardinality(votes) >= QuorumSize
            newQC == QC(msg.vote_type, msg.block)
            qcMsg == Message(MSG_QC, p, 0, msg.block, msg.vote_type, 0) \* Message to broadcast QC
          IN
            /\ pState' = newPState
            /\ IF hasQuorum /\ newQC \notin qcs THEN
                 /\ qcs' = qcs \union {newQC}
                 /\ IF msg.vote_type = 2 THEN  \* 2-votes finalize blocks
                      finalized' = finalized \union {msg.block}
                    ELSE
                      UNCHANGED finalized
                 /\ pendingMessages' = pendingMessages \union {qcMsg} \* Add QC broadcast message to pending
               ELSE
                 /\ UNCHANGED <<qcs, finalized>>
            /\ UNCHANGED <<blocks, blockObs, nextBlockID, after_gst>>

     [] msg.type = MSG_QC ->  \* QC message
       /\ pState' = [pState EXCEPT ![p].qcs = @ \union {<<msg.vote_type, msg.block>>}]
       /\ UNCHANGED <<blocks, blockObs, nextBlockID, qcs, finalized, after_gst>>

     [] msg.type = MSG_VIEW ->  \* View change message
       /\ msg.view > pState[p].view
       /\ pState' = [pState EXCEPT
                    ![p].view = msg.view,
                    ![p].phase = 0]  \* Reset phase on view change
       /\ UNCHANGED <<blocks, blockObs, nextBlockID, qcs, finalized, after_gst>>

\* Process Pending Messages - NEW ACTION
ProcessPendingMessages ==
  /\ pendingMessages /= {}
  /\ \E msg \in pendingMessages: \* Non-deterministic choice of message to process
       /\ pendingMessages' = pendingMessages \ {msg}
       /\ IF msg.from = 0 THEN \A p \in 1..Processes: ProcessMessage(p, msg) ELSE ProcessMessage(msg.from, msg) \* Process the message as if received by 'msg.from' (broadcast simulation)
       /\ UNCHANGED <<blocks, blockObs, nextBlockID, qcs, finalized, after_gst>>


\* Send and Process Message (Directly for Correct Processes - Simplified Network) - REMOVED, NO LONGER NEEDED


\* Transaction block creation (optimized)
CreateTxBlock(p) ==
  /\ IsCorrect(p)
  /\ nextBlockID < MaxBlocks
  /\ LET
       \* Find block author's highest slot transaction block
       pBlocks == {b \in pState[p].blocks:
                  /\ GetBlock(b).type = TR_BLOCK
                  /\ GetBlock(b).auth = p}
       highestSlot == IF pBlocks = {} THEN -1
                     ELSE CHOOSE s \in {GetBlock(b).slot: b \in pBlocks}:
                          \A s2 \in {GetBlock(b).slot: b \in pBlocks}: s >= s2
       slot == highestSlot + 1

       \* Find previous blocks to reference
       selfPrevID ==
         IF highestSlot >= 0 THEN
           CHOOSE b \in pState[p].blocks:
             /\ GetBlock(b).type = TR_BLOCK
             /\ GetBlock(b).auth = p
             /\ GetBlock(b).slot = highestSlot
         ELSE GENESIS_BLOCK

       tipID == IF Cardinality(GetTips(p)) = 1
               THEN CHOOSE b \in GetTips(p): TRUE
               ELSE GENESIS_BLOCK

       prevIDs ==
         IF tipID = GENESIS_BLOCK \/ tipID = selfPrevID
         THEN {selfPrevID}
         ELSE {selfPrevID, tipID}

       \* Calculate new block height
       prevHeights == {GetBlock(id).height: id \in prevIDs}
       newHeight == 1 + CHOOSE h \in prevHeights: \A h2 \in prevHeights: h >= h2

       \* Create new block
       newBlockID == nextBlockID
       newBlock == Block(newBlockID, TR_BLOCK, pState[p].view, newHeight, p, slot, prevIDs)
       blockMsg == Message(MSG_BLOCK, p, 0, newBlockID, 0, pState[p].view) \* Message to broadcast block
     IN
       /\ blocks' = blocks \union {newBlock}
       /\ UpdateObservation(newBlockID, prevIDs)
       /\ nextBlockID' = nextBlockID + 1
       /\ pState' = [pState EXCEPT ![p].blocks = @ \union {newBlockID}]
       /\ pendingMessages' = pendingMessages \union {blockMsg} \* Add block broadcast message to pending
       /\ UNCHANGED <<qcs, finalized, after_gst>>

\* Leader block creation (optimized)
CreateLeaderBlock(p) ==
  /\ IsCorrect(p)
  /\ p = Leader(pState[p].view)  \* Must be the leader
  /\ pState[p].phase = 0  \* Must be in phase 0
  /\ nextBlockID < MaxBlocks
  /\ Cardinality(GetTips(p)) > 1  \* Only needed when there are multiple tips
  /\ LET
       \* Find leader's highest slot leader block for this view
       leaderBlocks == {b \in pState[p].blocks:
                      /\ GetBlock(b).type = LEAD_BLOCK
                      /\ GetBlock(b).auth = p
                      /\ GetBlock(b).view = pState[p].view}
       highestSlot == IF leaderBlocks = {} THEN -1
                     ELSE CHOOSE s \in {GetBlock(b).slot: b \in leaderBlocks}:
                          \A s2 \in {GetBlock(b).slot: b \in leaderBlocks}: s >= s2
       slot == highestSlot + 1

       \* Setup previous block references
       selfPrevID ==
         IF highestSlot >= 0 THEN
           CHOOSE b \in pState[p].blocks:
             /\ GetBlock(b).type = LEAD_BLOCK
             /\ GetBlock(b).auth = p
             /\ GetBlock(b).view = pState[p].view
             /\ GetBlock(b).slot = highestSlot
         ELSE GENESIS_BLOCK

       tips == GetTips(p)

       prevIDs ==
         IF selfPrevID = GENESIS_BLOCK
         THEN tips
         ELSE {selfPrevID} \union tips

       \* Calculate new block height
       prevHeights == {GetBlock(id).height: id \in prevIDs}
       newHeight == 1 + CHOOSE h \in prevHeights: \A h2 \in prevHeights: h >= h2

       \* Create new block
       newBlockID == nextBlockID
       newBlock == Block(newBlockID, LEAD_BLOCK, pState[p].view, newHeight, p, slot, prevIDs)
       blockMsg == Message(MSG_BLOCK, p, 0, newBlockID, 0, pState[p].view) \* Message to broadcast block
     IN
       /\ blocks' = blocks \union {newBlock}
       /\ UpdateObservation(newBlockID, prevIDs)
       /\ nextBlockID' = nextBlockID + 1
       /\ pState' = [pState EXCEPT ![p].blocks = @ \union {newBlockID}]
       /\ pendingMessages' = pendingMessages \union {blockMsg} \* Add block broadcast message to pending
       /\ UNCHANGED <<qcs, finalized, after_gst>>

\* Vote for a block (optimized)
VoteForBlock(p, blockID, voteType) ==
  /\ IsCorrect(p)
  /\ blockID \in pState[p].blocks
  /\ <<voteType, blockID>> \notin pState[p].votes
  /\ LET
       block == GetBlock(blockID)
       voteMsg == Message(MSG_VOTE, p, 0, blockID, voteType, pState[p].view) \* Message to broadcast vote

       \* Check if can vote based on vote type
       canVote0 == \* 0-vote requirements (non-equivocation)
         /\ voteType = 0
         /\ ~(\E otherID \in pState[p].blocks:
              /\ blockID /= otherID
              /\ GetBlock(otherID).type = block.type
              /\ GetBlock(otherID).auth = block.auth
              /\ GetBlock(otherID).slot = block.slot
              /\ Conflict(blockID, otherID))

       canVote1 == \* 1-vote requirements
         /\ voteType = 1
         /\ block.type = TR_BLOCK => Cardinality(GetTips(p)) = 1
         /\ block.type = LEAD_BLOCK => pState[p].phase = 0

       canVote2 == \* 2-vote requirements
         /\ voteType = 2
         /\ <<1, blockID>> \in pState[p].qcs  \* Must have 1-QC first
         /\ block.type = TR_BLOCK =>
              /\ ~(\E otherID \in pState[p].blocks:
                    GetBlock(otherID).height > block.height)
              /\ pState[p].phase = 1
         /\ block.type = LEAD_BLOCK => pState[p].phase = 0
     IN
       /\ \/ canVote0
          \/ canVote1
          \/ canVote2
       /\ pState' = [pState EXCEPT ![p].votes = @ \union {<<voteType, blockID>>}]
       /\ IF voteType = 1 /\ block.type = TR_BLOCK
          THEN pState' = [pState' EXCEPT ![p].phase = 1]  \* Enter phase 1 on 1-vote for transaction
          ELSE TRUE
       /\ pendingMessages' = pendingMessages \union {voteMsg} \* Add vote broadcast message to pending
       /\ UNCHANGED <<blocks, blockObs, nextBlockID, qcs, finalized, after_gst>>

\* Non-deterministic View Change Trigger (Simplified Timer)
ViewChangeTrigger ==
  \E p \in CorrectProcesses:
    /\ CHOOSE j \in BOOLEAN: TRUE  \* Non-deterministic choice to trigger view change
    /\ pState' = [pState EXCEPT
                 ![p].view = @ + 1,
                 ![p].phase = 0]  \* Reset phase on view change
    /\ LET viewMsg == Message(MSG_VIEW, p, Leader(pState'[p].view), 0, 0, pState'[p].view) \* View message to leader
       IN
         /\ pendingMessages' = pendingMessages \union {viewMsg} \* Add view message to pending
    /\ UNCHANGED <<blocks, blockObs, nextBlockID, qcs, finalized, after_gst>>


\* Byzantine behavior (creating conflicting blocks)
CreateConflictingBlocks(p) ==
  /\ p \in ByzantineProcesses
  /\ nextBlockID + 1 < MaxBlocks
  /\ LET
       \* Create two conflicting blocks with the same slot
       block1ID == nextBlockID
       block2ID == nextBlockID + 1
       slot == 0
       newHeight == 1
       newBlock1 == Block(block1ID, TR_BLOCK, 0, newHeight, p, slot, {GENESIS_BLOCK})
       newBlock2 == Block(block2ID, TR_BLOCK, 0, newHeight, p, slot, {GENESIS_BLOCK})
       blockMsg1 == Message(MSG_BLOCK, p, 0, block1ID, 0, 0) \* Message to broadcast block1
       blockMsg2 == Message(MSG_BLOCK, p, 0, block2ID, 0, 0) \* Message to broadcast block2
     IN
       /\ blocks' = blocks \union {newBlock1, newBlock2}
       /\ blockObs' = Append(Append(blockObs, {GENESIS_BLOCK, block1ID}),
                            {GENESIS_BLOCK, block2ID})
       /\ nextBlockID' = nextBlockID + 2
       /\ pendingMessages' = pendingMessages \union {blockMsg1, blockMsg2} \* Add both conflicting block messages to pending
       /\ UNCHANGED <<pState, finalized, after_gst, qcs>>


\* Next state
Next ==
  \/ EnterGST
  \/ ProcessPendingMessages \* Process messages from the pending queue
  \/ \E p \in CorrectProcesses: CreateTxBlock(p)
  \/ \E p \in CorrectProcesses: CreateLeaderBlock(p)
  \/ \E p \in CorrectProcesses, blockID \in 0..nextBlockID-1, vt \in VoteType:
       VoteForBlock(p, blockID, vt)
  \/ ViewChangeTrigger
  \/ \E p \in ByzantineProcesses: CreateConflictingBlocks(p)

\* Safety invariant: no conflicting blocks are finalized
NoConflictingFinalized ==
  \A b1, b2 \in finalized:
    ~Conflict(b1, b2)

\* Complete specification
Spec ==
  /\ Init
  /\ [][Next]_vars
  \* Fairness - Removed Fairness for Simplified Model for Initial Exploration

====