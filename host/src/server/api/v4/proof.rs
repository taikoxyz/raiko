use axum::{extract::State, Json};
use raiko_core::interfaces::ProofRequest;
use serde_json::{json, Value};

// Import the reusing interfaces and types
use super::HostResult;

// Import the forwarding handler
use crate::server::api::v2::proof::proof_handler as internal_proof_handler;

#[axum::debug_handler]
/// Accept a proof request, return a list of tasks details, including the request key and the status of the proving tasks.
pub async fn proof_handler(
    State(actor): State<raiko_reqactor::Actor>,
    Json(req): Json<Value>,
) -> HostResult<super::types::RequestAndStatusVec> {
    let mut config = actor.default_request_config().clone();
    config.merge(&req)?;

    let block_number = config
        .block_number
        .ok_or_else(|| anyhow::anyhow!("block number is required"))?;
    let (always_proof_type, ballot_proof_type) = actor.draw(block_number);

    let always_request = {
        let mut config = config.clone();
        config.proof_type = Some(always_proof_type.to_string());
        ProofRequest::try_from(config)?
    };
    let ballot_request = {
        ballot_proof_type.map(|proof_type| {
            let mut config = config.clone();
            config.proof_type = Some(proof_type.to_string());
            ProofRequest::try_from(config).expect("checked above")
        })
    };

    if let Some(ballot_request) = ballot_request {
        let actor_ = actor.clone();
        Ok(vec![
            super::types::RequestAndStatus {
                request: always_request.clone(),
                status: internal_proof_handler(State(actor_), Json(json!(always_request))).await?,
            },
            super::types::RequestAndStatus {
                request: ballot_request.clone(),
                status: internal_proof_handler(State(actor), Json(json!(ballot_request))).await?,
            },
        ]
        .into())
    } else {
        Ok(vec![super::types::RequestAndStatus {
            request: always_request.clone(),
            status: internal_proof_handler(State(actor), Json(json!(always_request))).await?,
        }]
        .into())
    }
}
