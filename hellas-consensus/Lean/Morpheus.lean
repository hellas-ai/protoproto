namespace Morpheus

/-- Process ID type -/
abbrev ProcessId := Nat

/-- Transaction type -/
structure Transaction where
  id : Nat
  data : String
  deriving BEq, Repr, Inhabited

/-- Block types in the protocol -/
inductive BlockType
  | genesis
  | transaction
  | leader
  deriving BEq, Repr, Inhabited

/-- Vote level -/
inductive VoteLevel
  | zero
  | one
  | two
  deriving BEq, Repr, Inhabited

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
    view : Int := 0
    height : Nat := 0
    author : ProcessId := 0
    slot : Nat := 0
    txs : List Transaction := []
    /-- QCs for blocks of height < h -/
    prev : List QC := []
    /-- Direct hash pointers for fastpath (only used with fastBlockPointing option) -/
    directPointers : List Block := []
    oneQC? : Option QC := none
    justification : List ViewMessage := []
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
    voteCount : Nat := 0
    deriving BEq, Repr, Inhabited

  /-- View Message type -/
  structure ViewMessage where
    view : Int
    qc : QC
    deriving BEq, Repr, Inhabited
end

/-- Vote for a block -/
structure Vote where
  level : VoteLevel
  blockType : BlockType
  view : Int
  height : Nat
  author : ProcessId
  slot : Nat
  blockHash : Nat  -- Simplified hash representation
  signer : ProcessId
  deriving BEq, Repr, Inhabited

/-- Protocol messages that can be sent -/
inductive ProtocolMessage
  | block (sender : ProcessId) (b : Block)
  | vote (sender : ProcessId) (v : Vote)
  | qc (sender : ProcessId) (q : QC)
  | endView (sender : ProcessId) (view : Int)
  | viewMessage (sender : ProcessId) (vm : ViewMessage)
  | newBlock (sender : ProcessId) (b : Block) -- Notification of new block for fastpath
  deriving BEq, Repr, Inhabited

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

/-- Process state -/
structure ProcessState where
  id : ProcessId
  messages : List ProtocolMessage := []
  qcs : List QC := []
  /-- Directly observed blocks for fastpath -/
  observedBlocks : List Block := []
  view : Int := 0
  leaderSlot : Nat := 0
  txSlot : Nat := 0
  -- Tracks if process has voted at level z for block type x with slot s from author p
  voted : VoteLevel → BlockType → Nat → ProcessId → Bool := fun     => false
  -- Tracks the phase (0/1) for each view
  phase : Int → Bool := fun _ => false
  -- Time since entering the current view (used for complaints)
  viewTime : Nat := 0
  -- Last transaction block received (for fast leader block creation)
  lastTxBlock? : Option Block := none
  -- Tracks blocks that have been directly pointed to (for fastBlockPointing)
  directlyPointedBlocks : List Block := []
  deriving Inhabited

/-- Network state -/
structure NetworkState where
  currentTime : Nat := 0
  messages : List NetworkMessage := []
  gst : Nat := 100  -- Global Stabilization Time (arbitrary example)
  delta : Nat := 10  -- Message delay bound after GST
  fastpathConfig : FastpathConfig := {}

  -- Process states indexed by ID
  processes : List ProcessState := []
  deriving Inhabited

/-- Genesis block definition -/
def genesis : Block :=
  { type := BlockType.genesis
    view := -1
    height := 0
    author := 0
    slot := 0 }

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

