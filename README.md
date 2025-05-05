# Hellas Protocol Prototyping Sandbox

Here will be the garden where we grow the `hellas-protocol` crate while experimenting with protocol designs and implementations.

Guiding principles:

- High throughput
- Minimize coordination (avoid the chain in the hotpath)
- Fixed-functionality over needless flexibility

## Crates here

- [hellas-morpheus](./hellas-morpheus) is the consensus implementation
- [morpheus-viz](./morpheus-viz) is the interactive explainer & debugger for morpheus
- [hellas-protocol](./hellas-protocol) is the data types / state machines implementing the protocol
- [native-node](./native-node) is the real node
- [web-node](./web-node) puts the node in a browser
- [hades](./hades) is the block explorer / debugging interface
