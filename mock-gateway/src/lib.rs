pub mod generated;
pub mod router;
pub mod state;

pub use router::app;
pub use state::{AppState, MockContext};

use serde_json::{json, Value};

pub fn proposal_batch_id(body: &Value) -> Option<u64> {
    body.get("proposals")
        .and_then(Value::as_array)
        .and_then(|proposals| proposals.first())
        .and_then(|proposal| proposal.get("proposal_id"))
        .and_then(Value::as_u64)
}

pub fn proof_type(body: &Value) -> &str {
    body.get("proof_type")
        .and_then(Value::as_str)
        .unwrap_or("native")
}

pub fn ok_status(proof_type: &str, batch_id: Option<u64>, task_status: &str) -> Value {
    json!({
        "status": "ok",
        "proof_type": proof_type,
        "batch_id": batch_id,
        "data": {
            "status": task_status
        }
    })
}

pub fn error_status(error: &str, message: &str) -> Value {
    json!({
        "status": "error",
        "error": error,
        "message": message
    })
}

pub fn mock_proof_response(body: &Value, label: &str) -> Value {
    json!({
        "status": "ok",
        "proof_type": proof_type(body),
        "batch_id": proposal_batch_id(body),
        "data": {
            "proof": {
                "proof": format!("mock-proof:{label}"),
                "input": null,
                "quote": null,
                "uuid": null,
                "kzg_proof": null,
                "extra_data": null
            }
        }
    })
}
