# Hellas Browser Demo

A simple web application demonstrating how to run a Hellas blockchain node directly in the browser using WebAssembly.

## Features

- Start a new blockchain network
- Join an existing network using a ticket
- Submit transactions
- Monitor node status in real-time

## Prerequisites

- Rust with `wasm32-unknown-unknown` target
- wasm-pack
- Python 3 (for the simple HTTP server)

## Setup

1. Install prerequisites:
```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-pack
```

2. Build the WASM module:
```bash
npm run build
```

This will create a `pkg/` directory with the compiled WASM module and JavaScript bindings.

3. Start the web server:
```bash
npm run serve
```

4. Open http://localhost:8080 in your browser

## Usage

### Starting a New Network

1. Click "Start New Network"
2. Copy the generated ticket
3. Share the ticket with others to allow them to join your network

### Joining an Existing Network

1. Click "Join Network"
2. Paste the network ticket
3. Click "Connect"

### Submitting Transactions

1. Enter your signing key (hex format)
2. Enter the recipient's object ID (hex format)
3. Enter the amount and nonce
4. Click "Submit Transaction"

## Browser Console

The demo logs debug information to the browser console. Open the developer tools to see:
- Network events
- Consensus messages
- Transaction processing

## Security Note

This is a demo application. In production:
- Never expose private keys in the UI
- Use secure key management
- Implement proper authentication 