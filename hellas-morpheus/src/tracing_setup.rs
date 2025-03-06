use tracing::{debug, error, info, warn, Level};
use tracing_subscriber::prelude::*;
use tracing_subscriber::{EnvFilter, Registry};

#[cfg(target_arch = "wasm32")]
use tracing_wasm;

/// Initialize tracing infrastructure based on environment
pub fn init_tracing() {
    #[cfg(target_arch = "wasm32")]
    {
        // Set up WASM-specific tracing for browser integration
        console_error_panic_hook::set_once();
        tracing_wasm::set_as_global_default();
        info!("Tracing initialized for WebAssembly target");
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        // Set up non-WASM tracing (for tests and native running)
        let env_filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("hellas_morpheus=debug"));

        let subscriber = Registry::default()
            .with(env_filter)
            .with(tracing_subscriber::fmt::layer().with_target(true));

        tracing::subscriber::set_global_default(subscriber)
            .expect("Failed to set global default tracing subscriber");
        
        info!("Tracing initialized for native target");
    }
}

/// Register a new Morpheus process with tracing
pub fn register_process(id: &crate::Identity, n: usize, f: usize) {
    info!(process_id = ?id, total_processes = n, max_faulty = f, "Creating new Morpheus process");
}

/// Track protocol transitions such as view changes
pub fn protocol_transition(
    process_id: &crate::Identity, 
    transition_type: &str, 
    from: impl std::fmt::Debug, 
    to: impl std::fmt::Debug,
    reason: Option<&str>
) {
    if let Some(reason) = reason {
        info!(
            process_id = ?process_id,
            transition = transition_type,
            from = ?from,
            to = ?to,
            reason = reason,
            "Protocol state transition"
        );
    } else {
        info!(
            process_id = ?process_id,
            transition = transition_type,
            from = ?from,
            to = ?to,
            "Protocol state transition"
        );
    }
}

/// Track message sending for visualization
pub fn message_sent(
    from: &crate::Identity,
    to: Option<&crate::Identity>,
    message_type: &str,
    message: impl std::fmt::Debug,
) {
    if let Some(to) = to {
        debug!(
            from = ?from,
            to = ?to,
            message_type = message_type,
            message = ?message,
            "Message sent"
        );
    } else {
        debug!(
            from = ?from,
            to = "broadcast",
            message_type = message_type,
            message = ?message,
            "Message broadcast"
        );
    }
}

/// Track message receiving for visualization
pub fn message_received(
    recipient: &crate::Identity,
    from: &crate::Identity,
    message_type: &str,
    message: impl std::fmt::Debug,
) {
    debug!(
        recipient = ?recipient,
        from = ?from,
        message_type = message_type,
        message = ?message,
        "Message received"
    );
}

/// Track block creation events
pub fn block_created(
    author: &crate::Identity,
    block_type: &str,
    block: impl std::fmt::Debug,
) {
    info!(
        author = ?author,
        block_type = block_type,
        block = ?block,
        "Block created"
    );
}

/// Track QC formation events
pub fn qc_formed(
    process_id: &crate::Identity,
    qc_type: u8,
    qc: impl std::fmt::Debug,
) {
    info!(
        process_id = ?process_id,
        qc_type = qc_type,
        qc = ?qc,
        "Quorum certificate formed"
    );
}

/// Track block finalization events
pub fn block_finalized(
    process_id: &crate::Identity,
    block_key: impl std::fmt::Debug,
) {
    info!(
        process_id = ?process_id,
        block_key = ?block_key,
        "Block finalized"
    );
}

/// Track error conditions that might be interesting for the visualizer
pub fn protocol_error(
    process_id: &crate::Identity,
    error_type: &str,
    details: impl std::fmt::Debug,
) {
    error!(
        process_id = ?process_id,
        error_type = error_type,
        details = ?details,
        "Protocol error occurred"
    );
} 