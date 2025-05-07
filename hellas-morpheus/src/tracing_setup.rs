use tracing::{debug, error, info};

/// Register a new Morpheus process with tracing
pub fn register_process(id: &crate::Identity, n: u32, f: u32) {
    info!(target: "register_process", process_id = ?id, total_processes = n, max_faulty = f);
}

/// Track protocol transitions such as view changes
pub fn protocol_transition(
    process_id: &crate::Identity,
    transition_type: &str,
    from: impl std::fmt::Debug,
    to: impl std::fmt::Debug,
    reason: Option<&str>,
) {
    if let Some(reason) = reason {
        info!(
            target: "protocol_transition",
            process_id = ?process_id,
            transition = transition_type,
            from = ?from,
            to = ?to,
            reason = reason,
        );
    } else {
        info!(
            target: "protocol_transition",
            process_id = ?process_id,
            transition = transition_type,
            from = ?from,
            to = ?to,
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
            target: "message_sent",
            from = ?from,
            to = ?to,
            message_type = message_type,
            message = ?message,
        );
    } else {
        debug!(
            target: "message_sent",
            from = ?from,
            to = "broadcast",
            message_type = message_type,
            message = ?message,
        );
    }
}

/// Track block creation events
pub fn block_created(author: &crate::Identity, block_type: &str, block: impl std::fmt::Debug) {
    info!(
        target: "block_created",
        author = ?author,
        block_type = block_type,
        block = ?block,
    );
}

/// Track QC formation events
pub fn qc_formed(process_id: &crate::Identity, qc_type: u8, qc: impl std::fmt::Debug) {
    info!(
        target: "qc_formed",
        process_id = ?process_id,
        qc_type = qc_type,
        qc = ?qc,
    );
}

/// Track block finalization events
pub fn block_finalized(process_id: &crate::Identity, block_key: impl std::fmt::Debug) {
    info!(
        target: "block_finalized",
        process_id = ?process_id,
        block_key = ?block_key,
    );
}

/// Track error conditions that might be interesting for the visualizer
pub fn protocol_error(
    process_id: &crate::Identity,
    error_type: &str,
    details: impl std::fmt::Debug,
) {
    error!(
        target: "protocol_error",
        process_id = ?process_id,
        error_type = error_type,
        details = ?details,
    );
}
