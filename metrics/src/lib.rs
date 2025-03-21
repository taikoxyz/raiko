use lazy_static::lazy_static;
use prometheus::{
    register_counter, register_counter_vec, register_histogram, register_histogram_vec, Counter,
    CounterVec, Histogram, HistogramVec,
};
use std::time::Duration;

mod traits;

// Re-export
pub use traits::ToLabel;

lazy_static! {
    // HTTP metrics
    pub static ref HTTP_REQUEST_COUNT: Counter = register_counter!(
        "raiko_http_request_count",
        "the number of HTTP requests"
    )
    .unwrap();

    // Pool metrics
    pub static ref POOL_REQUEST_COUNT: CounterVec = register_counter_vec!(
        "raiko_pool_request_count",
        "the number of requests to the pool",
        &["request_type", "proof_type"]
    )
    .unwrap();
    pub static ref POOL_TRANSITION_DURATION_MILLIS: HistogramVec = register_histogram_vec!(
        "raiko_pool_transition_duration_millis",
        "the duration of request transiting from one status to another",
        &["request_type", "proof_type", "status_from", "status_to"]
    )
    .unwrap();

    // Actor metrics
    pub static ref ACTOR_CHANNEL_IN_COUNT: CounterVec = register_counter_vec!(
        "raiko_actor_channel_in_count",
        "the number of requests sent to the actor",
        &["request_type", "proof_type"]
    )
    .unwrap();

    pub static ref ACTOR_CHANNEL_OUT_COUNT: CounterVec = register_counter_vec!(
        "raiko_actor_channel_out_count",
        "the number of requests received from the actor",
        &["request_type", "proof_type"]
    )
    .unwrap();

    pub static ref ACTOR_CHANNEL_IN_DURATION_MILLIS: HistogramVec = register_histogram_vec!(
        "raiko_actor_channel_in_duration_millis",
        "the duration of requests sent to the actor",
        &["request_type", "proof_type"]
    )
    .unwrap();

    // Actor proving metrics
    pub static ref ACTOR_GENERATING_INPUT_DURATION_MILLIS: HistogramVec = register_histogram_vec!(
        "raiko_actor_generating_input_duration_millis",
        "the duration of generating input by the actor",
        &["request_type"]
    )
    .unwrap();

    pub static ref ACTOR_GENERATING_OUTPUT_DURATION_MILLIS: HistogramVec = register_histogram_vec!(
        "raiko_actor_generating_output_duration_millis",
        "the duration of generating output by the actor",
        &["request_type"]
    )
    .unwrap();

    pub static ref ACTOR_PROVING_DURATION_MILLIS: HistogramVec = register_histogram_vec!(
        "raiko_actor_proving_duration_millis",
        "the duration of requests being proved by the actor",
        &["request_type", "proof_type"]
    )
    .unwrap();

    // Batch request metrics
    pub static ref BATCH_REQUEST_BLOCK_COUNT: Histogram = register_histogram!(
        "raiko_batch_request_block_count",
        "the number of blocks in a batch request",
    )
    .unwrap();
}

pub fn inc_http_request_count() {
    HTTP_REQUEST_COUNT.inc();
}

pub fn inc_pool_request_count(request_type: impl ToLabel, proof_type: impl ToLabel) {
    POOL_REQUEST_COUNT
        .with_label_values(&[request_type.to_label(), proof_type.to_label()])
        .inc();
}

pub fn observe_pool_transition_duration(
    request_type: impl ToLabel,
    proof_type: impl ToLabel,
    status_from: impl ToLabel,
    status_to: impl ToLabel,
    duration: Duration,
) {
    POOL_TRANSITION_DURATION_MILLIS
        .with_label_values(&[
            request_type.to_label(),
            proof_type.to_label(),
            status_from.to_label(),
            status_to.to_label(),
        ])
        .observe(duration.as_millis() as f64);
}

pub fn inc_actor_channel_in_count(request_type: impl ToLabel, proof_type: impl ToLabel) {
    ACTOR_CHANNEL_IN_COUNT
        .with_label_values(&[request_type.to_label(), proof_type.to_label()])
        .inc();
}

pub fn inc_actor_channel_out_count(request_type: impl ToLabel, proof_type: impl ToLabel) {
    ACTOR_CHANNEL_OUT_COUNT
        .with_label_values(&[request_type.to_label(), proof_type.to_label()])
        .inc();
}

pub fn observe_actor_channel_in_duration(
    request_type: impl ToLabel,
    proof_type: impl ToLabel,
    duration: Duration,
) {
    ACTOR_CHANNEL_IN_DURATION_MILLIS
        .with_label_values(&[request_type.to_label(), proof_type.to_label()])
        .observe(duration.as_millis() as f64);
}

pub fn observe_actor_generating_input_duration(request_type: impl ToLabel, duration: Duration) {
    ACTOR_GENERATING_INPUT_DURATION_MILLIS
        .with_label_values(&[request_type.to_label()])
        .observe(duration.as_millis() as f64);
}

pub fn observe_actor_generating_output_duration(request_type: impl ToLabel, duration: Duration) {
    ACTOR_GENERATING_OUTPUT_DURATION_MILLIS
        .with_label_values(&[request_type.to_label()])
        .observe(duration.as_millis() as f64);
}

pub fn observe_actor_proving_duration(
    request_type: impl ToLabel,
    proof_type: impl ToLabel,
    duration: Duration,
) {
    ACTOR_PROVING_DURATION_MILLIS
        .with_label_values(&[request_type.to_label(), proof_type.to_label()])
        .observe(duration.as_millis() as f64);
}

pub fn observe_batch_request_block_count(block_count: u64) {
    BATCH_REQUEST_BLOCK_COUNT.observe(block_count as f64);
}
