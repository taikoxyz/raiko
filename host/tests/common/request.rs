use raiko_core::interfaces::{AggregationOnlyRequest, ProofRequestOpt, ProverSpecificOpts};
use raiko_host::server::api;
use raiko_lib::consts::Network;
use raiko_lib::proof_type::ProofType;
use raiko_lib::prover::Proof;
use raiko_tasks::{TaskReport, TaskStatus};
use serde_json::json;

use crate::common::Client;

pub fn make_proof_request(
    network: &Network,
    proof_type: &ProofType,
    block_number: u64,
) -> ProofRequestOpt {
    let json_guest_input = format!(
        "make_prove_request_{}_{}_{}_{}.json",
        network,
        proof_type,
        block_number,
        std::time::Instant::now().elapsed().as_secs()
    );
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
            native: Some(json!({
                "json_guest_input": json_guest_input,
            })),
            sgx: None,
            sp1: None,
            risc0: None,
        },
    }
}

pub async fn make_aggregate_proof_request(
    network: &Network,
    proof_type: &ProofType,
    block_numbers: Vec<u64>,
    proofs: Vec<Proof>,
) -> AggregationOnlyRequest {
    let json_guest_input = format!(
        "make_aggregate_proof_request_{}_{}_{}_{}.json",
        network,
        proof_type,
        block_numbers
            .iter()
            .map(|n| n.to_string())
            .collect::<Vec<String>>()
            .join(","),
        std::time::Instant::now().elapsed().as_secs()
    );
    AggregationOnlyRequest {
        aggregation_ids: block_numbers,
        proofs,
        proof_type: Some(proof_type.to_string()),
        prover_args: ProverSpecificOpts {
            native: Some(json!({
                "json_guest_input": json_guest_input,
            })),
            sgx: None,
            sp1: None,
            risc0: None,
        },
    }
}

pub async fn complete_proof_request(
    api_version: &str,
    client: &Client,
    request: &ProofRequestOpt,
) -> Proof {
    match api_version {
        "v2" => v2_complete_proof_request(client, request).await,
        _ => unreachable!(),
    }
}

pub async fn v2_complete_proof_request(client: &Client, request: &ProofRequestOpt) -> Proof {
    let start_time = std::time::Instant::now();
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
    while start_time.elapsed().as_secs() < 60 * 60 {
        interval.tick().await;

        match client
            .post("/v2/proof", request)
            .await
            .expect("failed to send request")
        {
            // Proof generation is in progress
            api::v2::Status::Ok {
                data: api::v2::ProofResponse::Status { status, .. },
            } => {
                assert!(
                    matches!(status, TaskStatus::Registered | TaskStatus::WorkInProgress),
                    "status should be either Registered or WorkInProgress, got: {status:?}"
                );
            }

            // Proof generation is successfully completed
            api::v2::Status::Ok {
                data: api::v2::ProofResponse::Proof { proof },
            } => {
                println!("proof generation completed, proof: {}", json!(proof));
                return proof;
            }

            // Proof generation failed
            api::v2::Status::Error { message, error } => {
                panic!("proof generation failed, message: {message}, error: {error:?}");
            }
        }
    }
    panic!("proof generation failed, error: timeout");
}

pub async fn complete_aggregate_proof_request(
    api_version: &str,
    client: &Client,
    request: &AggregationOnlyRequest,
) -> Proof {
    match api_version {
        "v3" => v3_complete_aggregate_proof_request(client, request).await,
        _ => unreachable!(),
    }
}

pub async fn v3_complete_aggregate_proof_request(
    client: &Client,
    request: &AggregationOnlyRequest,
) -> Proof {
    let start_time = std::time::Instant::now();
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
    while start_time.elapsed().as_secs() < 60 * 60 {
        interval.tick().await;

        match client
            .post("/v3/proof/aggregate", request)
            .await
            .expect("failed to send request")
        {
            // Proof generation is in progress
            api::v2::Status::Ok {
                data: api::v2::ProofResponse::Status { status, .. },
            } => {
                assert!(
                    matches!(status, TaskStatus::Registered | TaskStatus::WorkInProgress),
                    "status should be either Registered or WorkInProgress, got: {status:?}"
                );
            }

            // Proof generation is successfully completed
            api::v2::Status::Ok {
                data: api::v2::ProofResponse::Proof { proof },
            } => {
                println!("aggregation proof generation completed, proof: {}", json!(proof));
                return proof;
            }

            // Proof generation failed
            api::v2::Status::Error { message, error } => {
                panic!("proof generation failed, message: {message}, error: {error:?}");
            }
        }
    }

    panic!("aggregation proof generation failed, error: timeout");
}

/// Assert that the report is in the expected format.
pub async fn v2_assert_report(client: &Client) -> Vec<TaskReport> {
    let response = client
        .get(&format!("/v2/proof/report"))
        .await
        .expect("failed to send request");
    response.json().await.expect("failed to decode report body")
}
