# web-node

WebAssembly bindings for running a Hellas blockchain node in the browser.

## Features

- Run a full Hellas node directly in the browser
- WebRTC-based P2P networking using Iroh 0.90
- Same consensus and protocol as native nodes
- JavaScript/TypeScript API for web applications

## Building

Build the WASM module:

```bash
cd web-node
wasm-pack build --target web
```

This creates a `pkg/` directory with the generated JavaScript bindings.

## Usage in Web Applications

### Basic Setup

```html
<!DOCTYPE html>
<html>
<head>
    <script type="module">
        import init, { 
            init_hellas_node, 
            join_hellas_network, 
            submit_transaction,
            get_node_status 
        } from './pkg/web_node.js';

        async function main() {
            // Initialize WASM module
            await init();

            // Start a new node
            const ticket = await init_hellas_node();
            console.log('Node started! Ticket:', ticket);
            
            // Or join existing network
            // const nodeId = await join_hellas_network(ticket);
        }

        main();
    </script>
</head>
<body>
    <h1>Hellas Node in Browser</h1>
</body>
</html>
```

### API Reference

#### `init_hellas_node() -> Promise<string>`
Start a new Hellas network and return a ticket for others to join.

```javascript
const ticket = await init_hellas_node();
// Returns: "hellas:0/consensus_topic/protocol_topic/12D3Ko..."
```

#### `join_hellas_network(ticket: string) -> Promise<string>`
Join an existing network using a ticket.

```javascript
const nodeId = await join_hellas_network(ticket);
// Returns: "12D3Ko..." (your node ID)
```

#### `submit_transaction(from_key: string, to: string, amount: bigint, nonce: bigint) -> Promise<string>`
Submit a transfer transaction.

```javascript
const txHash = await submit_transaction(
    "0x1234...",  // Signing key (hex)
    "0xabcd...",  // Recipient object ID (hex)
    1000n,        // Amount
    1n            // Nonce
);
```

#### `get_node_status() -> Promise<Object>`
Get the current node status.

```javascript
const status = await get_node_status();
console.log(status);
// {
//   node_id: "12D3Ko...",
//   view: 5,
//   finalized_blocks: 10,
//   pending_transactions: 3,
//   connected_peers: 3
// }
```

## Example Application

See the `examples/browser-demo/` directory for a complete example web application.

## Security Considerations

- The node's secret key is generated in the browser and stored in memory
- Tickets contain network topology information - share carefully
- WebRTC connections are encrypted but browser security policies apply

## Browser Compatibility

- Chrome/Edge 90+
- Firefox 88+
- Safari 15+ (with WebRTC enabled)

## Development

### Running Tests

```bash
wasm-pack test --headless --chrome
```

### Debug Logging

The WASM module uses `tracing-wasm` for logging to the browser console. Enable debug logging:

```javascript
// In browser console
localStorage.setItem('RUST_LOG', 'hellas_node=debug,hellas_morpheus=info');
``` 