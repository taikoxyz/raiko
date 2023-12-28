use prometheus::{register_int_gauge_vec, IntGaugeVec};
use lazy_static::lazy_static;

lazy_static! {
    pub static ref SGX_PROOF_GEN_TIME: IntGaugeVec = register_int_gauge_vec!(
            "sgx_proof_time_gauge",
            "time taken for sgx proof generation", 
            &["duration"]
        )
    .unwrap();
}

// Function to increment the metric based on method and status
pub fn observe_sgx_gen(time: i64) {
    SGX_PROOF_GEN_TIME.with_label_values(&["duration"]).set(time);
}
