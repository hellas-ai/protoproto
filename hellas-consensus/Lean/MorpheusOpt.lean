namespace MorpheusOpt

/-- Process ID type -/
abbrev ProcessId := Nat

/-- Transaction type -/
structure Transaction where
  id : Nat
  data : String
  deriving BEq, Repr, Inhabited, Hashable

/-- Block types in the protocol -/
inductive BlockType
  | genesis
  | transaction
  | leader
  deriving BEq, Repr, Inhabited, Hashable

/-- Vote level -/
inductive VoteLevel
  | zero
  | one
  | two
  deriving BEq, Repr, Inhabited, Hashable

/-- Fastpath configuration options -/
structure FastpathConfig where
  /-- Send 0-votes to all processes (reduces latency by 1δ in ideal case) -/
  broadcast0Votes : Bool := false
  /-- Allow leaders to point to blocks without QCs (reduces latency by 1δ) -/
  fastBlockPointing : Bool := false
  /-- Allow leader blocks to be finalized with just a 1-QC from n processes (reduces latency by 1δ) -/
  fastLeaderFinalization : Bool := false
  /-- Have leaders produce new blocks immediately upon receiving transaction blocks (reduces latency by 1δ) -/
  immediateLeaderBlocks : Bool := false
  deriving Inhabited

/-- Core protocol types as mutually inductive definitions -/
mutual
  /-- Block type with all relevant fields -/
  structure Block where
    type : BlockType
    view : Int
    height : Nat
    author : ProcessId
    slot : Nat
    txs : List Transaction
    /-- QCs for blocks of height < h -/
    prev : List QC
    /-- Direct hash pointers for fastpath (only used with fastBlockPointing option) -/
    directPointers : List Block
    oneQC? : Option QC
    justification : List ViewMessage
    /-- Block hash - cached for efficiency -/
    hash : Nat
    deriving BEq, Repr, Inhabited

  /-- Quorum Certificate (QC) type -/
  structure QC where
    level : VoteLevel
    blockType : BlockType
    view : Int
    height : Nat
    author : ProcessId
    slot : Nat
    block : Block
    /-- Count of votes in the QC (for fastpath) -/
    voteCount : Nat
    /-- QC hash - cached for efficiency -/
    hash : Nat
    deriving BEq, Repr, Inhabited

  /-- View Message type -/
  structure ViewMessage where
    view : Int
    qc : QC
    deriving BEq, Repr, Inhabited
end

instance : Hashable Block where
  hash b := b.hash

instance : Hashable QC where
  hash q := q.hash

/-- Vote for a block -/
structure Vote where
  level : VoteLevel
  blockType : BlockType
  view : Int
  height : Nat
  author : ProcessId
  slot : Nat
  blockHash : Nat
  signer : ProcessId
  deriving BEq, Repr, Inhabited, Hashable

/-- Protocol messages that can be sent -/
inductive ProtocolMessage
  | block (sender : ProcessId) (b : Block)
  | vote (sender : ProcessId) (v : Vote)
  | qc (sender : ProcessId) (q : QC)
  | endView (sender : ProcessId) (view : Int)
  | viewMessage (sender : ProcessId) (vm : ViewMessage)
  | newBlock (sender : ProcessId) (b : Block)
  deriving BEq, Repr, Inhabited

def ProtocolMessage.getView : ProtocolMessage → Int
  | block _ b => b.view
  | vote _ v => v.view
  | qc _ q => q.view
  | endView _ v => v
  | viewMessage _ vm => vm.view
  | newBlock _ b => b.view

/-- Transitive closure for efficiently tracking block observation relations -/
structure TransitiveClosure where
  /-- Direct edges: blockHash → set of directly observed block hashes -/
  edges : HashMap Nat (HashSet Nat) := {}
  /-- Precomputed closure: blockHash → set of all observed block hashes -/
  closure : HashMap Nat (HashSet Nat) := {}
  deriving Inhabited

/-- Vote counter for efficiently tracking vote quorums -/
structure VoteCounter where
  level : VoteLevel
  blockHash : Nat
  /-- Set of process IDs that have voted -/
  voterSet : HashSet ProcessId := {}
  /-- Total vote count -/
  count : Nat := 0
  /-- Has a QC been formed for this counter? -/
  qcFormed : Bool := false
  deriving BEq, Repr, Inhabited

/-- Key for vote counter lookups -/
structure VoteCounterKey where
  level : VoteLevel
  blockHash : Nat
  deriving BEq, Repr, Inhabited, Hashable

/-- QC index key -/
structure QCKey where
  level : VoteLevel
  blockType : BlockType
  view : Int
  height : Nat
  author : ProcessId
  slot : Nat
  deriving BEq, Repr, Inhabited, Hashable

/-- Network message with delay -/
structure NetworkMessage where
  message : ProtocolMessage
  sender : ProcessId
  receivers : List ProcessId
  deliveryTime : Nat
  deriving Repr

/-- Effect type for protocol actions -/
inductive Effect
  | sendMessage (msg : ProtocolMessage) (recipients : List ProcessId)
  | noEffect
  deriving Repr

/-- Process state with optimized data structures -/
structure ProcessState where
  id : ProcessId

  /-- Core protocol state -/
  view : Int := 0
  leaderSlot : Nat := 0
  txSlot : Nat := 0
  viewTime : Nat := 0
  lastTxBlock? : Option Block := none

  /-- Phase tracking -/
  phase : HashMap Int Bool := {} -- View → phase (false = 0, true = 1)

  /-- Vote tracking -/
  voted : HashMap (VoteLevel × BlockType × Nat × ProcessId) Bool := {} -- For checking if already voted

  /-- Optimized data structures -/
  messages : List ProtocolMessage := [] -- Keep for backward compatibility
  messagesByView : HashMap Int (Array ProtocolMessage) := {} -- View → messages in that view

  /-- Block storage and indexing -/
  blocksByHash : HashMap Nat Block := {} -- Hash → Block
  blocksByHeight : HashMap Nat (Array Block) := {} -- Height → Blocks at that height
  blocksByTypeAndSlot : HashMap (BlockType × ProcessId × Nat) Block := {} -- (Type, Author, Slot) → Block

  /-- QC storage and indexing -/
  qcs : HashSet QC := {} -- All QCs
  qcsByBlock : HashMap Nat (Array QC) := {} -- BlockHash → QCs for that block
  qcsByKey : HashMap QCKey QC := {} -- QCKey → QC

  /-- Vote tracking -/
  voteCounters : HashMap VoteCounterKey VoteCounter := {} -- Track votes per block

  /-- Block observation tracking -/
  blockObservation : TransitiveClosure := {}

  /-- Fast path structures -/
  observedBlocks : HashSet Block := {} -- Directly observed blocks
  directlyPointedBlocks : HashSet Block := {} -- Blocks already pointed to

  /-- Complaint tracking -/
  unfinalizedQCs : HashMap QC Nat := {} -- QC → time since first seen

  deriving Inhabited

