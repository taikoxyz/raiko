use crate::server::{
    api::v3::{
        proof::shasta_handler::handle_shasta_batch_request, ProofResponse, Status as V3Status,
    },
    auth::AuthenticatedApiKey,
};
use axum::{extract::State, routing::post, Extension, Json, Router};
use raiko_reqactor::Actor;
use raiko_tasks::TaskStatus;
use serde::Deserialize;
use serde_json::{json, Value};

const MAX_V4_L2_BLOCK_RANGE_LEN: u64 = 100_000;
const MAX_V4_TOTAL_L2_BLOCKS_PER_REQUEST: u64 = MAX_V4_L2_BLOCK_RANGE_LEN;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct V4ProofRequest {
    proof_type: String,
    proposals: Vec<V4ProposalRequest>,
    #[serde(default)]
    aggregate: bool,
    #[serde(default)]
    prover: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct V4ProposalRequest {
    proposal_id: u64,
    #[serde(default)]
    checkpoint: Option<Value>,
    l1_inclusion_block_number: u64,
    l2_block_number_start: u64,
    l2_block_number_end: u64,
    last_anchor_block_number: u64,
}

struct V4Conversion {
    shasta_value: Value,
    proposal_id_start: u64,
    proposal_id_end: u64,
}

async fn proposal_proof_handler(
    State(actor): State<Actor>,
    Extension(authenticated_key): Extension<AuthenticatedApiKey>,
    Json(request): Json<Value>,
) -> Json<Value> {
    let conversion = match v4_to_shasta_conversion(request) {
        Ok(conversion) => conversion,
        Err(message) => return Json(v4_error("invalid_request", message)),
    };

    let response = match handle_shasta_batch_request(
        actor,
        authenticated_key,
        conversion.shasta_value.clone(),
    )
    .await
    {
        Ok(status) => v3_status_to_v4_value(&conversion, status),
        Err(error) => v4_error("proof_request_failed", error.to_string()),
    };

    Json(response)
}

#[cfg(test)]
fn v4_to_shasta_value(request: Value) -> Result<Value, String> {
    v4_to_shasta_conversion(request).map(|conversion| conversion.shasta_value)
}

fn v4_to_shasta_conversion(request: Value) -> Result<V4Conversion, String> {
    let request: V4ProofRequest =
        serde_json::from_value(request).map_err(|err| format!("invalid v4 request: {err}"))?;
    let first_proposal = request
        .proposals
        .first()
        .ok_or_else(|| "proposals must not be empty".to_string())?;
    if !request.aggregate && request.proposals.len() != 1 {
        return Err("aggregate=false accepts exactly one proposal".to_string());
    }

    let proposal_id_start = first_proposal.proposal_id;
    let proposal_id_end = request
        .proposals
        .last()
        .map(|proposal| proposal.proposal_id)
        .unwrap_or(proposal_id_start);
    let proof_type = request.proof_type;
    let mut total_l2_blocks = 0_u64;
    let mut proposals = Vec::with_capacity(request.proposals.len());
    for proposal in request.proposals {
        let (proposal, range_len) = v4_proposal_to_shasta_value(proposal)?;
        total_l2_blocks = total_l2_blocks
            .checked_add(range_len)
            .ok_or_else(|| "total proposals[].l2 block range length overflows u64".to_string())?;
        if total_l2_blocks > MAX_V4_TOTAL_L2_BLOCKS_PER_REQUEST {
            return Err(format!(
                "total proposals[].l2 block range length {total_l2_blocks} exceeds maximum \
                 {MAX_V4_TOTAL_L2_BLOCKS_PER_REQUEST}"
            ));
        }
        proposals.push(proposal);
    }

    let mut shasta_value = json!({
        "proof_type": proof_type,
        "aggregate": request.aggregate,
        "proposals": proposals,
    });
    if let Some(prover) = request.prover {
        shasta_value["prover"] = json!(prover);
    }

    Ok(V4Conversion {
        shasta_value,
        proposal_id_start,
        proposal_id_end,
    })
}

fn v4_proposal_to_shasta_value(proposal: V4ProposalRequest) -> Result<(Value, u64), String> {
    let range_len = proposal
        .l2_block_number_end
        .checked_sub(proposal.l2_block_number_start)
        .and_then(|len| len.checked_add(1))
        .ok_or_else(|| {
            "proposals[].l2_block_number_end must be greater than or equal to \
             proposals[].l2_block_number_start"
                .to_string()
        })?;
    if range_len > MAX_V4_L2_BLOCK_RANGE_LEN {
        return Err(format!(
            "proposals[].l2_block_number_start..=proposals[].l2_block_number_end range length \
             {range_len} exceeds maximum {MAX_V4_L2_BLOCK_RANGE_LEN}"
        ));
    }

    let l2_block_numbers =
        (proposal.l2_block_number_start..=proposal.l2_block_number_end).collect::<Vec<_>>();
    let mut shasta_proposal = json!({
        "proposal_id": proposal.proposal_id,
        "l1_inclusion_block_number": proposal.l1_inclusion_block_number,
        "l2_block_numbers": l2_block_numbers,
        "last_anchor_block_number": proposal.last_anchor_block_number,
    });
    if let Some(checkpoint) = proposal.checkpoint {
        shasta_proposal["checkpoint"] = checkpoint;
    }

    Ok((shasta_proposal, range_len))
}

fn v3_status_to_v4_value(conversion: &V4Conversion, status: V3Status) -> Value {
    match status {
        V3Status::Ok {
            proof_type, data, ..
        } => {
            let task_id = format!(
                "legacy-shasta-{}-{}-{}",
                conversion.proposal_id_start, conversion.proposal_id_end, proof_type
            );
            let (status, proof, error) = match data {
                ProofResponse::Status { status } => {
                    let error = task_status_error(&status);
                    (task_status_to_v4(&status), None, error)
                }
                ProofResponse::Proof { proof } => ("completed".to_string(), proof.proof, None),
            };

            let mut data = json!({
                "task_id": task_id,
                "status": status,
                "proof": proof,
            });
            if let Some(error) = error {
                data["error"] = json!(error);
            }

            json!({
                "status": "ok",
                "proof_type": proof_type.to_string(),
                "proposal_id_start": conversion.proposal_id_start,
                "proposal_id_end": conversion.proposal_id_end,
                "data": data,
            })
        }
        V3Status::Error { error, message } => v4_error(&error, message),
    }
}

fn task_status_to_v4(status: &TaskStatus) -> String {
    match status {
        TaskStatus::Success => "completed".to_string(),
        TaskStatus::Registered => "registered".to_string(),
        TaskStatus::WorkInProgress => "work_in_progress".to_string(),
        TaskStatus::Cancelled
        | TaskStatus::Cancelled_NeverStarted
        | TaskStatus::Cancelled_Aborted
        | TaskStatus::CancellationInProgress => "cancelled".to_string(),
        TaskStatus::ZKAnyNotDrawn => "zk_any_not_drawn".to_string(),
        _ => "failed".to_string(),
    }
}

fn task_status_error(status: &TaskStatus) -> Option<String> {
    match status {
        TaskStatus::Success
        | TaskStatus::Registered
        | TaskStatus::WorkInProgress
        | TaskStatus::ZKAnyNotDrawn => None,
        TaskStatus::NetworkFailure(error)
        | TaskStatus::IoFailure(error)
        | TaskStatus::AnyhowError(error)
        | TaskStatus::GuestProverFailure(error)
        | TaskStatus::TaskDbCorruption(error) => Some(error.clone()),
        other => Some(format!("{other:?}")),
    }
}

fn v4_error(error: &str, message: String) -> Value {
    json!({
        "status": "error",
        "error": error,
        "message": message,
    })
}

pub fn create_router() -> Router<Actor> {
    Router::new().route("/proof/proposal", post(proposal_proof_handler))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn converts_v4_proposal_ranges_to_shasta_l2_block_numbers() {
        let input = json!({
            "proof_type": "sgx",
            "aggregate": false,
            "proposals": [{
                "proposal_id": 42,
                "l1_inclusion_block_number": 100,
                "l2_block_number_start": 200,
                "l2_block_number_end": 202,
                "last_anchor_block_number": 199
            }],
            "prover": "0x0000000000000000000000000000000000000000"
        });

        let converted = super::v4_to_shasta_value(input).expect("convert v4 request");
        assert_eq!(converted["proof_type"], "sgx");
        assert_eq!(converted["aggregate"], false);
        assert_eq!(
            converted["prover"],
            "0x0000000000000000000000000000000000000000"
        );
        assert_eq!(converted["proposals"][0]["proposal_id"], 42);
        assert_eq!(
            converted["proposals"][0]["l2_block_numbers"],
            json!([200, 201, 202])
        );
        assert!(converted["proposals"][0]
            .get("l2_block_number_start")
            .is_none());
        assert!(converted["proposals"][0]
            .get("l2_block_number_end")
            .is_none());
    }

    #[test]
    fn rejects_v4_proposal_range_with_end_before_start() {
        let input = json!({
            "proof_type": "sgx",
            "proposals": [{
                "proposal_id": 42,
                "l1_inclusion_block_number": 100,
                "l2_block_number_start": 202,
                "l2_block_number_end": 200,
                "last_anchor_block_number": 199
            }]
        });

        let err = super::v4_to_shasta_value(input).expect_err("reject invalid range");
        assert!(err.contains("l2_block_number_end must be greater than or equal to"));
    }
}
