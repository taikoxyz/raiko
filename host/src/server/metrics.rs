// There is no attribute macro like #[tracing::instrument(target = "billing")] or similar to set a tracing target for an entire file or module in Rust.
// The correct way is to use the `target: "billing"` argument in each tracing macro invocation, as in:
// tracing::info!(target: "billing", "message");
// This is already reflected in the code below and should be used for all tracing in this file.

use crate::metrics;
use raiko_core::interfaces::{BatchProofRequest, ShastaProofRequest};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tracing::{debug, info};

#[derive(Debug, Clone)]
pub struct RequestMetrics {
    pub from: String,
    pub request_desc: String,
    pub start_time: Instant,
    pub last_request_time: Instant,
    pub end_time: Option<Instant>,
}

pub struct MetricsCollector {
    requests: Mutex<HashMap<String, RequestMetrics>>,
}

impl MetricsCollector {
    pub fn new() -> Self {
        Self {
            requests: Mutex::new(HashMap::new()),
        }
    }

    /// Record the start of a request
    pub fn record_request_in(&self, request_id: &str, caller: &str) {
        if let Ok(mut requests) = self.requests.lock() {
            if !requests.contains_key(request_id) {
                let start_time = Instant::now();
                let metrics = RequestMetrics {
                    from: caller.to_string(),
                    request_desc: request_id.to_string(),
                    start_time,
                    last_request_time: start_time,
                    end_time: None,
                };
                requests.insert(request_id.to_string(), metrics);
            }
        }

        info!(
            target: "billing",
            "BATCH_REQUEST_START - ID: {}",
            request_id
        );
    }

    /// Record the end of a request
    pub fn record_request_out(&self, request_id: &str, has_proof: bool) -> Option<Duration> {
        if let Ok(mut requests) = self.requests.lock() {
            if let Some(metrics) = requests.get_mut(request_id) {
                let end_time = Instant::now();
                let elapsed_time_since_last_req =
                    end_time.duration_since(metrics.last_request_time);
                metrics.last_request_time = end_time;

                if has_proof {
                    let duration = end_time.duration_since(metrics.start_time);
                    metrics.end_time = Some(end_time);

                    requests.remove(request_id);
                    info!(
                        target: "billing",
                        "BATCH_REQUEST_END - ID: {}, DURATION: {:?}, HAS_PROOF: {}",
                        request_id, duration, has_proof
                    );
                } else {
                    debug!(
                        target: "billing",
                        "BATCH_REQUEST_CONT - ID: {}, DURATION: {:?}, HAS_PROOF: {}",
                        request_id, elapsed_time_since_last_req, has_proof
                    );
                };
                return Some(elapsed_time_since_last_req);
            }
        }

        None
    }

    /// Get request metrics by request_id
    pub fn get_request_metrics(&self, request_id: &str) -> Option<RequestMetrics> {
        if let Ok(requests) = self.requests.lock() {
            requests.get(request_id).cloned()
        } else {
            None
        }
    }

    /// Clean up old request records (optional)
    pub fn cleanup_old_requests(&self, max_age: Duration) {
        if let Ok(mut requests) = self.requests.lock() {
            let now = Instant::now();
            requests.retain(|_, metrics| {
                if let Some(end_time) = metrics.end_time {
                    now.duration_since(end_time) < max_age
                } else {
                    true // Keep unfinished requests
                }
            });
        }
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

// Global metrics collector instance
lazy_static::lazy_static! {
    pub static ref METRICS_COLLECTOR: MetricsCollector = MetricsCollector::new();
}

/// Generate a unique request ID
pub fn generate_request_id(api_key: &str, batch_request: &BatchProofRequest) -> String {
    let request = format!(
        "{}_{}_batch_{}+{}",
        if batch_request.aggregate {
            "aggregate"
        } else {
            "single"
        },
        batch_request.proof_type,
        batch_request.batches.first().unwrap().batch_id,
        batch_request.batches.len(),
    );

    format!("{}_request_{}", api_key, request)
}

/// Convenience function: record request start
pub fn record_batch_request_in(api_key_owner: &str, batch_request: &BatchProofRequest) {
    let request_id = generate_request_id(api_key_owner, batch_request);
    METRICS_COLLECTOR.record_request_in(&request_id, api_key_owner);
}

/// Convenience function: record request end
pub fn record_batch_request_out(
    api_key_owner: &str,
    batch_request: &BatchProofRequest,
    has_proof: bool,
) {
    let request_id = generate_request_id(api_key_owner, batch_request);
    // record the increment of the request duration for prometheus counter metrics
    if let Some(duration_inc) = METRICS_COLLECTOR.record_request_out(&request_id, has_proof) {
        let batch_desc = format!(
            "{}+{}",
            batch_request.batches.first().unwrap().batch_id,
            batch_request.batches.len()
        );

        // record accumulated preconfimer proof generation increment
        metrics::accumulate_caller_proof_time_cost(api_key_owner, duration_inc);
        // record current proof generation increment, see if this task can not be done in time
        metrics::accumulate_single_proof_gen_time(
            batch_request.aggregate,
            &batch_request.proof_type,
            &batch_desc,
            duration_inc,
        );
    }
}

/// Generate a unique request ID
pub fn generate_shasta_request_id(api_key: &str, batch_request: &ShastaProofRequest) -> String {
    let request = format!(
        "{}_{}_proposal_{}+{}",
        if batch_request.aggregate {
            "aggregate"
        } else {
            "single"
        },
        batch_request.proof_type,
        batch_request.proposals.first().unwrap().proposal_id,
        batch_request.proposals.len(),
    );

    format!("{}_request_{}", api_key, request)
}

/// Convenience function: record request start
pub fn record_shasta_request_in(api_key_owner: &str, shasta_request: &ShastaProofRequest) {
    let request_id = generate_shasta_request_id(api_key_owner, shasta_request);
    METRICS_COLLECTOR.record_request_in(&request_id, api_key_owner);
}

/// Convenience function: record request end
pub fn record_shasta_request_out(
    api_key_owner: &str,
    shasta_request: &ShastaProofRequest,
    has_proof: bool,
) {
    let request_id = generate_shasta_request_id(api_key_owner, shasta_request);
    // record the increment of the request duration for prometheus counter metrics
    if let Some(duration_inc) = METRICS_COLLECTOR.record_request_out(&request_id, has_proof) {
        let batch_desc = format!(
            "{}+{}",
            shasta_request.proposals.first().unwrap().proposal_id,
            shasta_request.proposals.len(),
        );

        // record accumulated preconfimer proof generation increment
        metrics::accumulate_caller_proof_time_cost(api_key_owner, duration_inc);
        // record current proof generation increment, see if this task can not be done in time
        metrics::accumulate_single_proof_gen_time(
            shasta_request.aggregate,
            &shasta_request.proof_type,
            &batch_desc,
            duration_inc,
        );
    }
}
