use crate::server::api::{v2, v3};
use raiko_lib::proof_type::ProofType;
use raiko_reqpool::Status;
use raiko_tasks::TaskStatus;

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
