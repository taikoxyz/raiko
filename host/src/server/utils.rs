use crate::{
    interfaces::HostResult,
    server::api::{v2, v3},
};
use raiko_core::{interfaces::RaikoError, provider::get_task_data};
use raiko_lib::{primitives::keccak::keccak, proof_type::ProofType};
use raiko_reqactor::Actor;
use raiko_reqpool::Status;
use raiko_tasks::TaskStatus;
use serde_json::Value;

pub fn to_v2_status(
    proof_type: ProofType,
    batch_id: Option<u64>,
    result: Result<Status, String>,
) -> v2::Status {
    match result {
        Ok(status) => v2::Status::Ok {
            proof_type,
            batch_id,
            data: {
                match status {
                    Status::Registered => v2::ProofResponse::Status {
                        status: TaskStatus::Registered,
                    },
                    Status::WorkInProgress => v2::ProofResponse::Status {
                        status: TaskStatus::WorkInProgress,
                    },
                    Status::Cancelled => v2::ProofResponse::Status {
                        status: TaskStatus::Cancelled,
                    },
                    Status::Failed { error } => v2::ProofResponse::Status {
                        status: TaskStatus::AnyhowError(error),
                    },
                    Status::Success { proof } => v2::ProofResponse::Proof { proof },
                }
            },
        },
        Err(e) => v2::Status::Error {
            error: "task_failed".to_string(),
            message: e,
        },
    }
}

pub fn to_v2_cancel_status(result: Result<Status, String>) -> v2::CancelStatus {
    match result {
        Ok(status) => match status {
            Status::Success { .. } | Status::Cancelled | Status::Failed { .. } => {
                v2::CancelStatus::Ok
            }
            _ => v2::CancelStatus::Error {
                error: "cancel_failed".to_string(),
                message: format!("cancallation response unexpected status {}", status),
            },
        },
        Err(e) => v2::CancelStatus::Error {
            error: "cancel_failed".to_string(),
            message: e,
        },
    }
}

// TODO: remove the staled interface
pub fn to_v3_status(
    proof_type: ProofType,
    batch_id: Option<u64>,
    result: Result<Status, String>,
) -> v3::Status {
    to_v2_status(proof_type, batch_id, result)
}

pub fn to_v3_cancel_status(result: Result<Status, String>) -> v3::CancelStatus {
    to_v2_cancel_status(result)
}

// A zk_any request looks like: { "proof_type": "zk_any", "zk_any": { "aggregation": <bool> } }
pub fn is_zk_any_request(proof_request_opt: &Value) -> bool {
    let proof_type = proof_request_opt["proof_type"].as_str();
    return proof_type == Some("zk_any");
}

pub async fn draw_for_zk_any_request(
    actor: &Actor,
    proof_request_opt: &Value,
) -> HostResult<Option<ProofType>> {
    if actor.is_ballot_disabled().await {
        return Ok(None);
    }

    let network = proof_request_opt["network"]
        .as_str()
        .ok_or(RaikoError::InvalidRequestConfig(
            "Missing network".to_string(),
        ))?;
    let block_number =
        proof_request_opt["block_number"]
            .as_u64()
            .ok_or(RaikoError::InvalidRequestConfig(
                "Missing block number".to_string(),
            ))?;
    let (_, blockhash) = get_task_data(&network, block_number, actor.chain_specs()).await?;
    Ok(actor.draw(&blockhash).await)
}

pub async fn draw_for_zk_any_batch_request(
    actor: &Actor,
    batch_proof_request_opt: &Value,
) -> HostResult<Option<ProofType>> {
    if actor.is_ballot_disabled().await {
        return Ok(None);
    }

    let l1_network =
        batch_proof_request_opt["l1_network"]
            .as_str()
            .ok_or(RaikoError::InvalidRequestConfig(
                "Missing network".to_string(),
            ))?;
    let batches =
        batch_proof_request_opt["batches"]
            .as_array()
            .ok_or(RaikoError::InvalidRequestConfig(
                "Missing batches".to_string(),
            ))?;
    let first_batch = batches.first().ok_or(RaikoError::InvalidRequestConfig(
        "batches is empty".to_string(),
    ))?;
    let l1_inclusion_block_number = first_batch["l1_inclusion_block_number"].as_u64().ok_or(
        RaikoError::InvalidRequestConfig("Missing l1_inclusion_block_number".to_string()),
    )?;
    let (_, blockhash) =
        get_task_data(&l1_network, l1_inclusion_block_number, actor.chain_specs()).await?;
    Ok(actor.draw(&blockhash).await)
}

pub async fn draw_shasta_zk_request(
    actor: &Actor,
    proposal_id: u64,
    l1_inclusion_block_number: u64,
) -> HostResult<Option<ProofType>> {
    if actor.is_ballot_disabled().await {
        return Ok(None);
    }

    let seed_hash =
        keccak(format!("proposal:{}/{}", proposal_id, l1_inclusion_block_number,).as_bytes())
            .into();
    Ok(actor.draw(&seed_hash).await)
}
