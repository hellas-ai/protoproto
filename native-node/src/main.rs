//! Native Hellas node binary

use anyhow::Result;
use clap::{Parser, Subcommand};
use hellas_node::{Node, NodeConfig, NetworkConfig, ConsensusConfig, ProtocolConfig, HellasTicket};
use hellas_protocol::{SignedTransaction, Transaction, VerifyingKey, SigningKey, Pubkey, Amount, ObjectId};
use iroh_base::key::SecretKey;
use tokio::time::{sleep, Duration};
use tracing::{info, error, warn};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Start a new Hellas network
    Start {
        /// Port to listen on (default: random)
        #[arg(short, long)]
        port: Option<u16>,

        /// Enable mDNS discovery
        #[arg(long, default_value = "true")]
        enable_mdns: bool,

        /// Enable relay server
        #[arg(long, default_value = "true")]
        enable_relay: bool,
    },

    /// Join an existing Hellas network
    Join {
        /// Network ticket to join
        ticket: String,

        /// Port to listen on (default: random)
        #[arg(short, long)]
        port: Option<u16>,
    },

    /// Generate a new node key
    GenKey,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("hellas_node=info".parse()?)
                .add_directive("hellas_morpheus=info".parse()?)
                .add_directive("hellas_protocol=info".parse()?)
                .add_directive("iroh=warn".parse()?)
        )
        .init();

    let args = Args::parse();

    match args.command {
        Commands::Start { port, enable_mdns, enable_relay } => {
            start_node(port, enable_mdns, enable_relay).await?;
        }
        Commands::Join { ticket, port } => {
            join_network(&ticket, port).await?;
        }
        Commands::GenKey => {
            generate_key();
        }
    }

    Ok(())
}

async fn start_node(port: Option<u16>, enable_mdns: bool, enable_relay: bool) -> Result<()> {
    info!("Starting new Hellas network...");

    // Create node configuration
    let config = NodeConfig {
        network: NetworkConfig {
            secret_key: None, // Generate random
            bootstrap_nodes: vec![],
            port: port.unwrap_or(0),
            enable_mdns,
            enable_relay,
        },
        consensus: ConsensusConfig {
            n: 4,
            f: 1,
            delta_ms: 1000,
            enable_invariant_checks: false,
        },
        protocol: ProtocolConfig {
            chain_id: [1; 32],
            enable_parallel_execution: true,
            max_parallel_workers: 4,
        },
    };

    // Create and start the node
    let node = Node::new(config).await?;
    
    // Get node ID and create ticket for others to join
    let ticket = node.create_ticket()?;
    info!("Node started successfully!");
    info!("Node ID: {}", node.node_id());
    info!("");
    info!("To join this network from another node, run:");
    info!("  hellas-node join {}", ticket);
    
    // Run the node
    let (task, handle) = node.run().await?;

    // Spawn a task to periodically show status
    let status_handle = handle.clone();
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(30)).await;
            
            match status_handle.get_status().await {
                Ok(status) => {
                    info!(
                        "Status - View: {}, Finalized: {}, Pending: {}, Peers: {}",
                        status.consensus_view,
                        status.finalized_blocks,
                        status.pending_transactions,
                        status.connected_peers.len()
                    );
                }
                Err(e) => error!("Failed to get status: {}", e),
            }
        }
    });

    // Spawn a task to submit example transactions
    let tx_handle = handle.clone();
    tokio::spawn(async move {
        sleep(Duration::from_secs(10)).await;
        
        loop {
            // Create a dummy transaction
            let tx = create_example_transaction();
            
            match tx_handle.submit_transaction(tx).await {
                Ok(_effects) => info!("Transaction submitted successfully"),
                Err(e) => error!("Failed to submit transaction: {}", e),
            }
            
            sleep(Duration::from_secs(60)).await;
        }
    });

    // Wait for the node task
    info!("Node running. Press Ctrl+C to stop.");
    task.await?;

    Ok(())
}

async fn join_network(ticket_str: &str, port: Option<u16>) -> Result<()> {
    info!("Joining Hellas network...");

    // Parse the ticket
    let ticket: HellasTicket = ticket_str.parse()?;
    
    // Create node configuration
    let config = NodeConfig {
        network: NetworkConfig {
            secret_key: None, // Generate random
            bootstrap_nodes: vec![],
            port: port.unwrap_or(0),
            enable_mdns: false, // Disable mDNS when joining via ticket
            enable_relay: true,
        },
        consensus: ConsensusConfig {
            n: 4,
            f: 1,
            delta_ms: 1000,
            enable_invariant_checks: false,
        },
        protocol: ProtocolConfig {
            chain_id: [1; 32],
            enable_parallel_execution: true,
            max_parallel_workers: 4,
        },
    };
    
    // Create node
    let node = Node::new(config).await?;
    
    // Join the network
    info!("Connecting to network...");
    node.join_network(ticket).await?;
    
    info!("Successfully joined network!");
    info!("Node ID: {}", node.node_id());
    
    // Run the node
    let (task, handle) = node.run().await?;

    // Spawn status monitoring task
    let status_handle = handle.clone();
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(30)).await;
            
            match status_handle.get_status().await {
                Ok(status) => {
                    info!(
                        "Status - View: {}, Finalized: {}, Pending: {}, Peers: {}",
                        status.consensus_view,
                        status.finalized_blocks,
                        status.pending_transactions,
                        status.connected_peers.len()
                    );
                }
                Err(e) => error!("Failed to get status: {}", e),
            }
        }
    });

    // Wait for the node task
    info!("Node running. Press Ctrl+C to stop.");
    task.await?;

    Ok(())
}

fn generate_key() {
    let secret_key = SecretKey::generate();
    let public_key = secret_key.public_key();
    
    println!("Generated new node key:");
    println!("Secret key: {}", hex::encode(secret_key.to_bytes()));
    println!("Public key: {}", public_key);
}

/// Create an example transaction
fn create_example_transaction() -> SignedTransaction {
    use hellas_protocol::{Transaction as ProtocolTransaction};
    
    // Generate a test keypair
    let signing_key = SigningKey::new_random();
    let verifying_key = signing_key.verifying_key();
    
    // Create a simple transfer transaction
    let tx = ProtocolTransaction::Transfer {
        from: ObjectId::derive_from_pubkey(&Pubkey::from(verifying_key)),
        to: ObjectId::new_random(),
        amount: Amount(1000),
        nonce: 0,
    };
    
    // Sign the transaction
    SignedTransaction::new(tx, &signing_key)
} 