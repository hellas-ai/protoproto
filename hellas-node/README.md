# hellas-node

A unified library crate that integrates [Iroh 0.90](https://github.com/n0-computer/iroh), [hellas-morpheus](../hellas-morpheus) consensus, and [hellas-protocol](../hellas-protocol) into a complete blockchain node implementation.

This crate is designed to be used by both native and WASM deployable artifacts:
- [native-node](../native-node) - Native binary for running nodes on servers
- [web-node](../web-node) - WASM library for running nodes in browsers

## Features

- **Iroh 0.90 Networking**: Uses Iroh's gossip protocol for peer-to-peer communication
- **Morpheus Consensus**: Byzantine fault-tolerant consensus with high throughput
- **Hellas Protocol**: Object-centric execution layer optimized for AI compute workloads
- **Unified API**: Same interface works in both native and browser environments

## Architecture

The crate is organized into several modules:

- `config` - Configuration structures for network, consensus, and protocol settings
- `error` - Error types and handling
- `messages` - Network message types and serialization
- `network` - Iroh-based networking implementation
- `node` - High-level node implementation that ties everything together

## Usage

### Native Node Example

```rust
use hellas_node::{Node, NodeConfig, NetworkConfig, ConsensusConfig, ProtocolConfig};
use iroh_base::key::SecretKeyOption;

#[tokio::main]
async fn main() -> Result<()> {
    // Create node configuration
    let config = NodeConfig {
        network: NetworkConfig {
            secret_key: SecretKeyOption::Random,
            bootstrap_nodes: vec![],
            port: Some(11111),
            enable_mdns: true,
            enable_relay: true,
        },
        consensus: ConsensusConfig {
            n: 4,  // Total number of nodes
            f: 1,  // Maximum Byzantine faults tolerated
            delta: 10,  // Timeout parameter
        },
        protocol: ProtocolConfig {
            chain_id: [1; 32],
            enable_parallel_execution: true,
        },
    };

    // Create and start the node
    let node = Node::new(config).await?;
    
    // Get ticket for others to join
    let ticket = node.create_ticket()?;
    println!("Join with ticket: {}", ticket);
    
    // Run the node
    let (task, handle) = node.run().await?;
    
    // Use the handle to interact with the node
    // handle.submit_transaction(...).await?;
    // handle.query_object(...).await?;
    
    task.await?;
    Ok(())
}
```

### Web Node Example

```javascript
import init, { 
    init_hellas_node, 
    join_hellas_network,
    submit_transaction,
    get_node_status 
} from './hellas_node_wasm.js';

async function startNode() {
    // Initialize WASM
    await init();
    
    // Start a new node
    const ticket = await init_hellas_node();
    console.log('Node started. Join with ticket:', ticket);
    
    // Or join existing network
    // await join_hellas_network(ticket);
    
    // Get node status
    const status = await get_node_status();
    console.log('Node status:', JSON.parse(status));
    
    // Submit a transaction
    const effects = await submit_transaction(
        "pubkey_hex",
        "to_object_id_hex", 
        1000,  // amount
        0      // nonce
    );
    console.log('Transaction effects:', JSON.parse(effects));
}
```

## Network Architecture

The node uses a dual-topic gossip network:
- **Consensus Topic**: For Morpheus consensus messages (blocks, votes, QCs)
- **Protocol Topic**: For Hellas protocol messages (transactions, object queries)

Messages are signed using Iroh's cryptography and verified on receipt.

## Configuration

### Network Configuration

- `secret_key`: Node's identity key (generate random or provide existing)
- `bootstrap_nodes`: List of initial peers to connect to
- `port`: UDP port for networking (None = auto-select)
- `enable_mdns`: Enable local peer discovery
- `enable_relay`: Enable relay servers for NAT traversal

### Consensus Configuration

- `n`: Total number of nodes in the network
- `f`: Maximum Byzantine faults tolerated (must be < n/3)
- `delta`: Timeout parameter for view changes

### Protocol Configuration

- `chain_id`: Unique identifier for the blockchain
- `enable_parallel_execution`: Enable parallel transaction execution

## Development

To run the native example:
```bash
cd native-node
cargo run --example hellas_node_example
```

To build the WASM module:
```bash
cd web-node
wasm-pack build --target web
```

## License

Apache-2.0 