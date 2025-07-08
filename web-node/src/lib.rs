//! WASM bindings for Hellas node
//! 
//! This crate provides WebAssembly bindings for running a Hellas node in the browser.

#![cfg(target_arch = "wasm32")]

use wasm_bindgen::prelude::*;

// Re-export the main WASM module
pub use crate::hellas_node_wasm::*;

mod hellas_node_wasm;

/// Initialize the WASM module
#[wasm_bindgen(start)]
pub fn init() {
    // Set up panic hook for better error messages in the browser
    console_error_panic_hook::set_once();
    
    // Initialize console logging
    tracing_wasm::set_as_global_default();
} 