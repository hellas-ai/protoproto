//! Logic components for the protocol
//!
//! This module contains the core logic for the protocol, including:
//! - Action processing
//! - Block production
//! - Timeout management
//! - View management
//! - Vote tracking

// Export all submodules
pub(crate) mod actions;
pub(crate) mod block_production;
pub(crate) mod block_validation;
pub(crate) mod effects;
pub(crate) mod processor;
pub(crate) mod state;
pub(crate) mod view_management;
pub(crate) mod voting;

// Re-export commonly used items
pub use actions::*;
pub(crate) use block_production::*;
pub use block_validation::*;
pub use effects::*;
pub use processor::*;
pub use state::*;
pub(crate) use view_management::*;
pub(crate) use voting::*;