/-- Network state -/
structure NetworkState where
  currentTime : Nat := 0
  messages : List NetworkMessage := []
  gst : Nat := 100  -- Global Stabilization Time (arbitrary example)
  delta : Nat := 10  -- Message delay bound after GST
  fastpathConfig : FastpathConfig := {}

  /-- Process states indexed by ID -/
  processes : Array ProcessState := #[]
  deriving Inhabited

/-- Calculate block hash -/
def computeBlockHash (b : Block) : Nat :=
  hash (b.type, b.view, b.height, b.author, b.slot)

/-- Calculate QC hash -/
def computeQCHash (q : QC) : Nat :=
  hash (q.level, q.blockType, q.view, q.height, q.author, q.slot, q.block.hash)

/-- Create block with computed hash -/
def createBlock (type : BlockType) (view : Int) (height : Nat) (author : ProcessId) (slot : Nat)
                (txs : List Transaction := []) (prev : List QC := [])
                (directPointers : List Block := []) (oneQC? : Option QC := none)
                (justification : List ViewMessage := []) : Block :=
  let b : Block := {
    type := type,
    view := view,
    height := height,
    author := author,
    slot := slot,
    txs := txs,
    prev := prev,
    directPointers := directPointers,
    oneQC? := oneQC?,
    justification := justification,
    hash := 0 -- Placeholder
  }
  { b with hash := computeBlockHash b }

/-- Create QC with computed hash -/
def createQC (level : VoteLevel) (blockType : BlockType) (view : Int) (height : Nat)
             (author : ProcessId) (slot : Nat) (block : Block) (voteCount : Nat := 0) : QC :=
  let q : QC := {
    level := level,
    blockType := blockType,
    view := view,
    height := height,
    author := author,
    slot := slot,
    block := block,
    voteCount := voteCount,
    hash := 0 -- Placeholder
  }
  { q with hash := computeQCHash q }

/-- Genesis block definition -/
def genesis : Block :=
  createBlock BlockType.genesis (-1) 0 0 0

/-- QC for genesis block -/
def genesisQC : QC :=
  createQC VoteLevel.one BlockType.genesis (-1) 0 0 0 genesis

/-- Number of processes -/
def numProcesses : Nat := 4

/-- Byzantine threshold -/
def byzantineThreshold : Nat := (numProcesses - 1) / 3

/-- Leader of view v -/
def lead (n : Nat) (v : Int) : ProcessId :=
  v.natAbs % n

/-- QC comparison -/
def QC.lt (q1 q2 : QC) : Prop :=
  q1.view < q2.view ∨
  (q1.view = q2.view ∧ q1.blockType = BlockType.leader ∧ q2.blockType = BlockType.transaction) ∨
  (q1.view = q2.view ∧ q1.blockType = q2.blockType ∧ q1.height < q2.height)

instance : LT QC where
  lt := QC.lt

/-- QC less-than-or-equal relation -/
def QC.le (q1 q2 : QC) : Prop :=
  q1 < q2 ∨ (
    q1.view = q2.view ∧
    q1.blockType = q2.blockType ∧
    q1.height = q2.height
  )

instance : LE QC where
  le := QC.le

/-- Update transitive closure when adding a new edge -/
def TransitiveClosure.addEdge (tc : TransitiveClosure) (fromHash : Nat) (toHash : Nat) : TransitiveClosure :=
  -- Get current directly observed blocks
  let currentEdges := tc.edges.findD fromHash {}

  -- If this edge already exists, do nothing
  if currentEdges.contains toHash then
    tc
  else
    -- Add direct edge
    let newEdges := tc.edges.insert fromHash (currentEdges.insert toHash)

    -- Get current closure for this block
    let currentClosure := tc.closure.findD fromHash {}
    let toClosure := tc.closure.findD toHash {}

    -- Update closure by adding toHash and all blocks it observes
    let newClosure := tc.closure.insert fromHash (currentClosure.insert toHash |>.union toClosure)

    -- Update closures of all blocks that observe fromHash
    let newTc := { tc with edges := newEdges, closure := newClosure }

    -- Find all blocks that directly observe fromHash
    let observers := tc.edges.fold (fun observers fromHash' toSet =>
      if toSet.contains fromHash then
        fromHash' :: observers
      else
        observers
    ) []

    -- Recursively update closures for all observers
    observers.foldl (fun tc' observer =>
      -- Get updated closure for toHash
      let updatedClosure := newTc.closure.findD fromHash {}
      -- Get current closure for observer
      let observerClosure := newTc.closure.findD observer {}
      -- Update observer's closure to include all blocks observed by fromHash
      let newObserverClosure := newTc.closure.insert observer (observerClosure.union updatedClosure)
      { newTc with closure := newObserverClosure }
    ) newTc

/-- Add a block to the transitive closure -/
def TransitiveClosure.addBlock (tc : TransitiveClosure) (b : Block) : TransitiveClosure :=
  -- First ensure this block has an entry
  let tc := if tc.edges.contains b.hash then tc else
    { tc with
      edges := tc.edges.insert b.hash {},
      closure := tc.closure.insert b.hash (HashSet.empty.insert b.hash)
    }

  -- Add edges from this block to all blocks it directly points to
  let tc := b.prev.foldl (fun tc' qc =>
    TransitiveClosure.addEdge tc' b.hash qc.block.hash
  ) tc

  -- Add edges for direct pointers (fast path)
  b.directPointers.foldl (fun tc' b' =>
    TransitiveClosure.addEdge tc' b.hash b'.hash
  ) tc

