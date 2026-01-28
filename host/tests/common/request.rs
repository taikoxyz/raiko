use raiko_core::interfaces::{AggregationOnlyRequest, ProverSpecificOpts};
use raiko_host::server::api;
use raiko_lib::consts::Network;
use raiko_lib::proof_type::ProofType;
use raiko_lib::prover::Proof;
use raiko_tasks::{AggregationTaskDescriptor, TaskDescriptor, TaskReport, TaskStatus};
use serde_json::{json, Value};

use crate::common::Client;


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
            risc0: Some(json!({
                "bonsai": false, // run locally
                "snark": false,
                "profile": false,
                "execution_po2" : 21, // DEFAULT_SEGMENT_LIMIT_PO2 = 20
            })),
            sgx: None,
            sp1: None,
            sgxgeth: None,
        },
    }
}




pub async fn v3_complete_aggregate_proof_request(
    client: &Client,
    request: &AggregationOnlyRequest,
) -> Proof {
    let start_time = std::time::Instant::now();
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(2000));
    while start_time.elapsed().as_secs() < 60 * 60 {
        interval.tick().await;

        let task_status = get_status_of_aggregation_proof_request(client, request).await;
        println!("[v3_complete_aggregate_proof_request] task_status: {task_status:?}");

        let task_status_code: i32 = task_status.clone().into();
        assert!(
            task_status_code >= -4000,
            "aggregation proof generation failed, task_status: {task_status:?}, request: {request:?}",
        );

        if task_status != TaskStatus::Success {
            continue;
        }

        match client
            .post("/v3/proof/aggregate", request)
            .await
            .expect("failed to send request")
        {
            // Proof generation is in progress
            api::v3::Status::Ok {
                data: api::v3::ProofResponse::Status { status, .. },
                ..
            } => {
                assert!(
                    matches!(status, TaskStatus::Registered | TaskStatus::WorkInProgress),
                    "status should be either Registered or WorkInProgress, got: {status:?}"
                );
            }

            // Proof generation is successfully completed
            api::v3::Status::Ok {
                data: api::v3::ProofResponse::Proof { proof },
                ..
            } => {
                println!(
                    "aggregation proof generation completed, proof: {}",
                    json!(proof)
                );
                return proof;
            }

            // Proof generation failed
            api::v3::Status::Error { message, error } => {
                panic!("proof generation failed, message: {message}, error: {error:?}");
            }
        }
    }

    panic!("aggregation proof generation failed, error: timeout");
}

pub fn make_batch_proof_request(
    network: &Network,
    proof_type: &ProofType,
    batch_id: u64,
    l1_inclusion_block_number: u64,
) -> Value {
    json!({
        "network": network.to_string(),
        "l1_network": "ethereum",
        "batches": [{
            "batch_id": batch_id,
            "l1_inclusion_block_number": l1_inclusion_block_number
        }],
        "prover": "0x70997970C51812dc3A010C7d01b50e0d17dc79C8",
        "aggregate": false,
        "proof_type": proof_type.to_string(),
        "blob_proof_type": "proof_of_equivalence"
    })
}

pub async fn complete_batch_proof_request(client: &Client, request: &Value) -> Proof {
    let start_time = std::time::Instant::now();
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(2000));
    
    while start_time.elapsed().as_secs() < 60 * 60 {
        interval.tick().await;

        let batch_id = request["batches"][0]["batch_id"].as_u64().unwrap();
        let task_status = get_status_of_batch_proof_request(client, batch_id).await;
        println!("[complete_batch_proof_request] task_status: {task_status:?}");

        let task_status_code: i32 = task_status.clone().into();
        assert!(
            task_status_code >= -4000,
            "batch proof generation failed, task_status: {task_status:?}, request: {request:?}",
        );

        match client
            .post("/v3/proof/batch", request)
            .await
            .expect("failed to send request")
        {
            // Proof generation is in progress
            api::v3::Status::Ok {
                data: api::v3::ProofResponse::Status { status, .. },
                ..
            } => {
                if matches!(status, TaskStatus::Registered | TaskStatus::WorkInProgress) {
                    continue;
                }
            }

            // Proof generation is successfully completed
            api::v3::Status::Ok {
                data: api::v3::ProofResponse::Proof { proof },
                ..
            } => {
                println!("batch proof generation completed, proof: {}", json!(proof));
                return proof;
            }

            // Proof generation failed
            api::v3::Status::Error { message, error } => {
                panic!("batch proof generation failed, message: {message}, error: {error:?}");
            }
        }
    }
    panic!("batch proof generation failed, error: timeout");
}

/// Assert that the report is in the expected format for v3.
pub async fn v3_assert_report(client: &Client) -> Vec<TaskReport> {
    let response = client
        .get(&format!("/v3/proof/report"))
        .await
        .expect("failed to send request");
    response.json().await.expect("failed to decode report body")
}


pub async fn get_status_of_aggregation_proof_request(
    client: &Client,
    request: &AggregationOnlyRequest,
) -> TaskStatus {
    let descriptor = AggregationTaskDescriptor {
        aggregation_ids: request.aggregation_ids.clone(),
        proof_type: request.proof_type.clone().map(|p| p.to_string()),
    };
    let expected_task_descriptor: TaskDescriptor = TaskDescriptor::Aggregation(descriptor);
    let report = v3_assert_report(client).await;
    for (task_descriptor, task_status) in &report {
        if task_descriptor == &expected_task_descriptor {
            return task_status.clone();
        }
    }
    panic!(
        "aggregation proof request not found in report: report: {report:?}, request: {request:?}"
    );
}

pub async fn get_status_of_batch_proof_request(client: &Client, batch_id: u64) -> TaskStatus {
    let report = v3_assert_report(client).await;
    for (task_descriptor, task_status) in report.iter() {
        if let TaskDescriptor::BatchProof(batch_task_descriptor) = task_descriptor {
            if batch_task_descriptor.batch_id == batch_id {
                return task_status.clone();
            }
        }
        // If the task is a batch guest input task, check if the batch id matches the request
        if let TaskDescriptor::BatchGuestInput(batch_guest_input_desc) = task_descriptor {
            if batch_guest_input_desc.batch_id == batch_id {
                // return working in progress status
                return TaskStatus::WorkInProgress;
            }
        }
    }
    TaskStatus::Registered
}
