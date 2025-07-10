use anyhow::Result;
use hellas_node::{Node, NodeConfig, NetworkConfig, ConsensusConfig, ProtocolConfig};
use hellas_protocol::{SignedTransaction, Transaction, Amount, Pubkey};
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter("hellas_node=debug,hellas_morpheus=info,hellas_protocol=info")
        .init();

    // Create node configuration
    let config = NodeConfig {
        network: NetworkConfig {
            secret_key: None, // Auto-generate
            bootstrap_nodes: vec![],
            port: 0, // Auto-select
            enable_mdns: true,
            enable_relay: false,
        },
        consensus: ConsensusConfig {
            n: 4,
            f: 1,
            delta_ms: 1000,
            enable_invariant_checks: true,
        },
        protocol: ProtocolConfig {
            chain_id: [1; 32],
            enable_parallel_execution: true,
            max_parallel_workers: 4,
        },
    };

    // Create and start the node
    println!("Creating node...");
    let node = Node::new(config).await?;
    
    println!("Starting node...");
    let (task, handle) = node.run().await?;
    
    // Give the node time to initialize
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    
    // Get node status
    let status = handle.get_status().await?;
    println!("Node status: {:?}", status);
    
    // Create a test transaction
    let tx = Transaction::CreateAccount {
        initial_balance: Amount::from_units(1000),
    };
    
    let signed_tx = SignedTransaction::new_single_signer(
        Pubkey::test(1),
        tx,
        vec![],
        0,
    );
    
    println!("Submitting transaction...");
    let effects = handle.submit_transaction(signed_tx).await?;
    println!("Transaction effects: {:?}", effects);
    
    // Let it run for a bit
    println!("Node running... Press Ctrl+C to stop");
    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
    
    // Shutdown
    println!("Shutting down...");
    handle.shutdown().await?;
    task.await?;
    
    println!("Node stopped successfully");
    Ok(())
} 