/-- Check if one block observes another using transitive closure -/
def TransitiveClosure.observes (tc : TransitiveClosure) (b b' : Block) : Bool :=
  let closure := tc.closure.findD b.hash {}
  closure.contains b'.hash

/-- Check if two blocks conflict (neither observes the other) -/
def TransitiveClosure.conflicts (tc : TransitiveClosure) (b b' : Block) : Bool :=
  !(TransitiveClosure.observes tc b b') && !(TransitiveClosure.observes tc b' b)

/-- Add a vote to a vote counter -/
def VoteCounter.addVote (counter : VoteCounter) (vote : Vote) : VoteCounter :=
  if counter.voterSet.contains vote.signer then
    counter -- Already counted this voter
  else
    { counter with
      voterSet := counter.voterSet.insert vote.signer,
      count := counter.count + 1
    }

/-- Get vote counter key from vote -/
def VoteCounterKey.fromVote (v : Vote) : VoteCounterKey :=
  { level := v.level, blockHash := v.blockHash }

/-- Get vote counter key from block and level -/
def VoteCounterKey.fromBlockAndLevel (b : Block) (level : VoteLevel) : VoteCounterKey :=
  { level := level, blockHash := b.hash }

/-- Get QC key from QC -/
def QCKey.fromQC (q : QC) : QCKey :=
  {
    level := q.level,
    blockType := q.blockType,
    view := q.view,
    height := q.height,
    author := q.author,
    slot := q.slot
  }

/-- Initialize process state -/
def initProcessState (id : ProcessId) : ProcessState :=
  let emptyState : ProcessState := { id := id }

  -- Add genesis block
  let stateWithGenesis := { emptyState with
    blocksByHash := emptyState.blocksByHash.insert genesis.hash genesis,
    blocksByHeight := emptyState.blocksByHeight.insert 0 #[genesis],
    blocksByTypeAndSlot := emptyState.blocksByTypeAndSlot.insert (BlockType.genesis, 0, 0) genesis,
    blockObservation := TransitiveClosure.addBlock emptyState.blockObservation genesis,
    observedBlocks := emptyState.observedBlocks.insert genesis,
    messages := [ProtocolMessage.block 0 genesis]
  }

  -- Add genesis QC
  let qcKey := QCKey.fromQC genesisQC
  let qcsByBlock := stateWithGenesis.qcsByBlock.insert genesis.hash #[genesisQC]

  { stateWithGenesis with
    qcs := stateWithGenesis.qcs.insert genesisQC,
    qcsByBlock := qcsByBlock,
    qcsByKey := stateWithGenesis.qcsByKey.insert qcKey genesisQC,
    messagesByView := stateWithGenesis.messagesByView.insert (-1) #[ProtocolMessage.block 0 genesis]
  }

/-- Initialize network with a specified number of processes -/
def initNetwork (numProcs : Nat) (config : FastpathConfig := {}) : NetworkState :=
  let processes := Array.mkArray numProcs (initProcessState 0)
    |>.mapIdx (fun i _ => initProcessState i)

  {
    currentTime := 0,
    messages := [],
    processes := processes,
    fastpathConfig := config
  }

/-- Add message to process state with view-based indexing -/
def ProcessState.addMessage (state : ProcessState) (msg : ProtocolMessage) : ProcessState :=
  let view := msg.getView
  let viewMsgs := state.messagesByView.findD view #[]
  let newViewMsgs := viewMsgs.push msg

  { state with
    messages := msg :: state.messages,
    messagesByView := state.messagesByView.insert view newViewMsgs
  }

/-- Add block to process state with optimized indexing -/
def ProcessState.addBlock (state : ProcessState) (b : Block) : ProcessState :=
  -- Skip if already have this block
  if state.blocksByHash.contains b.hash then
    state
  else
    -- Add block to various indexes
    let heightBlocks := state.blocksByHeight.findD b.height #[]
    let newHeightBlocks := heightBlocks.push b

    -- Update transitive closure
    let newObservation := TransitiveClosure.addBlock state.blockObservation b

    -- Update state
    { state with
      blocksByHash := state.blocksByHash.insert b.hash b,
      blocksByHeight := state.blocksByHeight.insert b.height newHeightBlocks,
      blocksByTypeAndSlot := state.blocksByTypeAndSlot.insert (b.type, b.author, b.slot) b,
      blockObservation := newObservation,
      observedBlocks := state.observedBlocks.insert b
    }

/-- Add QC to process state with optimized indexing -/
def ProcessState.addQC (state : ProcessState) (q : QC) : ProcessState :=
  -- Skip if already have this QC
  if state.qcs.contains q then
    state
  else
    -- Create key for QC lookup
    let qcKey := QCKey.fromQC q

    -- Get existing QCs for this block
    let blockQCs := state.qcsByBlock.findD q.block.hash #[]
    let newBlockQCs := blockQCs.push q

    -- Update state
    { state with
      qcs := state.qcs.insert q,
      qcsByBlock := state.qcsByBlock.insert q.block.hash newBlockQCs,
      qcsByKey := state.qcsByKey.insert qcKey q
    }

/-- Get vote from vote counter key -/
def ProcessState.getVoteCounter (state : ProcessState) (key : VoteCounterKey) : VoteCounter :=
  state.voteCounters.findD key
    { level := key.level, blockHash := key.blockHash }

/-- Check if state has a quorum of votes for a block at a level -/
def ProcessState.hasVoteQuorum (state : ProcessState) (level : VoteLevel) (b : Block) : Bool :=
  let key := VoteCounterKey.fromBlockAndLevel b level
  let counter := state.getVoteCounter key
  counter.count ≥ numProcesses - byzantineThreshold

/-- Get all blocks from a view -/
def ProcessState.getBlocksInView (state : ProcessState) (view : Int) : Array Block :=
  let viewMsgs := state.messagesByView.findD view #[]
  viewMsgs.filterMap fun
    | ProtocolMessage.block _ b => if b.view = view then some b else none
    | _ => none

/-- Get all votes for a block -/
def ProcessState.getVotesForBlock (state : ProcessState) (level : VoteLevel) (b : Block) : Array Vote :=
  let viewMsgs := state.messagesByView.findD b.view #[]
  viewMsgs.filterMap fun
    | ProtocolMessage.vote _ v =>
        if v.level = level && v.blockHash = b.hash then some v else none
    | _ => none

/-- Check if a process has voted for a specific block properties -/
def ProcessState.hasVoted (state : ProcessState) (level : VoteLevel) (blockType : BlockType)
                          (slot : Nat) (author : ProcessId) : Bool :=
  state.voted.findD (level, blockType, slot, author) false

/-- Set voted flag for specific block properties -/
def ProcessState.setVoted (state : ProcessState) (level : VoteLevel) (blockType : BlockType)
                          (slot : Nat) (author : ProcessId) : ProcessState :=
  { state with voted := state.voted.insert (level, blockType, slot, author) true }

/-- Get phase for a view -/
def ProcessState.getPhase (state : ProcessState) (view : Int) : Bool :=
  state.phase.findD view false

/-- Set phase for a view -/
def ProcessState.setPhase (state : ProcessState) (view : Int) (phase : Bool) : ProcessState :=
  { state with phase := state.phase.insert view phase }

/-- Extract greatest 1-QC from state with optimized lookup -/
def ProcessState.greatest1QC (state : ProcessState) : Option QC :=
  let q1QCs := state.qcs.toArray.filter (fun q => q.level = VoteLevel.one)

  if q1QCs.isEmpty then
    none
  else
    -- Find maximum QC according to the ordering
    let maxQC := q1QCs.foldl (fun max curr =>
      if curr ≤ max then max else curr
    ) q1QCs[0]!

    some maxQC

/-- Check if a QC is a tip in this state -/
def ProcessState.isQCTip (state : ProcessState) (q : QC) : Bool :=
  state.qcs.toArray.all (fun q' =>
    -- Either q' doesn't observe q, or q also observes q'
    let qBlockObservesQ' := state.blockObservation.observes q.block q'.block
    let q'BlockObservesQ := state.blockObservation.observes q'.block q.block

    !q'BlockObservesQ || qBlockObservesQ'
  )

/-- Check if a QC is a single tip -/
def ProcessState.isQCSingleTip (state : ProcessState) (q : QC) : Bool :=
  state.qcs.toArray.all (fun q' =>
    -- q observes q'
    state.blockObservation.observes q.block q'.block
  )

/-- Check if a block is a single tip -/
def ProcessState.isBlockSingleTip (state : ProcessState) (b : Block) : Bool :=
  state.qcs.toArray.find? (fun q =>
    q.block.hash = b.hash && state.isQCSingleTip q
  ).isSome

/-- Check if a QC is finalized -/
def ProcessState.isQCFinalized (state : ProcessState) (q : QC) : Bool :=
  q.level = VoteLevel.two ||  -- Direct check for 2-QC
  (state.qcs.toArray.any (fun q' =>
    state.blockObservation.observes q'.block q.block && q'.level = VoteLevel.two
  ))

/-- Check if a block is finalized via fastpath (with just 1-QC from all processes) -/
def ProcessState.isBlockFinalizedFastpath (state : ProcessState) (b : Block) (numProcesses : Nat) : Bool :=
  state.qcs.toArray.any (fun q =>
    q.block.hash = b.hash &&
    q.level = VoteLevel.one &&
    q.voteCount = numProcesses
  )

/-- Check if a block is finalized via normal or fastpath -/
def ProcessState.isBlockFinalized (state : ProcessState) (b : Block) (config : FastpathConfig) : Bool :=
  -- Normal finalization via 2-QC
  let normalFinalized := state.qcs.toArray.any (fun q =>
    q.block.hash = b.hash && q.level = VoteLevel.two
  )

  -- Fastpath finalization via full 1-QC if enabled
  let fastpathFinalized :=
    if config.fastLeaderFinalization && b.type = BlockType.leader then
      state.isBlockFinalizedFastpath b numProcesses
    else
      false

  normalFinalized || fastpathFinalized

/-- Handle view update with optimized data structures -/
def handleViewUpdate (proc : ProcessId) (state : ProcessState) : (ProcessState × List Effect) :=
  let viewI := state.view

  -- Check for f+1 end-view messages
  let endViewMsgs := state.messages.filterMap fun
    | ProtocolMessage.endView _ view => if view ≥ viewI then some view else none
    | _ => none

  let maxEndView := if endViewMsgs.isEmpty then -1 else endViewMsgs.foldl max (-1)
  let formCert := maxEndView ≥ viewI

  -- Check for view certificate or QC with higher view
  let certViews := state.messages.filterMap fun
    | ProtocolMessage.viewMessage _ vm => if vm.view > viewI then some vm.view else none
    | _ => none

  let qcViews := state.qcs.toArray.filterMap fun q => if q.view > viewI then some q.view else none

  let maxCertView := if certViews.isEmpty then -1 else certViews.foldl max (-1)
  let maxQcView := if qcViews.isEmpty then -1 else qcViews.foldl max (-1)
  let maxView := max maxCertView maxQcView

  let updateView := maxView > viewI
  let newView := if updateView then maxView else viewI

  if formCert then
    -- Form a (v+1)-certificate and send it to all
    let viewCertMsg := ProtocolMessage.viewMessage proc {
      view := maxEndView + 1,
      qc := state.qcs.toArray[0]! -- Simplified; should select appropriate QC
    }
    let effects := [Effect.sendMessage viewCertMsg (List.range numProcesses)]
    (state, effects)
  else if updateView then
    -- Update view and send messages
    let newState := { state with view := newView }

    -- Send view certificate to all
    let viewCertMsg := ProtocolMessage.viewMessage proc {
      view := newView,
      qc := state.qcs.toArray[0]! -- Simplified; should select appropriate QC
    }

    -- Send tips to leader
    let leaderId := lead numProcesses newView

    -- Send view message to leader
    let viewMsg := ProtocolMessage.viewMessage proc {
      view := newView,
      qc := state.greatest1QC.getD state.qcs.toArray[0]!
    }

    let effects := [
      Effect.sendMessage viewCertMsg (List.range numProcesses),
      Effect.sendMessage viewMsg [leaderId]
    ]

    (newState, effects)
  else
    (state, [Effect.noEffect])

/-- Create vote for a block -/
def createVote (proc : ProcessId) (level : VoteLevel) (b : Block) : Vote :=
  Vote.mk level b.type b.view b.height b.author b.slot b.hash proc

/-- Send 0-votes for blocks with fastpath option -/
def send0Votes (proc : ProcessId) (state : ProcessState) (config : FastpathConfig) : (ProcessState × List Effect) :=
  -- Find blocks that need 0-votes efficiently using indexes
  let blocksNeedingVotes := state.blocksByHash.fold (fun acc _ b =>
    if !state.hasVoted VoteLevel.zero b.type b.slot b.author then
      b :: acc
    else
      acc
  ) []

  if blocksNeedingVotes.isEmpty then
    (state, [Effect.noEffect])
  else
    -- Create 0-votes
    let votesAndEffects := blocksNeedingVotes.map fun b =>
      let vote := createVote proc VoteLevel.zero b

      -- Send 0-vote only to block creator or to all processes if fastpath enabled
      let effect := if config.broadcast0Votes then
        Effect.sendMessage (ProtocolMessage.vote proc vote) (List.range numProcesses)
      else
        Effect.sendMessage (ProtocolMessage.vote proc vote) [b.author]

      (vote, effect)

    -- Update state with votes
    let newState := blocksNeedingVotes.foldl (fun s b =>
      s.setVoted VoteLevel.zero b.type b.slot b.author
    ) state

    -- Extract effects
    let effects := votesAndEffects.map (·.2)

    (newState, effects)

/-- Process a vote and update vote counters -/
def ProcessState.processVote (state : ProcessState) (vote : Vote) : ProcessState :=
  let key := VoteCounterKey.fromVote vote
  let counter := state.getVoteCounter key
  let newCounter := VoteCounter.addVote counter vote

  { state with voteCounters := state.voteCounters.insert key newCounter }

/-- Process votes and form QCs when threshold reached -/
def processVotes (proc : ProcessId) (state : ProcessState) : (ProcessState × List Effect) :=
  -- Find vote counters that have reached quorum but don't have QCs yet
  let countersNeedingQCs := state.voteCounters.fold (fun acc key counter =>
    if counter.count >= numProcesses - byzantineThreshold &&
       !counter.qcFormed then
      (key, counter) :: acc
    else
      acc
  ) []

  if countersNeedingQCs.isEmpty then
    (state, [Effect.noEffect])
  else
    -- Create QCs for these counters
    let qcsAndEffects := countersNeedingQCs.filterMap fun (key, counter) =>
      -- Find the block for this vote counter
      match state.blocksByHash.find? key.blockHash with
      | none => none
      | some b =>
          -- Create QC
          let qc := createQC key.level b.type b.view b.height b.author b.slot b counter.count

          -- Create effect to send QC
          let effect := Effect.sendMessage (ProtocolMessage.qc proc qc) (List.range numProcesses)

          -- Mark counter as having a QC
          let newCounter := { counter with qcFormed := true }

          some ((qc, key, newCounter), effect)

    -- Update state
    let newState := qcsAndEffects.foldl (fun s ((qc, key, newCounter), _) =>
      let s := s.addQC qc
      { s with voteCounters := s.voteCounters.insert key newCounter }
    ) state

    -- Extract effects
    let effects := qcsAndEffects.map (·.2)

    (newState, effects)

/-- Send 0-QCs for blocks with fast path optimizations -/
def send0QCs (proc : ProcessId) (state : ProcessState) : (ProcessState × List Effect) :=
  -- Find blocks with 0-quorums where auth = proc using vote counters
  let blocksWithQuorums := state.voteCounters.fold (fun acc key counter =>
    if key.level == VoteLevel.zero &&
       counter.count >= numProcesses - byzantineThreshold &&
       !counter.qcFormed then
      -- Get block from hash
      match state.blocksByHash.find? key.blockHash with
      | none => acc
      | some b => if b.author == proc then b :: acc else acc
    else
      acc
  ) []

  if blocksWithQuorums.isEmpty then
    (state, [Effect.noEffect])
  else
    -- Create 0-QCs for these blocks with vote counts
    let qcsAndEffects := blocksWithQuorums.map fun b =>
      let key := VoteCounterKey.fromBlockAndLevel b VoteLevel.zero
      let counter := state.getVoteCounter key

      -- Create QC with vote count
      let qc := createQC VoteLevel.zero b.type b.view b.height b.author b.slot b counter.count

      -- Create effect to send QC
      let effect := Effect.sendMessage (ProtocolMessage.qc proc qc) (List.range numProcesses)

      -- Mark counter as having a QC
      let newCounter := { counter with qcFormed := true }

      ((qc, key, newCounter), effect)

    -- Update state
    let newState := qcsAndEffects.foldl (fun s ((qc, key, newCounter), _) =>
      let s := s.addQC qc
      { s with voteCounters := s.voteCounters.insert key newCounter }
    ) state

    -- Extract effects
    let effects := qcsAndEffects.map (·.2)

    (newState, effects)

/-- Is payload ready (for transaction block creation)? -/
def isPayloadReady (proc : ProcessId) (state : ProcessState) : Bool :=
  let slot := state.txSlot

  -- Precondition: slot = 0 or there's a QC for previous transaction block
  slot = 0 || state.qcs.toArray.any (fun q =>
    q.author = proc &&
    q.blockType = BlockType.transaction &&
    q.slot = slot - 1
  )

/-- Make transaction block with fastpath support -/
def makeTxBlock (proc : ProcessId) (txs : List Transaction) (state : ProcessState) : Block :=
  let viewI := state.view
  let slotI := state.txSlot

  -- Find previous transaction block QC or genesis QC efficiently
  let q1Opt := if slotI > 0 then
    state.qcs.toArray.find? (fun q =>
      q.author = proc && q.blockType = BlockType.transaction && q.slot = slotI - 1)
  else
    state.qcs.toArray.find? (fun q => q.block.hash = genesis.hash && q.level = VoteLevel.one)

  -- Find single tip QC if it exists efficiently
  let q2Opt := state.qcs.toArray.find? (fun q => state.isQCSingleTip q)

  -- Calculate prev set and max height
  let prev := match q1Opt, q2Opt with
    | some q1, some q2 => if q1 = q2 then [q1] else [q1, q2]
    | some q1, none => [q1]
    | none, some q2 => [q2]
    | none, none => []

  let maxHeight := prev.foldl (fun h q => max h q.height) 0

  -- Get greatest 1-QC
  let qGreatest := state.greatest1QC

  -- Create the block
  createBlock
    BlockType.transaction
    viewI
    (maxHeight + 1)
    proc
    slotI
    txs
    prev
    []  -- No direct pointers initially
    qGreatest
    []  -- No justification for transaction blocks

/-- Handle transaction block creation with fastpath notification -/
def handleTransaction (proc : ProcessId) (txs : List Transaction) (state : ProcessState) (config : FastpathConfig) : (ProcessState × List Effect) :=
  if isPayloadReady proc state && !txs.isEmpty then
    -- Make a transaction block
    let newBlock := makeTxBlock proc txs state

    -- Effect to send block to all processes
    let blockEffect := Effect.sendMessage (ProtocolMessage.block proc newBlock) (List.range numProcesses)

    -- If fastpath is enabled, also send a notification to leader for immediate processing
    let fastpathEffects :=
      if config.immediateLeaderBlocks then
        let leaderId := lead numProcesses state.view
        if leaderId ≠ proc then  -- Don't send to self if process is the leader
          [Effect.sendMessage (ProtocolMessage.newBlock proc newBlock) [leaderId]]
        else
          []
      else
        []

    -- Update state
    let newState := {
      state with
      txSlot := state.txSlot + 1,
      lastTxBlock? := some newBlock  -- Store for fastpath
    }

    -- Add block to state's indexes
    let newState := newState.addBlock newBlock

    (newState, [blockEffect] ++ fastpathEffects)
  else
    (state, [Effect.noEffect])

/-- Is leader ready with optimized lookups? -/
def isLeaderReady (proc : ProcessId) (state : ProcessState) : Bool :=
  let v := state.view
  let leadV := lead numProcesses v
  let slotLead := state.leaderSlot

  -- Check if proc is the leader
  if proc ≠ leadV then
    false
  else
    -- Get all leader blocks for this view efficiently
    let leaderBlocksInView := state.getBlocksInView v |>.filter (fun b =>
      b.type = BlockType.leader && b.author = proc
    )

    let isFirstLeaderBlock := leaderBlocksInView.isEmpty

    if isFirstLeaderBlock then
      -- First leader block - check view messages and previous leader block
      let viewMsgCount := state.messagesByView.findD v #[] |>.filter (fun
        | ProtocolMessage.viewMessage _ vm => vm.view = v
        | _ => false
      ) |>.size

      let hasQcForPrevLeader := slotLead = 0 ||
        state.qcs.toArray.any (fun q =>
          q.blockType = BlockType.leader && q.author = proc && q.slot = slotLead - 1
        )

      viewMsgCount ≥ (numProcesses - byzantineThreshold) && hasQcForPrevLeader
    else
      -- Subsequent leader block - check for 1-QC of previous leader block
      state.qcs.toArray.any (fun q =>
        q.level = VoteLevel.one &&
        q.blockType = BlockType.leader &&
        q.author = proc &&
        q.slot = slotLead - 1
      )

/-- Check if leader should create a block in response to a new transaction block (fastpath) -/
def shouldCreateFastpathLeaderBlock (proc : ProcessId) (state : ProcessState) (b : Block) : Bool :=
  let v := state.view
  let leadV := lead numProcesses v

  -- Must be leader and in phase 0
  proc = leadV && !state.getPhase v &&
  -- Block must be a transaction block in current view
  b.type = BlockType.transaction && b.view = v &&
  -- Leader must be ready to create blocks
  isLeaderReady proc state

/-- Make leader block with fastpath support -/
def makeLeaderBlock (proc : ProcessId) (state : ProcessState) (config : FastpathConfig) : Block :=
  let viewI := state.view
  let slotLead := state.leaderSlot

  -- Find tips of QCs efficiently
  let tips := state.qcs.toArray.filter (fun q => state.isQCTip q)

  -- Add pointer to previous leader block if needed
  let prevQCOpt := if slotLead > 0 then
    state.qcs.toArray.find? (fun q =>
      q.author = proc && q.blockType = BlockType.leader && q.slot = slotLead - 1
    )
  else
    none

  let prev := match prevQCOpt with
    | some q => if tips.contains q then tips.toList else q :: tips.toList
    | none => tips.toList

  -- For fastpath, add direct pointers to recently observed blocks
  let directPointers :=
    if config.fastBlockPointing then
      -- Use observed blocks that aren't already pointed to by QCs
      state.observedBlocks.toArray.filter (fun b =>
        !state.directlyPointedBlocks.contains b &&
        !prev.any (fun q => q.block.hash = b.hash)
      ).toList
    else
      []

  -- Calculate max height
  let maxHeight := prev.foldl (fun h q => max h q.height) 0

  -- Get leader blocks in this view efficiently
  let leaderBlocksInView := state.getBlocksInView viewI |>.filter (fun b =>
    b.type = BlockType.leader && b.author = proc
  )

  -- Check if first leader block in this view
  let isFirstInView := leaderBlocksInView.isEmpty

  -- Handle justification and oneQC
  let (justification, oneQCOpt) :=
    if isFirstInView then
      -- First leader block - collect view messages
      let viewMsgs := state.messagesByView.findD viewI #[] |>.filterMap fun
        | ProtocolMessage.viewMessage _ vm => if vm.view = viewI then some vm else none
        | _ => none

      -- Find maximal 1-QC
      let bestQCOpt := state.greatest1QC

      (viewMsgs.toList, bestQCOpt)
    else
      -- Subsequent leader block - use 1-QC for previous leader block
      let prevQCOpt := state.qcs.toArray.find? (fun q =>
        q.level = VoteLevel.one &&
        q.blockType = BlockType.leader &&
        q.author = proc &&
        q.slot = slotLead - 1
      )

      ([], prevQCOpt)

  -- Create the block
  createBlock
    BlockType.leader
    viewI
    (maxHeight + 1)
    proc
    slotLead
    []  -- No transactions in leader blocks
    prev
    directPointers
    oneQCOpt
    justification

/-- Handle leader block creation with fastpath support -/
def handleLeader (proc : ProcessId) (state : ProcessState) (config : FastpathConfig) : (ProcessState × List Effect) :=
  let v := state.view
  let phaseI := state.getPhase v

  if proc = lead numProcesses v && isLeaderReady proc state && !phaseI then
    -- Check if Q_i has a single tip
    let hasSingleTip := state.qcs.toArray.any (fun q => state.isQCSingleTip q)

    if !hasSingleTip then
      -- Make leader block
      let newBlock := makeLeaderBlock proc state config

      -- Effect to send block to all processes
      let effect := Effect.sendMessage (ProtocolMessage.block proc newBlock) (List.range numProcesses)

      -- Update state
      let newPointedBlocks := if config.fastBlockPointing then
        newBlock.directPointers.foldl (fun s b => s.insert b) state.directlyPointedBlocks
      else
        state.directlyPointedBlocks

      let newState := {
        state with
        leaderSlot := state.leaderSlot + 1,
        directlyPointedBlocks := newPointedBlocks
      }

      -- Add block to state's indexes
      let newState := newState.addBlock newBlock

      (newState, [effect])
    else
      (state, [Effect.noEffect])
  else
    (state, [Effect.noEffect])

/-- Handle fastpath leader block creation in response to new transaction block -/
def handleFastpathLeaderBlock (proc : ProcessId) (state : ProcessState) (b : Block) (config : FastpathConfig) : (ProcessState × List Effect) :=
  if config.immediateLeaderBlocks && shouldCreateFastpathLeaderBlock proc state b then
    -- Store the block in observed blocks if not already there
    let stateWithBlock :=
      if state.observedBlocks.contains b then
        state
      else
        state.addBlock b

    -- Create a leader block that points to this transaction block
    let newBlock := makeLeaderBlock proc stateWithBlock config

    -- Effect to send block to all processes
    let effect := Effect.sendMessage (ProtocolMessage.block proc newBlock) (List.range numProcesses)

    -- Update state
    let newPointedBlocks := if config.fastBlockPointing then
      newBlock.directPointers.foldl (fun s b => s.insert b) stateWithBlock.directlyPointedBlocks
    else
      stateWithBlock.directlyPointedBlocks

    let newState := {
      stateWithBlock with
      leaderSlot := stateWithBlock.leaderSlot + 1,
      directlyPointedBlocks := newPointedBlocks
    }

    -- Add block to state's indexes
    let newState := newState.addBlock newBlock

    (newState, [effect])
  else
    (state, [Effect.noEffect])

/-- Vote for transaction blocks with fastpath support -/
def voteForTxBlocks (proc : ProcessId) (state : ProcessState) (config : FastpathConfig) : (ProcessState × List Effect) :=
  let v := state.view

  -- Check if there is a finalized leader block in this view efficiently
  let hasFinLeader := state.getBlocksInView v |>.any (fun b =>
    b.type = BlockType.leader && state.isBlockFinalized b config
  )

  -- Check if there is an unfinalized leader block in this view
  let hasUnfinLeader := state.getBlocksInView v |>.any (fun b =>
    b.type = BlockType.leader && !state.isBlockFinalized b config
  )

  if hasFinLeader && !hasUnfinLeader then
    -- Process 1-votes for transaction blocks efficiently
    let txNeeding1Vote := state.getBlocksInView v |>.filter (fun b =>
      b.type = BlockType.transaction &&
      state.isBlockSingleTip b &&
      (match b.oneQC? with
       | some qc => state.qcs.toArray.all (fun q => q.level ≠ VoteLevel.one || q ≤ qc)
       | none => false) &&
      !state.hasVoted VoteLevel.one BlockType.transaction b.slot b.author
    ).toList

    -- Process 2-votes for transaction blocks efficiently
    let txNeeding2Vote := state.qcs.toArray.filter (fun q =>
      q.level = VoteLevel.one && q.blockType = BlockType.transaction &&
      state.isQCSingleTip q &&
      !state.hasVoted VoteLevel.two BlockType.transaction q.slot q.author &&
      !state.blocksByHash.fold (fun found _ b => found || b.height > q.height) false
    ).toList

    if txNeeding1Vote.isEmpty && txNeeding2Vote.isEmpty then
      (state, [Effect.noEffect])
    else
      -- Create votes
      let vote1Effects := txNeeding1Vote.map (fun b =>
        let vote := createVote proc VoteLevel.one b
        Effect.sendMessage (ProtocolMessage.vote proc vote) (List.range numProcesses)
      )

      let vote2Effects := txNeeding2Vote.map (fun q =>
        let vote := createVote proc VoteLevel.two q.block
        Effect.sendMessage (ProtocolMessage.vote proc vote) (List.range numProcesses)
      )

      -- Update state
      let stateWithVotes1 := txNeeding1Vote.foldl (fun s b =>
        s.setVoted VoteLevel.one BlockType.transaction b.slot b.author
      ) state

      let stateWithVotes2 := txNeeding2Vote.foldl (fun s q =>
        s.setVoted VoteLevel.two BlockType.transaction q.slot q.author
      ) stateWithVotes1

      -- Update phase if any votes were sent
      let newState := if !txNeeding1Vote.isEmpty || !txNeeding2Vote.isEmpty then
        stateWithVotes2.setPhase v true
      else
        stateWithVotes2

      (newState, vote1Effects ++ vote2Effects)
  else
    (state, [Effect.noEffect])

/-- Vote for leader blocks with fastpath support -/
def voteForLeaderBlocks (proc : ProcessId) (state : ProcessState) (config : FastpathConfig) : (ProcessState × List Effect) :=
  let v := state.view

  -- Only vote if in phase 0
  let inPhase0 := !state.getPhase v

  if inPhase0 then
    -- Process 1-votes for leader blocks efficiently
    let leadNeeding1Vote := state.getBlocksInView v |>.filter (fun b =>
      b.type = BlockType.leader &&
      !state.hasVoted VoteLevel.one BlockType.leader b.slot b.author
    ).toList

    -- Process 2-votes for leader blocks efficiently
    let leadNeeding2Vote := state.qcs.toArray.filter (fun q =>
      q.level = VoteLevel.one && q.blockType = BlockType.leader && q.view = v &&
      !state.hasVoted VoteLevel.two BlockType.leader q.slot q.author
    ).toList

    if leadNeeding1Vote.isEmpty && leadNeeding2Vote.isEmpty then
      (state, [Effect.noEffect])
    else
      -- Create votes
      let vote1Effects := leadNeeding1Vote.map (fun b =>
        let vote := createVote proc VoteLevel.one b
        Effect.sendMessage (ProtocolMessage.vote proc vote) (List.range numProcesses)
      )

      let vote2Effects := leadNeeding2Vote.map (fun q =>
        let vote := createVote proc VoteLevel.two q.block
        Effect.sendMessage (ProtocolMessage.vote proc vote) (List.range numProcesses)
      )

      -- Update state
      let stateWithVotes1 := leadNeeding1Vote.foldl (fun s b =>
        s.setVoted VoteLevel.one BlockType.leader b.slot b.author
      ) state

      let newState := leadNeeding2Vote.foldl (fun s q =>
        s.setVoted VoteLevel.two BlockType.leader q.slot q.author
      ) stateWithVotes1

      (newState, vote1Effects ++ vote2Effects)
  else
    (state, [Effect.noEffect])

/-- Handle complaints with optimized tracking -/
def handleComplaints (proc : ProcessId) (delta : Nat) (state : ProcessState) : (ProcessState × List Effect) :=
  let v := state.view

  -- Find unfinalized QCs efficiently
  let unfinalizedQCs := state.qcs.toArray.filter (fun q =>
    !state.isQCFinalized q
  )

  -- Update unfinalized QC tracking
  let unfinalizedQCsWithTime := unfinalizedQCs.map (fun q =>
    let time := state.unfinalizedQCs.findD q 0
    let newTime := time + 1
    (q, newTime)
  )

  -- QCs not finalized for 6Δ
  let complainQCs := unfinalizedQCsWithTime.filter (fun (_, time) =>
    time ≥ 6 * delta
  )

  -- QCs not finalized for 12Δ
  let endViewQCs := unfinalizedQCsWithTime.filter (fun (_, time) =>
    time ≥ 12 * delta
  )

  -- Update tracking state
  let stateWithUpdatedTracking := { state with
    unfinalizedQCs := unfinalizedQCsWithTime.foldl (fun m (q, time) =>
      m.insert q time
    ) state.unfinalizedQCs
  }

  if !endViewQCs.isEmpty then
    -- Create end-view message
    let endViewMsg := ProtocolMessage.endView proc v
    let effect := Effect.sendMessage endViewMsg (List.range numProcesses)

    (stateWithUpdatedTracking, [effect])
  else if !complainQCs.isEmpty then
    -- Send complaint to leader
    let leaderId := lead numProcesses v

    -- Send a QC to the leader (simplified - in reality would select a specific QC)
    let qc := complainQCs[0]!.1
    let effect := Effect.sendMessage (ProtocolMessage.qc proc qc) [leaderId]

    (stateWithUpdatedTracking, [effect])
  else
    (stateWithUpdatedTracking, [Effect.noEffect])

/-- Process a new received message with optimized indexes -/
def handleMessage (proc : ProcessId) (msg : ProtocolMessage) (state : ProcessState) : ProcessState :=
  -- Add message to state with view indexing
  let state := state.addMessage msg

  -- Process specific message types
  match msg with
  | ProtocolMessage.block _ b =>
      -- Add block to state's indexes
      state.addBlock b
  | ProtocolMessage.qc _ q =>
      -- Add QC to state's indexes
      state.addQC q
  | ProtocolMessage.vote _ v =>
      -- Update vote counters
      state.processVote v
  | _ => state

/-- Type for transition functions -/
abbrev TransitionFn := ProcessId → ProcessState → FastpathConfig → ProcessState × List Effect

/-- Apply a transition and check if it produced effects -/
def applyTransition (proc : ProcessId) (state : ProcessState) (config : FastpathConfig) (transition : TransitionFn) : (ProcessState × List Effect × Bool) :=
  let (newState, effects) := transition proc state config
  let updated := effects ≠ [Effect.noEffect]
  (newState, effects, updated)

/-- Handle new block notifications for fastpath -/
def handleNewBlockNotifications (proc : ProcessId) (state : ProcessState) (config : FastpathConfig) : (ProcessState × List Effect) :=
  let newBlockMsgs := state.messages.filterMap fun
    | ProtocolMessage.newBlock _ b => some b
    | _ => none

  if newBlockMsgs.isEmpty then
    (state, [Effect.noEffect])
  else
    -- Process each new block notification
    newBlockMsgs.foldl (fun (s, effects) b =>
      let (newState, newEffects) := handleFastpathLeaderBlock proc s b config
      (newState, effects ++ newEffects)
    ) (state, [])

/-- Process a single protocol step with optimized data structures -/
def processStep (proc : ProcessId) (delta : Nat) (txs : List Transaction) (state : ProcessState) (config : FastpathConfig) : (ProcessState × List Effect) :=
  let transitions : List TransitionFn := [
    (fun p s c => handleViewUpdate p s),
    (fun p s c => send0Votes p s c),
    (fun p s c => processVotes p s),
    (fun p s c => send0QCs p s),
    (fun p s c => handleTransaction p txs s c),
    (fun p s c => handleLeader p s c),
    (fun p s c => handleNewBlockNotifications p s c),
    (fun p s c => voteForTxBlocks p s c),
    (fun p s c => voteForLeaderBlocks p s c),
    (fun p s c => handleComplaints p delta s)
  ]

  -- Apply transitions until no more updates
  let rec applyTransitions (s : ProcessState) (effects : List Effect) (transitions : List TransitionFn) : (ProcessState × List Effect) :=
    match transitions with
    | [] => (s, effects)
    | transition :: rest =>
        let (newState, newEffects, updated) := applyTransition proc s config transition
        if updated then
          -- If transition produced effects, start over from the beginning
          applyTransitions newState (effects ++ newEffects) [
            (fun p s c => handleViewUpdate p s),
            (fun p s c => send0Votes p s c),
            (fun p s c => processVotes p s),
            (fun p s c => send0QCs p s),
            (fun p s c => handleTransaction p txs s c),
            (fun p s c => handleLeader p s c),
            (fun p s c => handleNewBlockNotifications p s c),
            (fun p s c => voteForTxBlocks p s c),
            (fun p s c => voteForLeaderBlocks p s c),
            (fun p s c => handleComplaints p delta s)
          ]
        else
          -- Continue with remaining transitions
          applyTransitions newState effects rest

  let (newState, allEffects) := applyTransitions state [] transitions

  -- Update viewTime in final state
  ({ newState with viewTime := state.viewTime + 1 }, allEffects)

/-- Network message with delay -/
structure NetworkMessage where
  message : ProtocolMessage
  sender : ProcessId
  receivers : List ProcessId
  deliveryTime : Nat
  deriving Repr

/-- Deliver messages in the network with efficient processing -/
def deliverMessages (net : NetworkState) : NetworkState :=
  let currentTime := net.currentTime

  -- Find messages that should be delivered at the current time
  let (toDeliver, remaining) := net.messages.partition (·.deliveryTime ≤ currentTime)

  -- Apply each message to the recipient's state
  let processes := net.processes.mapIdx (fun i proc =>
    let messagesForProc := toDeliver.filter (fun m => m.receivers.contains i)

    -- Process each message with optimized handlers
    messagesForProc.foldl (fun p m =>
      handleMessage p.id m.message p
    ) proc
  )

  { net with
    messages := remaining,
    processes := processes
  }

/-- Calculate message delay based on network conditions -/
def calculateDelay (net : NetworkState) : Nat :=
  if net.currentTime < net.gst then
    -- Before GST, arbitrary delay (simplified)
    42
  else
    -- After GST, bounded delay
    net.delta

/-- Convert effects to network messages -/
def effectsToNetworkMessages (sender : ProcessId) (effects : List Effect) (net : NetworkState) : List NetworkMessage :=
  effects.filterMap fun
    | Effect.noEffect => none
    | Effect.sendMessage msg recipients =>
        let delay := calculateDelay net
        let deliveryTime := net.currentTime + delay
        some {
          message := msg,
          sender := sender,
          receivers := recipients,
          deliveryTime := deliveryTime
        }

/-- Execute one step of the network with optimized message processing -/
def stepNetwork (net : NetworkState) (procs : List ProcessId) (txsByProc : List (ProcessId × List Transaction)) : NetworkState :=
  -- Deliver messages
  let net := deliverMessages net

  -- Process steps and collect messages
  let newMessagesAndProcesses := procs.foldl (fun (newMsgs, procs) procId =>
    if procId >= procs.size then
      (newMsgs, procs)  -- Process not found
    else
      let proc := procs[procId]!
      let txs := txsByProc.lookup procId |>.getD []

      -- Process step
      let (newProc, effects) := processStep proc.id net.delta txs proc net.fastpathConfig

      -- Convert effects to network messages
      let netMsgs := effectsToNetworkMessages proc.id effects net

      -- Update process in the array
      let newProcs := procs.set procId newProc

      (newMsgs ++ netMsgs, newProcs)
  ) ([], net.processes)

  let (newMessages, newProcesses) := newMessagesAndProcesses

  -- Update network state
  { net with
    currentTime := net.currentTime + 1,
    messages := newMessages ++ net.messages,
    processes := newProcesses
  }

/-- Run the network for n steps -/
def runNetwork (net : NetworkState) (steps : Nat) : NetworkState :=
  if steps = 0 then
    net
  else
    -- For simplicity, have all processes take steps with no transactions
    let allProcs := List.range net.processes.size
    let noTxs := []
    let net' := stepNetwork net allProcs noTxs
    runNetwork net' (steps - 1)

/-- Example network with all fastpath options enabled -/
def fastpathNetwork : NetworkState :=
  let fastConfig : FastpathConfig := {
    broadcast0Votes := true,
    fastBlockPointing := true,
    fastLeaderFinalization := true,
    immediateLeaderBlocks := true
  }
  initNetwork numProcesses fastConfig

/-- Compute latency reduction with fastpath options -/
def computeLatencyReduction (config : FastpathConfig) : Nat :=
  let baseLatency := 8  -- Base latency is 8δ in worst case

  -- Each fastpath option can reduce latency by δ
  let reduction :=
    (if config.broadcast0Votes then 1 else 0) +
    (if config.fastBlockPointing then 1 else 0) +
    (if config.fastLeaderFinalization then 1 else 0) +
    (if config.immediateLeaderBlocks then 1 else 0)

  -- In the ideal case, all options together reduce from 8δ to 3δ
  min reduction baseLatency

/-- Extract finalized transactions from a process state -/
def extractFinalizedTransactions (state : ProcessState) (config : FastpathConfig) : List Transaction :=
  -- Find all finalized blocks
  let finalizedBlocks := state.blocksByHash.fold (fun acc _ b =>
    if state.isBlockFinalized b config && b.type = BlockType.transaction then
      b :: acc
    else
      acc
  ) []

  -- Extract transactions from finalized blocks
  finalizedBlocks.foldl (fun acc b => acc ++ b.txs) []

end MorpheusOpt
