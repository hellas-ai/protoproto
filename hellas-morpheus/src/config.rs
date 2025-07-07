use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct ConsensusConfig {
    /// Total number of processes in the system
    pub n: u32,
    /// Maximum number of faulty processes tolerated (n-f is the quorum size)
    pub f: u32,
    /// Network delay parameter (Δ in pseudocode)
    /// Used for timeouts in the protocol (6Δ and 12Δ)
    pub delta: u128,
}
