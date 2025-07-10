use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Configuration for a Hellas node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    /// Network configuration
    pub network: NetworkConfig,
    
    /// Consensus configuration
    pub consensus: ConsensusConfig,
    
    /// Protocol configuration
    pub protocol: ProtocolConfig,
}

/// Network configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Optional secret key for the node (will be generated if not provided)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_key: Option<String>,
    
    /// Bootstrap nodes to connect to
    #[serde(default)]
    pub bootstrap_nodes: Vec<String>,
    
    /// Port to bind to (0 for automatic)
    #[serde(default)]
    pub port: u16,
    
    /// Enable mDNS discovery
    #[serde(default = "default_true")]
    pub enable_mdns: bool,
    
    /// Enable relay server
    #[serde(default)]
    pub enable_relay: bool,
}

/// Consensus configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusConfig {
    /// Total number of validators
    pub n: u32,
    
    /// Maximum number of faulty validators
    pub f: u32,
    
    /// Network delay parameter (Δ in milliseconds)
    #[serde(default = "default_delta")]
    pub delta_ms: u128,
    
    /// Enable invariant checking
    #[serde(default)]
    pub enable_invariant_checks: bool,
}

/// Protocol configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolConfig {
    /// Chain ID
    pub chain_id: [u8; 32],
    
    /// Enable parallel execution
    #[serde(default = "default_true")]
    pub enable_parallel_execution: bool,
    
    /// Maximum parallel workers
    #[serde(default = "default_parallel_workers")]
    pub max_parallel_workers: usize,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            network: NetworkConfig::default(),
            consensus: ConsensusConfig::default(),
            protocol: ProtocolConfig::default(),
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            secret_key: None,
            bootstrap_nodes: Vec::new(),
            port: 0,
            enable_mdns: true,
            enable_relay: false,
        }
    }
}

impl Default for ConsensusConfig {
    fn default() -> Self {
        Self {
            n: 4,
            f: 1,
            delta_ms: 1000,
            enable_invariant_checks: false,
        }
    }
}

impl Default for ProtocolConfig {
    fn default() -> Self {
        Self {
            chain_id: [0u8; 32],
            enable_parallel_execution: true,
            max_parallel_workers: 4,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_delta() -> u128 {
    1000
}

fn default_parallel_workers() -> usize {
    4
} 