/-- Points-to relation for blocks, including direct pointers -/
def Block.pointsTo (b b' : Block) : Prop :=
  b.prev.any (fun qc => qc.block = b') || b.directPointers.contains b'

/-- Block observes relation (transitive closure of points-to) -/
inductive Block.observes : Block → Block → Prop
  | refl (b : Block) : Block.observes b b
  | step (b b' b'' : Block) :
      Block.pointsTo b b' → Block.observes b' b'' → Block.observes b b''

/-- Block conflicts relation -/
def Block.conflicts (b b' : Block) : Prop :=
  ¬Block.observes b b' ∧ ¬Block.observes b' b

/-- All blocks observed by block b -/
def Block.observedBlocks (b : Block) : Set Block :=
  { b' | Block.observes b b' }

/-- QC observes relation -/
def QC.observes (q q' : QC) (state : ProcessState) : Prop :=
  q ∈ state.qcs ∧ q' ∈ state.qcs ∧ (
    (q.blockType = q'.blockType ∧ q.author = q'.author ∧ q.slot > q'.slot) ∨
    (q.blockType = q'.blockType ∧ q.author = q'.author ∧ q.slot = q'.slot ∧
     ((q.level = VoteLevel.one ∧ q'.level = VoteLevel.zero) ∨
      (q.level = VoteLevel.two ∧ q'.level = VoteLevel.one) ∨
      (q.level = VoteLevel.two ∧ q'.level = VoteLevel.zero))) ∨
    (Block.pointsTo q.block q'.block)
  )

/-- Is the QC a tip in this state? -/
def QC.isTip (q : QC) (state : ProcessState) : Prop :=
  q ∈ state.qcs ∧ ∀ q' ∈ state.qcs, QC.observes q' q state → QC.observes q q' state

/-- Is the QC a single tip? -/
def QC.isSingleTip (q : QC) (state : ProcessState) : Prop :=
  q ∈ state.qcs ∧ ∀ q' ∈ state.qcs, QC.observes q q' state

/-- Get blocks from process state -/
def ProcessState.getBlocks (state : ProcessState) : List Block :=
  state.messages.filterMap fun
    | ProtocolMessage.block  b => some b
    |  => none

/-- Check if a block is in the state -/
def ProcessState.hasBlock (state : ProcessState) (b : Block) : Bool :=
  state.getBlocks.any (· = b) || state.observedBlocks.any (· = b)

/-- Is the block a single tip? -/
def Block.isSingleTip (b : Block) (state : ProcessState) : Prop :=
  ∃ q ∈ state.qcs,
    QC.isSingleTip q state ∧
    q.block = b ∧
    ∀ m ∈ state.messages,
      match m with
      | ProtocolMessage.block  b' => q.block = b' → b = b'
      |  => True

/-- Is the QC finalized? -/
def QC.isFinalized (q : QC) (state : ProcessState) : Prop :=
  ∃ q' ∈ state.qcs, QC.observes q' q state ∧ q.level = VoteLevel.two

/-- Check if a block is finalized via fastpath (with just 1-QC from all processes) -/
def Block.isFinalizedFastpath (b : Block) (state : ProcessState) (numProcesses : Nat) : Bool :=
  state.qcs.any (fun q =>
    q.block = b &&
    q.level = VoteLevel.one &&
    q.voteCount = numProcesses
  )

/-- Leader of view v -/
def lead (n : Nat) (v : Int) : ProcessId :=
  v.natAbs % n

/-- Number of processes -/
def numProcesses : Nat := 4  -- Example value

/-- Byzantine threshold -/
def byzantineThreshold : Nat := (numProcesses - 1) / 3

/-- Initialize process state -/
def initProcessState (id : ProcessId) : ProcessState :=
  let init1QC := {
    level := VoteLevel.one,
    blockType := BlockType.genesis,
    view := -1,
    height := 0,
    author := 0,
    slot := 0,
    block := genesis
  }
  { id := id
    messages := [ProtocolMessage.block 0 genesis]
    qcs := [init1QC]
    observedBlocks := [genesis] }

/-- Initialize network -/
def initNetwork (numProcs : Nat) (config : FastpathConfig := {}) : NetworkState :=
  { currentTime := 0
    messages := []
    processes := List.range numProcs |>.map initProcessState
    fastpathConfig := config }

/-- Get votes from process state for a given block -/
def ProcessState.getVotes (state : ProcessState) (level : VoteLevel) (b : Block) : List Vote :=
  state.messages.filterMap fun
    | ProtocolMessage.vote  v =>
        if v.level = level && v.blockType = b.type &&
           v.view = b.view && v.height = b.height &&
           v.author = b.author && v.slot = b.slot
        then some v
        else none
    |  => none

/-- Check if state has a quorum of votes of the given level for a block -/
def ProcessState.hasVoteQuorum (state : ProcessState) (level : VoteLevel) (b : Block) : Bool :=
  let votes := state.getVotes level b
  let uniqueVoters := (votes.map (·.signer)).eraseDups
  uniqueVoters.length ≥ numProcesses - byzantineThreshold

/-- Get vote count for a block at given level -/
def ProcessState.getVoteCount (state : ProcessState) (level : VoteLevel) (b : Block) : Nat :=
  let votes := state.getVotes level b
  (votes.map (·.signer)).eraseDups.length

/-- Extract greatest 1-QC from state -/
def ProcessState.greatest1QC (state : ProcessState) : Option QC :=
  let q1QCs := state.qcs.filter (fun q => q.level = VoteLevel.one)
  if q1QCs.isEmpty then
    none
  else
    -- Find maximum QC according to the ordering
    some (q1QCs.foldl (fun max curr =>
      if curr ≤ max then max else curr
    ) q1QCs.head!)

/-- Handle view update (lines 16-22 in Algorithm 1) -/
def handleViewUpdate (proc : ProcessId) (state : ProcessState) : (ProcessState × List Effect) :=
  let viewI := state.view

  -- Check for f+1 end-view messages
  let endViewMsgs := state.messages.filterMap fun
    | ProtocolMessage.endView  view => if view ≥ viewI then some view else none
    |  => none

  let maxEndView := if endViewMsgs.isEmpty then -1 else endViewMsgs.foldl max (-1)
  let formCert := maxEndView ≥ viewI

  -- Check for view certificate or QC with higher view
  let certViews := state.messages.filterMap fun
    | ProtocolMessage.viewMessage  vm => if vm.view > viewI then some vm.view else none
    |  => none

  let qcViews := state.qcs.filterMap fun q => if q.view > viewI then some q.view else none

  let maxCertView := if certViews.isEmpty then -1 else certViews.foldl max (-1)
  let maxQcView := if qcViews.isEmpty then -1 else qcViews.foldl max (-1)
  let maxView := max maxCertView maxQcView

  let updateView := maxView > viewI
  let newView := if updateView then maxView else viewI

  if formCert then
    -- Form a (v+1)-certificate and send it to all
    let viewCertMsg := ProtocolMessage.viewMessage proc {
      view := maxEndView + 1,
      qc := state.qcs.head! -- Simplified; should select appropriate QC
    }
    let effects := [Effect.sendMessage viewCertMsg (List.range numProcesses)]
    (state, effects)
  else if updateView then
    -- Update view and send messages
    let newState := { state with view := newView }

    -- Send view certificate to all
    let viewCertMsg := ProtocolMessage.viewMessage proc {
      view := newView,
      qc := state.qcs.head! -- Simplified; should select appropriate QC
    }

    -- Send tips to leader
    let leaderId := lead numProcesses newView

    -- Send view message to leader
    let viewMsg := ProtocolMessage.viewMessage proc {
      view := newView,
      qc := state.greatest1QC.getD state.qcs.head!
    }

    let effects := [
      Effect.sendMessage viewCertMsg (List.range numProcesses),
      Effect.sendMessage viewMsg [leaderId]
    ]

    (newState, effects)
  else
    (state, [Effect.noEffect])

/-- Send 0-votes for blocks (lines 24-25 in Algorithm 1) with fastpath option -/
def send0Votes (proc : ProcessId) (state : ProcessState) (config : FastpathConfig) : (ProcessState × List Effect) :=
  -- Find blocks that need 0-votes
  let blocksNeedingVotes := state.getBlocks.filter fun b =>
    !state.voted VoteLevel.zero b.type b.slot b.author

  if blocksNeedingVotes.isEmpty then
    (state, [Effect.noEffect])
  else
    -- Create 0-votes
    let effects := blocksNeedingVotes.map fun b =>
      let vote := Vote.mk
        VoteLevel.zero
        b.type
        b.view
        b.height
        b.author
        b.slot
        b.height  -- Simplified hash
        proc

      -- Send 0-vote only to block creator or to all processes if fastpath enabled
      if config.broadcast0Votes then
        Effect.sendMessage (ProtocolMessage.vote proc vote) (List.range numProcesses)
      else
        Effect.sendMessage (ProtocolMessage.vote proc vote) [b.author]

    -- Update voted function
    let newVoted := fun z x s p =>
      if z = VoteLevel.zero && blocksNeedingVotes.any (fun b =>
        b.type = x && b.slot = s && b.author = p
      ) then
        true
      else
        state.voted z x s p

    let newState := { state with voted := newVoted }

    (newState, effects)

/-- Create a QC for a block with vote count -/
def createQC (level : VoteLevel) (b : Block) (voteCount : Nat) : QC :=
  QC.mk
    level
    b.type
    b.view
    b.height
    b.author
    b.slot
    b
    voteCount

/-- Send 0-QCs for blocks (lines 26-28 in Algorithm 1) with fastpath counting -/
def send0QCs (proc : ProcessId) (state : ProcessState) : (ProcessState × List Effect) :=
  -- Find blocks with 0-quorums where auth = proc
  let blocksWithQuorums := state.getBlocks.filter fun b =>
    b.author = proc && state.hasVoteQuorum VoteLevel.zero b

  -- Check if 0-QC already sent
  let needQC := blocksWithQuorums.filter fun b =>
    !state.qcs.any (fun q => q.level = VoteLevel.zero && q.block = b)

  if needQC.isEmpty then
    (state, [Effect.noEffect])
  else
    -- Create 0-QCs for these blocks with vote counts
    let newQCs := needQC.map fun b =>
      let voteCount := state.getVoteCount VoteLevel.zero b
      createQC VoteLevel.zero b voteCount

    -- Create effect to send QCs to all processes
    let effects := newQCs.map fun qc =>
      Effect.sendMessage (ProtocolMessage.qc proc qc) (List.range numProcesses)

    -- Add QCs to local state
    let newState := { state with qcs := newQCs ++ state.qcs }

    (newState, effects)

/-- Is payload ready (for transaction block creation)? -/
def isPayloadReady (proc : ProcessId) (state : ProcessState) : Bool :=
  let slot := state.txSlot

  -- Precondition: slot = 0 or there's a QC for previous transaction block
  slot = 0 || state.qcs.any (fun q =>
    q.author = proc &&
    q.blockType = BlockType.transaction &&
    q.slot = slot - 1
  )

/-- Make transaction block with fastpath support -/
def makeTxBlock (proc : ProcessId) (txs : List Transaction) (state : ProcessState) : Block :=
  let viewI := state.view
  let slotI := state.txSlot

  -- Find previous transaction block QC or genesis QC
  let q1Opt := if slotI > 0
    then state.qcs.find? (fun q =>
      q.author = proc && q.blockType = BlockType.transaction && q.slot = slotI - 1)
    else state.qcs.find? (fun q => q.block = genesis && q.level = VoteLevel.one)

  -- Find single tip QC if it exists
  let q2Opt := state.qcs.find? (fun q => QC.isSingleTip q state)

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
  {
    type := BlockType.transaction,
    view := viewI,
    height := maxHeight + 1,
    author := proc,
    slot := slotI,
    txs := txs,
    prev := prev,
    oneQC? := qGreatest
  }

/-- Handle transaction block creation (lines 30-31 in Algorithm 1) with fastpath notification -/
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

    (newState, [blockEffect] ++ fastpathEffects)
  else
    (state, [Effect.noEffect])

/-- Is leader ready? -/
def isLeaderReady (proc : ProcessId) (state : ProcessState) : Bool :=
  let v := state.view
  let leadV := lead numProcesses v
  let slotLead := state.leaderSlot

  -- Check if proc is the leader
  if proc ≠ leadV then
    false
  else
    -- Check if first leader block or subsequent
    let isFirstLeaderBlock := !state.getBlocks.any (fun b =>
      b.type = BlockType.leader && b.view = v && b.author = proc
    )

    if isFirstLeaderBlock then
      -- First leader block - check view messages and previous leader block
      let viewMsgCount := state.messages.filterMap fun
        | ProtocolMessage.viewMessage  vm => if vm.view = v then some vm else none
        |  => none
        |>.length

      let hasQcForPrevLeader := slotLead = 0 ||
        state.qcs.any (fun q =>
          q.blockType = BlockType.leader && q.author = proc && q.slot = slotLead - 1
        )

      viewMsgCount ≥ (numProcesses - byzantineThreshold) && hasQcForPrevLeader
    else
      -- Subsequent leader block - check for 1-QC of previous leader block
      state.qcs.any (fun q =>
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
  proc = leadV && !state.phase v &&
  -- Block must be a transaction block in current view
  b.type = BlockType.transaction && b.view = v &&
  -- Leader must be ready to create blocks
  isLeaderReady proc state

/-- Make leader block with fastpath support -/
def makeLeaderBlock (proc : ProcessId) (state : ProcessState) (config : FastpathConfig) : Block :=
  let viewI := state.view
  let slotLead := state.leaderSlot

  -- Find tips of QCs
  let tips := state.qcs.filter (fun q => QC.isTip q state)

  -- Add pointer to previous leader block if needed
  let prevQCOpt := if slotLead > 0 then
    state.qcs.find? (fun q =>
      q.author = proc && q.blockType = BlockType.leader && q.slot = slotLead - 1
    )
  else
    none

  let prev := match prevQCOpt with
    | some q => if tips.contains q then tips else q :: tips
    | none => tips

  -- For fastpath, add direct pointers to recently observed blocks
  let directPointers :=
    if config.fastBlockPointing then
      -- Use observed blocks that aren't already pointed to by QCs
      state.observedBlocks.filter (fun b =>
        !state.directlyPointedBlocks.contains b &&
        !prev.any (fun q => q.block = b)
      )
    else
      []

  -- Calculate max height
  let maxHeight := prev.foldl (fun h q => max h q.height) 0

  -- Check if first leader block in this view
  let isFirstInView := !state.getBlocks.any (fun b =>
    b.type = BlockType.leader && b.view = viewI && b.author = proc
  )

  -- Handle justification and oneQC
  let (justification, oneQCOpt) :=
    if isFirstInView then
      -- First leader block - collect view messages
      let viewMsgs := state.messages.filterMap fun
        | ProtocolMessage.viewMessage  vm => if vm.view = viewI then some vm else none
        |  => none

      -- Find maximal 1-QC
      let bestQCOpt := state.greatest1QC

      (viewMsgs, bestQCOpt)
    else
      -- Subsequent leader block - use 1-QC for previous leader block
      let prevQCOpt := state.qcs.find? (fun q =>
        q.level = VoteLevel.one &&
        q.blockType = BlockType.leader &&
        q.author = proc &&
        q.slot = slotLead - 1
      )

      ([], prevQCOpt)

  -- Create the block
  {
    type := BlockType.leader,
    view := viewI,
    height := maxHeight + 1,
    author := proc,
    slot := slotLead,
    prev := prev,
    directPointers := directPointers,
    oneQC? := oneQCOpt,
    justification := justification
  }

/-- Handle leader block creation (lines 33-34 in Algorithm 1) with fastpath support -/
def handleLeader (proc : ProcessId) (state : ProcessState) (config : FastpathConfig) : (ProcessState × List Effect) :=
  let v := state.view
  let phaseI := state.phase v

  if proc = lead numProcesses v && isLeaderReady proc state && !phaseI then
    -- Check if Q_i has a single tip
    let hasSingleTip := state.qcs.any (fun q => QC.isSingleTip q state)

    if !hasSingleTip then
      -- Make leader block
      let newBlock := makeLeaderBlock proc state config

      -- Effect to send block to all processes
      let effect := Effect.sendMessage (ProtocolMessage.block proc newBlock) (List.range numProcesses)

      -- Update state
      let newPointedBlocks := if config.fastBlockPointing then
        state.directlyPointedBlocks ++ newBlock.directPointers
      else
        state.directlyPointedBlocks

      let newState := {
        state with
        leaderSlot := state.leaderSlot + 1,
        directlyPointedBlocks := newPointedBlocks
      }

      (newState, [effect])
    else
      (state, [Effect.noEffect])
  else
    (state, [Effect.noEffect])

/-- Handle fastpath leader block creation in response to new transaction block -/
def handleFastpathLeaderBlock (proc : ProcessId) (state : ProcessState) (b : Block) (config : FastpathConfig) : (ProcessState × List Effect) :=
  if config.immediateLeaderBlocks && shouldCreateFastpathLeaderBlock proc state b then
    -- Store the block in observed blocks if not already there
    let newObservedBlocks := if state.observedBlocks.contains b then
      state.observedBlocks
    else
      b :: state.observedBlocks

    let stateWithBlock := { state with observedBlocks := newObservedBlocks }

    -- Create a leader block that points to this transaction block
    let newBlock := makeLeaderBlock proc stateWithBlock config

    -- Effect to send block to all processes
    let effect := Effect.sendMessage (ProtocolMessage.block proc newBlock) (List.range numProcesses)

    -- Update state
    let newPointedBlocks := if config.fastBlockPointing then
      stateWithBlock.directlyPointedBlocks ++ newBlock.directPointers
    else
      stateWithBlock.directlyPointedBlocks

    let newState := {
      stateWithBlock with
      leaderSlot := stateWithBlock.leaderSlot + 1,
      directlyPointedBlocks := newPointedBlocks
    }

    (newState, [effect])
  else
    (state, [Effect.noEffect])

/-- Create vote for a block -/
def createVote (proc : ProcessId) (level : VoteLevel) (b : Block) : Vote :=
  Vote.mk
    level
    b.type
    b.view
    b.height
    b.author
    b.slot
    b.height  -- Simplified hash
    proc

/-- Check if a block is finalized via normal or fastpath -/
def isBlockFinalized (b : Block) (state : ProcessState) (config : FastpathConfig) : Bool :=
  -- Normal finalization via 2-QC
  let normalFinalized := state.qcs.any (fun q =>
    q.block = b && q.level = VoteLevel.two
  )

  -- Fastpath finalization via full 1-QC if enabled
  let fastpathFinalized :=
    if config.fastLeaderFinalization && b.type = BlockType.leader then
      Block.isFinalizedFastpath b state numProcesses
    else
      false

  normalFinalized || fastpathFinalized

/-- Vote for transaction blocks (lines 36-47 in Algorithm 1) with fastpath support -/
def voteForTxBlocks (proc : ProcessId) (state : ProcessState) (config : FastpathConfig) : (ProcessState × List Effect) :=
  let v := state.view

  -- Check if there is a finalized leader block in this view
  let hasFinLeader := state.getBlocks.any (fun b =>
    b.type = BlockType.leader && b.view = v &&
    isBlockFinalized b state config
  )

  -- Check if there is an unfinalized leader block in this view
  let hasUnfinLeader := state.getBlocks.any (fun b =>
    b.type = BlockType.leader && b.view = v &&
    !isBlockFinalized b state config
  )

  if hasFinLeader && !hasUnfinLeader then
    -- Process 1-votes for transaction blocks
    let txNeeding1Vote := state.getBlocks.filter (fun b =>
      b.type = BlockType.transaction && b.view = v &&
      Block.isSingleTip b state &&
      (match b.oneQC? with
       | some qc => state.qcs.all (fun q => q.level ≠ VoteLevel.one || q ≤ qc)
       | none => false) &&
      !state.voted VoteLevel.one BlockType.transaction b.slot b.author
    )

    -- Process 2-votes for transaction blocks
    let txNeeding2Vote := state.qcs.filter (fun q =>
      q.level = VoteLevel.one && q.blockType = BlockType.transaction &&
      QC.isSingleTip q state &&
      !state.voted VoteLevel.two BlockType.transaction q.slot q.author &&
      !state.getBlocks.any (fun b => b.height > q.height)
    )

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

      -- Update voted function for 1-votes
      let votedAfter1 := fun z x s p =>
        if z = VoteLevel.one && x = BlockType.transaction &&
           txNeeding1Vote.any (fun b => s = b.slot && p = b.author)
        then true
        else state.voted z x s p

      -- Update voted function for 2-votes
      let votedAfter2 := fun z x s p =>
        if z = VoteLevel.two && x = BlockType.transaction &&
           txNeeding2Vote.any (fun q => s = q.slot && p = q.author)
        then true
        else votedAfter1 z x s p

      -- Update phase
      let newPhase := if !txNeeding1Vote.isEmpty || !txNeeding2Vote.isEmpty then
        fun view => if view = v then true else state.phase view
      else
        state.phase

      let newState := {
        state with
        voted := votedAfter2,
        phase := newPhase
      }

      (newState, vote1Effects ++ vote2Effects)
  else
    (state, [Effect.noEffect])

/-- Vote for leader blocks (lines 49-54 in Algorithm 1) with fastpath support -/
def voteForLeaderBlocks (proc : ProcessId) (state : ProcessState) (config : FastpathConfig) : (ProcessState × List Effect) :=
  let v := state.view

  -- Only vote if in phase 0
  let inPhase0 := !state.phase v

  if inPhase0 then
    -- Process 1-votes for leader blocks
    let leadNeeding1Vote := state.getBlocks.filter (fun b =>
      b.type = BlockType.leader && b.view = v &&
      !state.voted VoteLevel.one BlockType.leader b.slot b.author
    )

    -- Process 2-votes for leader blocks
    let leadNeeding2Vote := state.qcs.filter (fun q =>
      q.level = VoteLevel.one && q.blockType = BlockType.leader && q.view = v &&
      !state.voted VoteLevel.two BlockType.leader q.slot q.author
    )

    if leadNeeding1Vote.isEmpty && leadNeeding2Vote.isEmpty then
      (state, [Effect.noEffect])
    else
      -- Create votes - for fastpath, count how many votes are being sent
      let vote1Effects := leadNeeding1Vote.map (fun b =>
        let vote := createVote proc VoteLevel.one b
        Effect.sendMessage (ProtocolMessage.vote proc vote) (List.range numProcesses)
      )

      let vote2Effects := leadNeeding2Vote.map (fun q =>
        let vote := createVote proc VoteLevel.two q.block
        Effect.sendMessage (ProtocolMessage.vote proc vote) (List.range numProcesses)
      )

      -- Update voted function for 1-votes
      let votedAfter1 := fun z x s p =>
        if z = VoteLevel.one && x = BlockType.leader &&
           leadNeeding1Vote.any (fun b => s = b.slot && p = b.author)
        then true
        else state.voted z x s p

      -- Update voted function for 2-votes
      let votedAfter2 := fun z x s p =>
        if z = VoteLevel.two && x = BlockType.leader &&
           leadNeeding2Vote.any (fun q => s = q.slot && p = q.author)
        then true
        else votedAfter1 z x s p

      let newState := { state with voted := votedAfter2 }

      (newState, vote1Effects ++ vote2Effects)
  else
    (state, [Effect.noEffect])

/-- Process votes to create QCs, with fastpath support -/
def processVotes (proc : ProcessId) (state : ProcessState) (config : FastpathConfig) : (ProcessState × List Effect) :=
  -- Find all the blocks for which we have votes
  let blocksWithVotes := state.messages.filterMap fun
    | ProtocolMessage.vote  v =>
        state.getBlocks.find? (fun b =>
          b.type = v.blockType && b.view = v.view &&
          b.height = v.height && b.author = v.author && b.slot = v.slot
        )
    |  => none
    |>.eraseDups

  -- For each level, check if we have enough votes to form a QC
  let newQCsAndEffects := [VoteLevel.one, VoteLevel.two].foldl (fun (qcs, effects) level =>
    -- For each block, check if we have enough votes and don't already have a QC
    let blocksNeedingQC := blocksWithVotes.filter (fun b =>
      state.hasVoteQuorum level b &&
      !state.qcs.any (fun q => q.level = level && q.block = b)
    )

    if blocksNeedingQC.isEmpty then
      (qcs, effects)
    else
      -- Create QCs with vote counts
      let newQCs := blocksNeedingQC.map (fun b =>
        let voteCount := state.getVoteCount level b
        createQC level b voteCount
      )

      -- Create effects to send QCs
      let newEffects := newQCs.map (fun qc =>
        Effect.sendMessage (ProtocolMessage.qc proc qc) (List.range numProcesses)
      )

      (qcs ++ newQCs, effects ++ newEffects)
  ) ([], [])

  let (newQCs, effects) := newQCsAndEffects

  -- If no new QCs, just return the original state
  if newQCs.isEmpty then
    (state, [Effect.noEffect])
  else
    -- Add new QCs to state
    let newState := { state with qcs := newQCs ++ state.qcs }
    (newState, effects)

/-- Handle complaints (lines 56-59 in Algorithm 1) -/
def handleComplaints (proc : ProcessId) (delta : Nat) (state : ProcessState) : (ProcessState × List Effect) :=
  let v := state.view

  -- Find unfinalized QCs
  let unfinalizedQCs := state.qcs.filter (fun q =>
    !state.qcs.any (fun q' => q'.level = VoteLevel.two && q'.block = q.block)
  )

  -- QCs not finalized for 6Δ
  let complainQCs := unfinalizedQCs.filter (fun _ =>
    state.viewTime ≥ 6 * delta
  )

  -- QCs not finalized for 12Δ
  let endViewQCs := unfinalizedQCs.filter (fun _ =>
    state.viewTime ≥ 12 * delta
  )

  if !endViewQCs.isEmpty then
    -- Create end-view message
    let endViewMsg := ProtocolMessage.endView proc v
    let effect := Effect.sendMessage endViewMsg (List.range numProcesses)

    (state, [effect])
  else if !complainQCs.isEmpty then
    -- Send complaint to leader
    let leaderId := lead numProcesses v

    -- Send a QC to the leader (simplified - in reality would select a specific QC)
    let qc := complainQCs.head!
    let effect := Effect.sendMessage (ProtocolMessage.qc proc qc) [leaderId]

    (state, [effect])
  else
    (state, [Effect.noEffect])

/-- Process a new received block, updating observed blocks -/
def processReceivedBlock (proc : ProcessId) (b : Block) (state : ProcessState) : ProcessState :=
  if state.observedBlocks.contains b then
    state
  else
    { state with observedBlocks := b :: state.observedBlocks }

/-- Type for transition functions -/
abbrev TransitionFn := ProcessId → ProcessState → FastpathConfig → ProcessState × List Effect

/-- Apply a transition and check if it produced effects -/
def applyTransition (proc : ProcessId) (state : ProcessState) (config : FastpathConfig) (transition : TransitionFn) : (ProcessState × List Effect × Bool) :=
  let (newState, effects) := transition proc state config
  let updated := effects ≠ [Effect.noEffect]
  (newState, effects, updated)

/-- Check for newBlock notifications and handle them -/
def handleNewBlockNotifications (proc : ProcessId) (state : ProcessState) (config : FastpathConfig) : (ProcessState × List Effect) :=
  let newBlockMsgs := state.messages.filterMap fun
    | ProtocolMessage.newBlock  b => some b
    |  => none

  if newBlockMsgs.isEmpty then
    (state, [Effect.noEffect])
  else
    -- Process each new block notification
    newBlockMsgs.foldl (fun (s, effects) b =>
      let (newState, newEffects) := handleFastpathLeaderBlock proc s b config
      (newState, effects ++ newEffects)
    ) (state, [])

/-- Process a single protocol step, implementing Algorithm 1 with fastpath options -/
def processStep (proc : ProcessId) (delta : Nat) (txs : List Transaction) (state : ProcessState) (config : FastpathConfig) : (ProcessState × List Effect) :=
  let transitions : List TransitionFn := [
    (fun p s c => handleViewUpdate p s),
    (fun p s c => send0Votes p s c),
    (fun p s c => send0QCs p s),
    (fun p s c => handleTransaction p txs s c),
    (fun p s c => handleLeader p s c),
    (fun p s c => handleNewBlockNotifications p s c),  -- Added for fastpath
    (fun p s c => processVotes p s c),                 -- Added for fastpath
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
            (fun p s c => send0QCs p s),
            (fun p s c => handleTransaction p txs s c),
            (fun p s c => handleLeader p s c),
            (fun p s c => handleNewBlockNotifications p s c),
            (fun p s c => processVotes p s c),
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

/-- Handle receipt of a message by a process -/
def handleMessage (proc : ProcessId) (msg : ProtocolMessage) (state : ProcessState) : ProcessState :=
  -- Add message to state
  let newState := { state with messages := msg :: state.messages }

  -- For block messages, also update observed blocks
  match msg with
  | ProtocolMessage.block  b => processReceivedBlock proc b newState
  |  => newState

/-- Deliver messages in the network -/
def deliverMessages (net : NetworkState) : NetworkState :=
  let currentTime := net.currentTime

  -- Find messages that should be delivered at the current time
  let (toDeliver, remaining) := net.messages.partition (·.deliveryTime ≤ currentTime)

  -- Apply each message to the recipient's state
  let processes := net.processes.map (fun proc =>
    let messagesForProc := toDeliver.filter (fun m => m.receivers.contains proc.id)

    -- Process each message
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

/-- Execute one step of the network -/
def stepNetwork (net : NetworkState) (procs : List ProcessId) (txsByProc : List (ProcessId × List Transaction)) : NetworkState :=
  -- Deliver messages
  let net := deliverMessages net

  -- Each process executes its step
  let processes := net.processes
  let procIdxMap := processes.map (·.id) |>.enum.map (fun (i, id) => (id, i)) |>.toList

  -- Process steps and collect messages
  let newMessagesAndProcesses := procs.foldl (fun (newMsgs, procs) procId =>
    match procIdxMap.lookup procId with
    | none => (newMsgs, procs)  -- Process not found
    | some idx =>
        let proc := procs[idx]!
        let txs := txsByProc.lookup procId |>.getD []

        -- Process step
        let (newProc, effects) := processStep proc.id net.delta txs proc net.fastpathConfig

        -- Convert effects to network messages
        let netMsgs := effectsToNetworkMessages proc.id effects net

        -- Update process in the list
        let newProcs := procs.set! idx newProc

        (newMsgs ++ netMsgs, newProcs)
  ) ([], processes)

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
    let allProcs := List.range net.processes.length
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

end Morpheus
