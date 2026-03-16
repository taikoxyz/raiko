use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::{json, Value};
use tower::ServiceExt;

use raiko_mock_gateway::{app, mock_proof_response, AppState};

fn shasta_body() -> Vec<u8> {
    serde_json::to_vec(&json!({
        "l1_network": "ethereum",
        "network": "taiko",
        "proof_type": "native",
        "prover": "0x0000000000000000000000000000000000000000",
        "aggregate": false,
        "proposals": [
            {
                "proposal_id": 101,
                "l1_inclusion_block_number": 9001
            }
        ]
    }))
    .unwrap()
}

#[tokio::test]
async fn mock_gateway_returns_configured_error_on_fourth_call() {
    let app = app(AppState::default());

    for expected_error in [false, false, false, true] {
        let response = app
            .clone()
            .oneshot(
                Request::post("/v3/proof/batch/shasta")
                    .header("content-type", "application/json")
                    .body(Body::from(shasta_body()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let payload: Value = serde_json::from_slice(&bytes).unwrap();

        if expected_error {
            assert_eq!(payload["status"], "error");
        } else {
            assert_eq!(payload["status"], "ok");
        }
    }

    let health = app
        .oneshot(Request::get("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(health.status(), StatusCode::OK);
}

#[test]
fn app_state_can_mark_request_memory_by_body() {
    let state = AppState::default();
    let body: Value = serde_json::from_slice(&shasta_body()).unwrap();
    let ctx = state.new_context();

    assert!(!ctx.has_seen_request(&body));
    ctx.mark_request_seen(&body);
    assert!(ctx.has_seen_request(&body));
}

#[test]
fn mock_proof_response_returns_ok_proof_envelope() {
    let body: Value = serde_json::from_slice(&shasta_body()).unwrap();
    let payload = mock_proof_response(&body, "repeat-request");

    assert_eq!(payload["status"], "ok");
    assert_eq!(
        payload["data"]["proof"]["proof"],
        "mock-proof:repeat-request"
    );
    assert!(payload["data"]["proof"].get("input").is_some());
}
