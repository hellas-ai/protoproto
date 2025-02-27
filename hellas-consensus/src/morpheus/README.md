# Morpheus Consensus

A Rust implementation of the Morpheus Consensus protocol, which efficiently transitions between low and high throughput conditions to provide optimal latency in both settings.

## Overview

Morpheus Consensus is designed to provide the best of both worlds:

- During **low throughput**: A leaderless blockchain protocol with just 3δ latency
- During **high throughput**: A leader-based DAG protocol with competitive latency and communication complexity

The protocol naturally morphs between these two modes as needed, excelling in both settings without compromising performance.

## Features

- **Adaptive Performance**: Automatically transitions between low and high throughput modes
- **Low Latency**: 3δ latency in low throughput mode, competitive with state-of-the-art in high throughput
- **Quiescent**: No message overhead when no transactions are being processed
- **Leaderless When Possible**: Avoids the overhead of leaders during low throughput
- **Seamless Recovery**: Rapid recovery from asynchronous periods
- **Byzantine Fault Tolerant**: Resilient to up to f < n/3 Byzantine failures
- **Efficient Communication**: Linear amortized communication complexity

## Key Concepts

### Block Types
- **Transaction Blocks**: Contain the actual transactions
- **Leader Blocks**: Used to resolve conflicts and order transaction blocks during high throughput

### Voting Process
- **0-votes**: For data availability and non-equivocation during high throughput
- **1-votes**: First round of voting on blocks
- **2-votes**: Second round to finalize blocks

### Views and Phases
- **Views**: Periods with a specific leader
- **Phases**: Each view has two possible phases:
  - Phase 0: High throughput mode with leader blocks
  - Phase 1: Low throughput mode with direct transaction block finalization

## Usage

Basic usage:

```rust
use morpheus::{Morpheus, MorpheusConfig};
use morpheus::types::{ProcessId, Transaction};
use std::time::Duration;

// Create a configuration
let config = MorpheusConfig {
    process_id: ProcessId(0),      // This node's ID
    num_processes: 4,              // Total nodes in the network
    f: 1,                          // Can tolerate 1 Byzantine fault
    delta: Duration::from_millis(500), // Message delay bound
};

// Create a Morpheus instance
let mut morpheus = Morpheus::new(config);

// Add transactions
let transaction = Transaction {
    data: b"Example transaction".to_vec(),
};
morpheus.add_transaction(transaction);

// Run the protocol (in a real application, this would be in a loop)
morpheus.step();

// Get the ordered log of transactions
let log = morpheus.get_log();
```

## Implementation Details

This implementation uses the `muchin` crate to structure the protocol as a state machine with pure and effectful components. The key modules are:

- `types.rs`: Core type definitions
- `state.rs`: Complete state structure with incremental updates
- `actions.rs`: Protocol actions (pure and effectful)
- `model.rs`: Core protocol logic
- `ordering.rs`: Implementation of the total ordering function

## Differences from the Paper

This implementation follows the paper closely, with a few practical adjustments:

1. We use a unified state structure rather than separate components
2. All indices are incrementally maintained for efficient operations
3. Deterministic ordering is ensured with BTreeMap/BTreeSet
4. The implementation provides an ExtractableSMR interface

## Performance Characteristics

As described in the paper, Morpheus achieves:

- **Low Throughput**: 3δ latency, better than protocols like PBFT, Tendermint, and Autobahn
- **High Throughput**: Latency matching Autobahn and Sailfish
- **Linear Amortized Communication**: Without requiring batching or erasure coding

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## References

- [Morpheus Consensus: Excelling on Trails and Autobahns](https://arxiv.org/abs/2502.08465)