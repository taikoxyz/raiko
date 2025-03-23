use crate::{
    interfaces::HostResult,
    server::api::{v2, v3},
};
use raiko_core::{interfaces::RaikoError, provider::get_task_data};
use raiko_lib::proof_type::ProofType;
use raiko_reqactor::Actor;
use raiko_reqpool::Status;
use raiko_tasks::TaskStatus;
use serde_json::Value;

pub fn to_v2_status(proof_type: ProofType, result: Result<Status, String>) -> v2::Status {
    match result {
        Ok(status) => v2::Status::Ok {
            proof_type,
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
pub fn to_v3_status(proof_type: ProofType, result: Result<Status, String>) -> v3::Status {
    to_v2_status(proof_type, result)
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
    Ok(actor.draw(&blockhash))
}

pub fn fulfill_sp1_params(req: &mut Value) {
    let zk_any_opts = req["zk_any"].as_object().clone();
    let sp1_recursion = match zk_any_opts {
        None => serde_json::Value::String("plonk".to_string()),
        Some(zk_any) => {
            let aggregation = zk_any["aggregation"].as_bool().unwrap_or(false);
            if aggregation {
                serde_json::Value::String("compressed".to_string())
            } else {
                serde_json::Value::String("plonk".to_string())
            }
        }
    };

    let sp1_opts = req["sp1"].as_object_mut();
    match sp1_opts {
        None => {
            let mut sp1_opts = serde_json::Map::new();
            sp1_opts.insert("recursion".to_string(), sp1_recursion);
            req["sp1"] = serde_json::Value::Object(sp1_opts);
        }
        Some(sp1_opts) => {
            sp1_opts.insert("recursion".to_string(), sp1_recursion);
        }
    }
}
