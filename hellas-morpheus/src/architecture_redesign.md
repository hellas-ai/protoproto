# Morpheus Architecture Redesign

## Overview

This document outlines the redesign of the Morpheus consensus protocol to improve separation of concerns, prepare for future merkalization/ZK proofs, and break up the monolithic `MorpheusProcess` structure.

## Storage Layer Architecture

### 1. Bulk Storage (Append-Only)
- **Purpose**: Store immutable consensus artifacts
- **Contents**:
  - Blocks (all types: Genesis, Transaction, Leader)
  - Quorum Certificates (QCs)
  - Votes
  - View certificates
- **Properties**:
  - Append-only semantics
  - Content-addressed (by hash)
  - Supports efficient range queries
  - Future: Merkle tree integration

### 2. Snapshot Storage
- **Purpose**: Lightweight state snapshots that reference bulk storage
- **Contents**:
  - State roots pointing to bulk storage
  - Indexed views of the DAG
  - Current consensus state
- **Properties**:
  - Periodic snapshots
  - Fast state reconstruction
  - Prunable old snapshots

### 3. Event Log
- **Purpose**: Comprehensive event sourcing for deterministic replay
- **Contents**:
  - All protocol events in order
  - External inputs (transactions, timeouts)
  - State transitions
- **Properties**:
  - Strictly ordered
  - Deterministic replay capability
  - Supports debugging and auditing

## Component Architecture

### Core Components

#### 1. ProcessIdentity
```rust
struct ProcessIdentity {
    id: Identity,
    keybook: KeyBook,
    chainid: [u8; 32],
    consensus_params: ConsensusParams,
}

struct ConsensusParams {
    n: u32,      // Total processes
    f: u32,      // Max faulty
    delta: u128, // Network delay bound
}
```

#### 2. ViewManager
- Manages view transitions
- Tracks current view and phase
- Handles view certificates
- Manages leader election

#### 3. VoteManager
- Tracks votes and forms quorums
- Manages voting eligibility
- Handles vote deduplication
- Maintains pending votes

#### 4. BlockProducer
- Creates transaction blocks
- Creates leader blocks
- Manages block slots
- Validates block construction

#### 5. DAGIndex
- Maintains block relationships
- Tracks DAG tips
- Implements observes relation
- Manages block pointers

#### 6. QCIndex
- Tracks all QCs
- Manages finalization state
- Maintains unfinalized QCs
- Tracks max QCs by type

#### 7. TimeoutManager
- Tracks protocol timeouts
- Sends complaints
- Triggers view changes
- Manages time-based transitions

#### 8. MessageHandler
- Processes incoming messages
- Routes to appropriate components
- Manages message deduplication
- Coordinates responses

### Storage Components

#### 1. BulkStore
```rust
trait BulkStore {
    async fn append_block(&mut self, block: Arc<Signed<Block<Tr>>>) -> Result<BlockRef>;
    async fn append_qc(&mut self, qc: FinishedQC) -> Result<QCRef>;
    async fn get_block(&self, ref: BlockRef) -> Result<Arc<Signed<Block<Tr>>>>;
    async fn get_qc(&self, ref: QCRef) -> Result<FinishedQC>;
}
```

#### 2. SnapshotStore
```rust
trait SnapshotStore {
    async fn save_snapshot(&mut self, state: ConsensusState) -> Result<StateRoot>;
    async fn load_snapshot(&self, root: StateRoot) -> Result<ConsensusState>;
    async fn get_latest_snapshot(&self) -> Result<Option<(StateRoot, ConsensusState)>>;
}
```

#### 3. EventStore
```rust
trait EventStore {
    async fn append_event(&mut self, event: Event<Tr>) -> Result<EventId>;
    async fn replay_events(&self, from: EventId, to: EventId) -> Result<Vec<Event<Tr>>>;
    async fn get_event_count(&self) -> Result<u64>;
}
```

## Data Flow

1. **Message Reception**
   - MessageHandler receives message
   - Deduplication check
   - Event logged to EventStore
   - Routed to appropriate component

2. **Block Production**
   - BlockProducer creates block
   - Block appended to BulkStore
   - DAGIndex updated
   - Event logged

3. **Voting**
   - VoteManager checks eligibility
   - Vote created and broadcast
   - QuorumTrack updated
   - QC formed when quorum reached

4. **State Snapshots**
   - Periodic snapshot triggered
   - Current state serialized
   - References to bulk storage included
   - Snapshot saved with state root

## Migration Path

1. **Phase 1**: Extract indices from StateIndex
2. **Phase 2**: Extract managers from MorpheusProcess
3. **Phase 3**: Implement new storage traits
4. **Phase 4**: Wire components together
5. **Phase 5**: Add merkalization support

## Future Considerations

- ZK proof generation for state transitions
- Merkle tree integration in BulkStore
- Pruning strategies for old data
- Cross-chain state verification 