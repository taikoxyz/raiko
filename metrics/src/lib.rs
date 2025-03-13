use lazy_static::lazy_static;
use prometheus::{
    register_counter_vec, register_gauge, register_histogram_vec, CounterVec, Gauge, HistogramVec,
};
use raiko_lib::proof_type::ProofType;
use std::time::Duration;

lazy_static! {
    pub static ref IN_ACTIONS: CounterVec = register_counter_vec!(
        "raiko_in_actions_count",
        "raiko_in_actions_count",
        &["action_type", "proof_type", "request_type"]
    )
    .unwrap();
    pub static ref ACTION_WAIT_SEMAPHORE_DURATION_MILLIS: HistogramVec = register_histogram_vec!(
        "raiko_action_wait_semaphore_duration_millis",
        "raiko_action_wait_semaphore_duration_millis",
        &["request_type"]
    )
    .unwrap();
    pub static ref ACTION_PROVE_DURATION_MILLIS: HistogramVec = register_histogram_vec!(
        "raiko_action_prove_duration_millis",
        "raiko_action_prove_duration_millis",
        &["proof_type", "request_type", "status"]
    )
    .unwrap();
}

pub fn increment_in_actions(
    action_type: impl ToLabel,
    proof_type: impl ToLabel,
    request_type: impl ToLabel,
) {
    IN_ACTIONS
        .with_label_values(&[
            action_type.to_label(),
            proof_type.to_label(),
            request_type.to_label(),
        ])
        .inc();
}

pub fn observe_action_wait_semaphore_duration(request_type: impl ToLabel, duration: Duration) {
    ACTION_WAIT_SEMAPHORE_DURATION_MILLIS
        .with_label_values(&[request_type.to_label()])
        .observe((duration.as_secs_f64() * 1_000.0).round() / 1_000.0);
}

pub fn observe_action_prove_duration(
    proof_type: impl ToLabel,
    request_type: impl ToLabel,
    status: impl ToLabel,
    duration: Duration,
) {
    ACTION_PROVE_DURATION_MILLIS
        .with_label_values(&[
            proof_type.to_label(),
            request_type.to_label(),
            status.to_label(),
        ])
        .observe((duration.as_secs_f64() * 1_000.0).round() / 1_000.0);
}

pub trait ToLabel {
    fn to_label(&self) -> &'static str;
}

impl ToLabel for &ProofType {
    fn to_label(&self) -> &'static str {
        match self {
            ProofType::Native => "native",
            ProofType::Sp1 => "sp1",
            ProofType::Sgx => "sgx",
            ProofType::Risc0 => "risc0",
        }
    }
}
