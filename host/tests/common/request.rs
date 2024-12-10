use raiko_core::interfaces::{ProofRequestOpt, ProverSpecificOpts};
use raiko_host::server::api;
use raiko_lib::consts::Network;
use raiko_lib::proof_type::ProofType;
use raiko_tasks::TaskStatus;
use serde_json::json;

// TODO redis
// TODO fix v1/v3 impl
// TODO fix db data race in tests

use crate::common::Client;

pub fn make_proof_request(
    network: Network,
    proof_type: ProofType,
    block_number: u64,
) -> ProofRequestOpt {
    ProofRequestOpt {
        block_number: Some(block_number),
        network: Some(network.to_string()),
        proof_type: Some(proof_type.to_string()),

        // Untesting parameters
        l1_inclusion_block_number: None,
        l1_network: Some("ethereum".to_string()),
        graffiti: Some(
            "8008500000000000000000000000000000000000000000000000000000000000".to_owned(),
        ),
        prover: Some("0x70997970C51812dc3A010C7d01b50e0d17dc79C8".to_owned()),
        blob_proof_type: Some("proof_of_equivalence".to_string()),
        prover_args: ProverSpecificOpts {
            native: None,
            sgx: None,
            sp1: None,
            risc0: None,
        }, // TODO handle prover args
    }
}

pub async fn complete_proof_request(api_version: &str, client: &Client, request: &ProofRequestOpt) {
    match api_version {
        "v2" => v2_complete_proof_request(client, request).await,
        _ => unreachable!(),
    }
}

pub async fn v2_complete_proof_request(client: &Client, request: &ProofRequestOpt) {
    let start_time = std::time::Instant::now();
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
    while start_time.elapsed().as_secs() < 5 * 60 {
        interval.tick().await;

        match client
            .post("/v2/proof", request)
            .await
            .expect("failed to send request")
        {
            // Proof genration is in progress
            api::v2::Status::Ok {
                data: api::v2::ProofResponse::Status { status, .. },
            } => {
                assert!(
                    matches!(status, TaskStatus::Registered | TaskStatus::WorkInProgress),
                    "status should be either Registered or WorkInProgress, got: {status:?}"
                );
            }

            // Proof genration is successfully completed
            api::v2::Status::Ok {
                data: api::v2::ProofResponse::Proof { proof },
            } => {
                println!("proof generation completed, proof: {}", json!(proof));
                return;
            }

            // Proof genration failed
            api::v2::Status::Error { message, error } => {
                panic!("proof generation failed, message: {message}, error: {error:?}");
            }
        }
    }
    panic!("proof generation failed, error: timeout");
}
