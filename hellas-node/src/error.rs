use thiserror::Error;

/// Errors that can occur in the Hellas node
#[derive(Error, Debug)]
pub enum NodeError {
    /// Network-related errors
    #[error("Network error: {0}")]
    Network(#[from] NetworkError),
    
    /// Consensus-related errors
    #[error("Consensus error: {0}")]
    Consensus(String),
    
    /// Protocol-related errors
    #[error("Protocol error: {0}")]
    Protocol(#[from] hellas_protocol::ExecutionError),
    
    /// Configuration errors
    #[error("Configuration error: {0}")]
    Config(String),
    
    /// IO errors
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    /// Serialization errors
    #[error("Serialization error: {0}")]
    Serialization(String),
}

/// Network-specific errors
#[derive(Error, Debug)]
pub enum NetworkError {
    /// Iroh endpoint error
    #[error("Endpoint error: {0}")]
    Endpoint(String),
    
    /// Connection error
    #[error("Connection error: {0}")]
    Connection(String),
    
    /// Gossip error
    #[error("Gossip error: {0}")]
    Gossip(String),
    
    /// Invalid node ID
    #[error("Invalid node ID: {0}")]
    InvalidNodeId(String),
    
    /// Timeout
    #[error("Operation timed out")]
    Timeout,
}

/// Result type for node operations
pub type NodeResult<T> = Result<T, NodeError>;

/// Result type for network operations
pub type NetworkResult<T> = Result<T, NetworkError>; 