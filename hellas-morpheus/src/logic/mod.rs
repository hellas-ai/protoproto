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
pub(crate) mod block_producer;
pub(crate) mod block_validation;
pub(crate) mod effects;
pub(crate) mod processor;
pub(crate) mod timeout_manager;
pub(crate) mod view_manager;
pub(crate) mod vote_manager;

// Re-export commonly used items
pub use actions::*;
pub use block_producer::*;
pub use block_validation::*;
pub use effects::*;
pub use processor::process_action;
pub use timeout_manager::*;
pub use view_manager::*;
pub use vote_manager::*;