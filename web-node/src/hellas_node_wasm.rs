//! Hellas node WASM bindings for web environments

use hellas_node::{Node, NodeConfig, NetworkConfig, ConsensusConfig, ProtocolConfig, HellasTicket, NodeHandle};
use hellas_protocol::{SignedTransaction, Transaction as ProtocolTransaction, VerifyingKey, SigningKey, Pubkey, Amount, ObjectId};
use wasm_bindgen::prelude::*;
use js_sys::Promise;
use wasm_bindgen_futures::future_to_promise;
use tracing::{info, error};
use std::sync::{Arc, Mutex};

// Global node handle for the running node
static NODE_HANDLE: Mutex<Option<NodeHandle>> = Mutex::new(None);

/// Initialize the Hellas node in the browser
#[wasm_bindgen]
pub async fn init_hellas_node() -> Result<String, JsError> {
    // Initialize tracing for the browser console
    tracing_wasm::set_as_global_default();
    
    info!("Initializing Hellas node in browser...");
    
    // Create node configuration
    let config = NodeConfig {
        network: NetworkConfig {
            secret_key: None, // Generate random
            bootstrap_nodes: vec![],
            port: 0, // Browser will use WebRTC
            enable_mdns: false, // mDNS doesn't work in browser
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
    
    // Create and start the node
    let node = Node::new(config)
        .await
        .map_err(|e| JsError::new(&format!("Failed to create node: {}", e)))?;
    
    // Get the ticket for others to join
    let ticket = node.create_ticket()
        .map_err(|e| JsError::new(&format!("Failed to create ticket: {}", e)))?;
    
    // Run the node
    let (_task, handle) = node.run()
        .await
        .map_err(|e| JsError::new(&format!("Failed to run node: {}", e)))?;
    
    // Store the handle globally
    *NODE_HANDLE.lock().unwrap() = Some(handle);
    
    info!("Hellas node started successfully");
    Ok(ticket.to_string())
}

/// Join an existing Hellas network
#[wasm_bindgen]
pub async fn join_hellas_network(ticket_str: String) -> Result<(), JsError> {
    info!("Joining Hellas network with ticket: {}", ticket_str);
    
    // Parse the ticket
    let ticket: HellasTicket = ticket_str
        .parse()
        .map_err(|e| JsError::new(&format!("Invalid ticket: {}", e)))?;
    
    // Create node configuration
    let config = NodeConfig {
        network: NetworkConfig {
            secret_key: None, // Generate random
            bootstrap_nodes: vec![],
            port: 0,
            enable_mdns: false,
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
    let node = Node::new(config)
        .await
        .map_err(|e| JsError::new(&format!("Failed to create node: {}", e)))?;
    
    // Join the network
    node.join_network(ticket)
        .await
        .map_err(|e| JsError::new(&format!("Failed to join network: {}", e)))?;
    
    // Run the node
    let (_task, handle) = node.run()
        .await
        .map_err(|e| JsError::new(&format!("Failed to run node: {}", e)))?;
    
    // Store the handle globally
    *NODE_HANDLE.lock().unwrap() = Some(handle);
    
    info!("Successfully joined Hellas network");
    Ok(())
}

/// Submit a transaction to the network
#[wasm_bindgen]
pub fn submit_transaction(
    from_pubkey: String,
    to_object_id: String,
    amount: u64,
    nonce: u64,
) -> Promise {
    future_to_promise(async move {
        let handle = NODE_HANDLE
            .lock()
            .unwrap()
            .as_ref()
            .ok_or_else(|| JsError::new("Node not initialized"))?
            .clone();
        
        // Create a test signing key (in production, this would come from the user's wallet)
        let signing_key = SigningKey::new_random();
        let verifying_key = signing_key.verifying_key();
        
        // Parse the recipient object ID
        let to_id = ObjectId::from_hex(&to_object_id)
            .map_err(|e| JsError::new(&format!("Invalid object ID: {}", e)))?;
        
        // Create the transaction
        let tx = ProtocolTransaction::Transfer {
            from: ObjectId::derive_from_pubkey(&Pubkey::from(verifying_key)),
            to: to_id,
            amount: Amount(amount),
            nonce,
        };
        
        // Sign the transaction
        let signed_tx = SignedTransaction::new(tx, &signing_key);
        
        // Submit to the network
        let effects = handle
            .submit_transaction(signed_tx)
            .await
            .map_err(|e| JsError::new(&format!("Failed to submit transaction: {}", e)))?;
        
        // Convert effects to JS value
        let effects_json = serde_json::to_string(&effects)
            .map_err(|e| JsError::new(&format!("Failed to serialize effects: {}", e)))?;
        
        Ok(JsValue::from_str(&effects_json))
    })
}

/// Query an object from the network
#[wasm_bindgen]
pub fn query_object(object_id: String) -> Promise {
    future_to_promise(async move {
        let handle = NODE_HANDLE
            .lock()
            .unwrap()
            .as_ref()
            .ok_or_else(|| JsError::new("Node not initialized"))?
            .clone();
        
        // Parse the object ID
        let id = ObjectId::from_hex(&object_id)
            .map_err(|e| JsError::new(&format!("Invalid object ID: {}", e)))?;
        
        // Query the object
        let object = handle
            .query_object(id)
            .await
            .map_err(|e| JsError::new(&format!("Failed to query object: {}", e)))?;
        
        // Convert to JS value
        match object {
            Some(obj) => {
                let obj_json = serde_json::to_string(&obj)
                    .map_err(|e| JsError::new(&format!("Failed to serialize object: {}", e)))?;
                Ok(JsValue::from_str(&obj_json))
            }
            None => Ok(JsValue::NULL),
        }
    })
}

/// Get the current node status
#[wasm_bindgen]
pub fn get_node_status() -> Promise {
    future_to_promise(async move {
        let handle = NODE_HANDLE
            .lock()
            .unwrap()
            .as_ref()
            .ok_or_else(|| JsError::new("Node not initialized"))?
            .clone();
        
        let status = handle
            .get_status()
            .await
            .map_err(|e| JsError::new(&format!("Failed to get status: {}", e)))?;
        
        // Convert to JS value
        let status_json = serde_json::to_string(&status)
            .map_err(|e| JsError::new(&format!("Failed to serialize status: {}", e)))?;
        
        Ok(JsValue::from_str(&status_json))
    })
}

/// Shutdown the node
#[wasm_bindgen]
pub fn shutdown_node() -> Promise {
    future_to_promise(async move {
        let handle = NODE_HANDLE
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| JsError::new("Node not initialized"))?;
        
        handle
            .shutdown()
            .await
            .map_err(|e| JsError::new(&format!("Failed to shutdown: {}", e)))?;
        
        Ok(JsValue::UNDEFINED)
    })
} 