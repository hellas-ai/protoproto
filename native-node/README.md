# native-node

Native binary for running a Hellas blockchain node.

## Features

- Start a new Hellas network or join an existing one
- P2P networking using Iroh 0.90
- Byzantine fault-tolerant consensus (Morpheus)
- Object-centric transaction execution (Hellas Protocol)

## Installation

```bash
cd native-node
cargo install --path .
```

## Usage

### Start a new network

Start the first node in a new network:

```bash
hellas-node start
```

Options:
- `--port <PORT>`: Specify the port to listen on (default: random)
- `--enable-mdns <true/false>`: Enable mDNS discovery (default: true)
- `--enable-relay <true/false>`: Enable relay server (default: true)

The node will output a ticket that others can use to join:

```
Node started successfully!
Node ID: 12D3Ko...
To join this network from another node, run:
  hellas-node join hellas:0/consensus_topic/protocol_topic/12D3Ko...
```

### Join an existing network

Join an existing network using a ticket:

```bash
hellas-node join <TICKET>
```

Options:
- `--port <PORT>`: Specify the port to listen on (default: random)

### Generate a node key

Generate a new node keypair:

```bash
hellas-node gen-key
```

This will output:
- Secret key: For node identity (keep this private!)
- Public key: Your node's public identifier

## Configuration

The node uses the following default configuration:

- **Consensus**: 4 nodes total, tolerates 1 Byzantine fault
- **Network**: WebRTC transport with optional mDNS and relay
- **Protocol**: Parallel transaction execution enabled

## Monitoring

The node will periodically log its status:

```
Status - View: 5, Finalized: 10, Pending: 3, Peers: 3
```

- **View**: Current consensus view number
- **Finalized**: Number of finalized blocks
- **Pending**: Number of pending transactions
- **Peers**: Number of connected peers

## Example Transactions

The node automatically submits example transactions every 60 seconds for testing purposes. 