## Morpheus Protocol Synopsis

Morpheus is a Byzantine Fault Tolerant (BFT) consensus protocol designed to perform efficiently in both high-throughput and low-throughput network conditions. It dynamically adapts its strategy based on observed network behavior and potential conflicts.

**Core Ideas:**

1.  **Adaptive Strategy:** Morpheus operates primarily in a leaderless, low-latency mode when blocks do not conflict. It introduces leaders only when necessary to resolve conflicts (e.g., due to network delays or high concurrent block production), similar to protocols like Autobahn.
2.  **Dual Throughput Modes:**
    *   **Low Throughput:** Aims for minimal latency. Transactions blocks can be finalized quickly (target 3δ finality) using 1-votes and 2-votes directly between processes without leader involvement.
    *   **High Throughput:** Resembles leader-based DAG protocols. Leaders produce dedicated *leader blocks* to establish a total order over *transaction blocks*. Processes primarily vote on leader blocks. Transaction blocks use 0-votes mainly for data availability and non-equivocation guarantees within a producer's block stream.
3.  **DAG Structure:** Blocks form a Directed Acyclic Graph (DAG), where blocks can point to multiple predecessors via Quorum Certificates (QCs).
4.  **Block Types:**
    *   **Genesis Block (`BlockType::Genesis`):** The root of the DAG.
    *   **Transaction Block (`BlockType::Tr`):** Contains application transactions. Produced by all participants.
    *   **Leader Block (`BlockType::Lead`):** Produced by the designated view leader during high-throughput phases to order transaction blocks.
5.  **Voting and Finalization:** Uses a system of *z-votes* (0, 1, 2) and corresponding *z-QCs* (Quorum Certificates) formed from *n-f* votes. A block is typically considered final when its 2-QC is "observed" (reachable in the DAG) by another QC.
6.  **Views and Phases:** Operates in *views*, each with a rotating leader. Each view can transition between a high-throughput phase (`Phase::High`) and a low-throughput phase (`Phase::Low`). View changes occur via PBFT-style certificate exchange (`EndView`, `EndViewCert`, `StartView` messages) if progress stalls.

## Implementation Overview

This implementation aims to be a correct and reasonably efficient representation of the Morpheus protocol.

**Key Implementation Techniques:**

1.  **State Management (`StateIndex`):** The paper describes state (`M_i`, `Q_i`) using set-theoretic definitions. For efficiency, this implementation uses indexed data structures within `StateIndex` (e.g., `BTreeMap`, `BTreeSet`). This allows for faster lookups and updates (often O(log n) or O(1)) compared to iterating over potentially large message histories.
    *   `index.blocks`: Stores received blocks, indexed by `BlockKey`.
    *   `index.qcs`: Stores formed QCs, indexed by `VoteData`.
    *   Various helper indexes (`qc_by_slot`, `qc_by_view`, `block_pointed_by`, etc.) facilitate efficient querying.
    *   `index.tips`: Tracks the current tips of the QC DAG.
    *   `index.finalized`, `index.unfinalized`, `index.unfinalized_2qc`: Track block finalization status incrementally.
