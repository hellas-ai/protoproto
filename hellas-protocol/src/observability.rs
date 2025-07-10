//! # Observability Module
//!
//! This module provides comprehensive tracing and metrics instrumentation
//! for the Hellas protocol, enabling production-grade monitoring and debugging.

use metrics::{counter, describe_counter, describe_gauge, describe_histogram, gauge, histogram};
use std::time::Instant;
use tracing::{debug, info, instrument, trace, warn};

/// Initialize metrics descriptions
pub fn init_metrics() {
    // Transaction metrics
    describe_counter!(
        "hellas_transactions_total",
        "Total number of transactions processed"
    );
    describe_counter!(
        "hellas_transactions_failed",
        "Number of failed transactions"
    );
    describe_histogram!(
        "hellas_transaction_duration_seconds",
        "Transaction execution duration"
    );

    // State metrics
    describe_gauge!("hellas_accounts_total", "Total number of accounts");
    describe_gauge!("hellas_escrows_active", "Number of active job escrows");
    describe_gauge!("hellas_total_supply", "Total HELL token supply");

    // Channel metrics
    describe_counter!(
        "hellas_channel_spends_total",
        "Total channel spending operations"
    );
    describe_counter!(
        "hellas_channel_budget_exhausted",
        "Number of budget exhaustion events"
    );
    describe_histogram!(
        "hellas_channel_utilization",
        "Channel budget utilization ratio"
    );

    // Consensus metrics
    describe_gauge!("hellas_block_height", "Current block height");
    describe_histogram!("hellas_block_processing_time", "Block processing duration");
    describe_gauge!("hellas_validators_active", "Number of active validators");

    // Performance metrics
    describe_histogram!(
        "hellas_parallel_execution_speedup",
        "Parallel execution speedup factor"
    );
    describe_counter!("hellas_cache_hits", "Number of state cache hits");
    describe_counter!("hellas_cache_misses", "Number of state cache misses");
}

/// Transaction type for metrics labeling
#[derive(Debug, Clone, Copy)]
pub enum TransactionType {
    CreateAccount,
    SettleDirectly,
    PostJob,
    ClaimJob,
    CommitResult,
    FinalizeJob,
    AbortJob,
    ResetBudget,
}

impl TransactionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CreateAccount => "create_account",
            Self::SettleDirectly => "settle_directly",
            Self::PostJob => "post_job",
            Self::ClaimJob => "claim_job",
            Self::CommitResult => "commit_result",
            Self::FinalizeJob => "finalize_job",
            Self::AbortJob => "abort_job",
            Self::ResetBudget => "reset_budget",
        }
    }
}

/// Helper struct for timing operations
pub struct Timer {
    start: Instant,
    metric_name: &'static str,
}

impl Timer {
    pub fn new(metric_name: &'static str) -> Self {
        Self {
            start: Instant::now(),
            metric_name,
        }
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        let duration = self.start.elapsed().as_secs_f64();
        histogram!(self.metric_name, duration);
    }
}

/// Record transaction execution
#[instrument(skip_all, fields(tx_type = %tx_type.as_str()))]
pub fn record_transaction(tx_type: TransactionType, success: bool, duration_secs: f64) {
    counter!("hellas_transactions_total", 1, "type" => tx_type.as_str());

    if !success {
        counter!("hellas_transactions_failed", 1, "type" => tx_type.as_str());
        warn!("Transaction failed: {:?}", tx_type);
    } else {
        debug!("Transaction succeeded: {:?}", tx_type);
    }

    histogram!(
        "hellas_transaction_duration_seconds",
        duration_secs,
        "type" => tx_type.as_str()
    );
}

/// Record channel spending
#[instrument(skip_all)]
pub fn record_channel_spend(validator: &str, _account: &str, _amount: u64, success: bool) {
    counter!(
        "hellas_channel_spends_total",
        1,
        "validator" => validator.to_string(),
        "success" => success.to_string()
    );

    if !success {
        counter!(
            "hellas_channel_budget_exhausted",
            1,
            "validator" => validator.to_string()
        );
        trace!("Channel budget exhausted for validator {}", validator);
    }
}

/// Record channel utilization
pub fn record_channel_utilization(validator: &str, utilization_ratio: f64) {
    histogram!(
        "hellas_channel_utilization",
        utilization_ratio,
        "validator" => validator.to_string()
    );
}

/// Update state metrics
pub fn update_state_metrics(accounts: usize, active_escrows: usize, total_supply: u64) {
    gauge!("hellas_accounts_total", accounts as f64);
    gauge!("hellas_escrows_active", active_escrows as f64);
    gauge!("hellas_total_supply", total_supply as f64);
}

/// Update consensus metrics
pub fn update_consensus_metrics(block_height: u64, validators: usize) {
    gauge!("hellas_block_height", block_height as f64);
    gauge!("hellas_validators_active", validators as f64);
}

/// Record parallel execution performance
pub fn record_parallel_speedup(sequential_time: f64, parallel_time: f64) {
    let speedup = sequential_time / parallel_time;
    histogram!("hellas_parallel_execution_speedup", speedup);
    info!("Parallel execution speedup: {:.2}x", speedup);
}

/// Record cache performance
pub fn record_cache_access(hit: bool) {
    if hit {
        counter!("hellas_cache_hits", 1);
    } else {
        counter!("hellas_cache_misses", 1);
    }
}

/// Record a generic operation
pub fn record_operation(operation: &str, success: bool, duration: f64) {
    let labels = vec![
        ("operation", operation.to_string()),
        ("success", success.to_string()),
    ];
    counter!("hellas_operations_total", 1, &labels);
    histogram!("hellas_operation_duration_seconds", duration, &labels);
}

/// Start Prometheus metrics exporter
pub fn start_metrics_server(port: u16) -> Result<(), Box<dyn std::error::Error>> {
    let builder = metrics_exporter_prometheus::PrometheusBuilder::new();
    let _handle = builder
        .with_http_listener(([0, 0, 0, 0], port))
        .install_recorder()?;

    info!("Metrics server started on port {}", port);

    // Note: In production, the handle should be kept alive.
    // The metrics server will run in the background.

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_type_strings() {
        assert_eq!(TransactionType::CreateAccount.as_str(), "create_account");
        assert_eq!(TransactionType::SettleDirectly.as_str(), "settle_directly");
    }

    #[test]
    fn test_timer() {
        let _timer = Timer::new("test_metric");
        std::thread::sleep(std::time::Duration::from_millis(10));
        // Timer will record metric on drop
    }
}