2.  **Incremental Updates:** Instead of re-computing relations or sets from the entire history, state updates (like tips, finalization, max QCs) are performed incrementally as new blocks and QCs arrive (`record_block`, `record_qc`).
3.  **Quorum Tracking (`QuorumTrack`):** The `QuorumTrack` struct manages incoming partial signatures (votes) for specific data (like `VoteData` or `ViewNum`) and automatically triggers QC/Certificate formation logic when `n-f` (or `f+1` for EndView) unique votes are received.
4.  **Pending Votes (`pending_votes`):** Efficiently manages voting eligibility. Instead of scanning all blocks/QCs on every state change, potential votes are queued per view and re-evaluated only when relevant state changes occur (new block/QC, view change, finalization).
5.  **Core Logic (`MorpheusProcess`):** Encapsulates the state (`StateIndex`, `view_i`, `phase_i`, etc.) and implements the protocol logic primarily within `process_message`, `check_timeouts`, and `try_produce_blocks`.
6.  **Invariant Checking (`invariants.rs`):** Includes a comprehensive set of checks (`check_invariants`) that are run (in debug builds) after message processing to verify that the internal state remains consistent with protocol rules, despite the optimized data structures. This is crucial for catching bugs introduced by the deviation from the paper's literal state definitions.
7.  **Modular Structure:** Code is divided into modules:
    *   `types.rs`: Core data structures (Blocks, Votes, QCs, Messages, etc.).
    *   `crypto.rs`: Cryptographic types and signature verification helpers (using `hints`).
    *   `state_tracking.rs`: `StateIndex` definition and methods for updating state (`record_block`, `record_qc`).
    *   `process.rs`: `MorpheusProcess` definition and main message handling logic.
    *   `block_production.rs`: Logic for `PayloadReady`, `LeaderReady`, `MakeTrBlock`, `MakeLeaderBlock`.
    *   `block_validation.rs`: Implements block validity checks according to paper rules.
    *   `invariants.rs`: Defines and checks internal state consistency rules.
    *   `format.rs`: Helper functions for readable debug logging.
    *   `test_harness.rs`: A simulation framework for testing network interactions.

## Mapping Implementation to Paper Concepts

| Paper Concept                 | Implementation Element(s)                                     | Notes                                                                 |
| :---------------------------- | :------------------------------------------------------------ | :-------------------------------------------------------------------- |
| Process `p_i`                 | `MorpheusProcess` struct                                      | Holds all state and logic for a single process.                       |
| Message set `M_i`             | Partially represented by `index.blocks`, `index.qcs`, etc.    | Not stored explicitly; state updated incrementally. `received_messages` tracks unique message hashes for duplicate detection. |
| QC set `Q_i`                  | `index.qcs`, `index.all_1qc`, `index.unfinalized_2qc`         | Stored and indexed efficiently.                                       |
| `view_i`, `slot_i(x)`         | `view_i`, `slot_i_lead`, `slot_i_tr` fields                   | Direct mapping.                                                       |
| `voted_i(z, x, s, p_j)`       | `voted_i: BTreeSet<(u8, BlockType, SlotNum, Identity)>`       | Tracks votes cast by this process.                                    |
| `phase_i(v)`                  | `phase_i: BTreeMap<ViewNum, Phase>`                           | Tracks the phase for each view.                                       |
| `lead(v)`                     | `lead(view)` method                                           | Calculates the leader for a given view.                               |
| `PayloadReady_i`              | `payload_ready()` method                                      | Checks readiness to produce a transaction block.                      |
| `MakeTrBlock_i`               | `make_tr_block()` method                                      | Creates and sends a transaction block.                                |
| `LeaderReady_i`               | `leader_ready()` method                                       | Checks readiness to produce a leader block.                           |
| `MakeLeaderBlock_i`           | `make_leader_block()` method                                  | Creates and sends a leader block.                                     |
| `z-vote`, `z-QC`              | `VoteData`, `ThreshPartial<VoteData>`, `ThreshSigned<VoteData>` | Data structures for votes and QCs.                                    |
| `EndView`, `v-certificate`    | `Message::EndView`, `Message::EndViewCert`                    | Message types for view changes.                                       |
| `View v message`              | `Message::StartView`, `StartView` struct                      | Message sent to leader upon entering a view.                          |
| Block Validity Rules (p9,10) | `block_validation::block_valid` method                        | Implements the specified checks.                                      |
| Observes relation (`⪰` on Q_i) | `observes()` method                                           | Implements the reachability and z-level/slot comparison logic.        |
| Tips of `Q_i`                 | `index.tips: Vec<VoteData>`                                   | Maintained incrementally in `record_qc`.                              |
| Single Tip                    | `block_is_single_tip` method (helper)                         | Used in voting eligibility checks.                                    |
| Finalization                  | `index.finalized`, `index.unfinalized`, `index.unfinalized_2qc` | Tracked incrementally based on 2-QC observation.                      |
| Algorithm 1 Instructions      | Primarily implemented in `process_message` and related methods | Logic maps to the steps described in the pseudocode.                  |
| Complain Logic (Lines 56-59)  | `check_timeouts()` method                                     | Implements the 6Δ and 12Δ timeout logic.                            |
