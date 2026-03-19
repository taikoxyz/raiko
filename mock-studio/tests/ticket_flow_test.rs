use anyhow::Context;
use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use tower::ServiceExt;

use raiko_mock_studio::models::SpecGeneration;
use raiko_mock_studio::{
    app, openrouter::MockPlanner, AppState, FakeHandlerGenerator, FakePlanner, FakeRunner,
    GatewayForwarder, LocalCargoRunner,
};

#[derive(Clone)]
struct FakeGatewayForwarder {
    response: String,
    seen_target: Arc<Mutex<Option<String>>>,
    seen_body: Arc<Mutex<Option<String>>>,
}

impl FakeGatewayForwarder {
    fn success(response: String) -> Self {
        Self {
            response,
            seen_target: Arc::new(Mutex::new(None)),
            seen_body: Arc::new(Mutex::new(None)),
        }
    }
}

#[async_trait]
impl GatewayForwarder for FakeGatewayForwarder {
    async fn forward_shasta_request(
        &self,
        base_url: &str,
        body: &str,
    ) -> anyhow::Result<String> {
        *self.seen_target.lock().unwrap() = Some(base_url.to_string());
        *self.seen_body.lock().unwrap() = Some(body.to_string());
        Ok(self.response.clone())
    }
}

#[tokio::test]
async fn root_page_contains_operator_ui_markers() {
    let app = app(AppState::for_tests(
        FakePlanner::success(),
        FakeHandlerGenerator::success(),
        FakeRunner::success("http://203.0.113.10:4100"),
    ));

    let response = app
        .oneshot(Request::get("/").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let html = String::from_utf8(bytes.to_vec()).unwrap();

    assert!(html.contains("Mock Studio"));
    assert!(html.contains("ticket-requirement"));
    assert!(html.contains("ticket-history"));
    assert!(html.contains("gateway-request"));
    assert!(html.contains("gateway-output"));
    assert!(html.contains("ticket-runtime"));
    assert!(html.contains("ticket-handler-mode"));
    assert!(html.contains("login"));
    assert!(html.contains("setInterval"));
    assert!(!html.contains("id=\"gateway-target\" readonly"));
}

#[tokio::test]
async fn ui_state_endpoint_returns_ticket_history_and_defaults() {
    let app = app(AppState::for_tests(
        FakePlanner::success(),
        FakeHandlerGenerator::success(),
        FakeRunner::success("http://203.0.113.10:4100"),
    ));

    let created = app
        .clone()
        .oneshot(
            Request::post("/api/tickets")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "requirement": "Return error on the 4th /v3/proof/batch/shasta call"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::OK);

    let response = app
        .oneshot(Request::get("/api/ui/state").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let payload: Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(payload["tickets"].as_array().unwrap().len(), 1);
    assert_eq!(payload["tickets"][0]["ticket_id"], "ticket-1");
    assert_eq!(payload["tickets"][0]["handler_mode"], "renderer");
    assert_eq!(payload["preferred_gateway_target"], "");
    let template = payload["gateway_request_template"].as_str().unwrap();
    assert!(template.contains("\"proof_type\": \"native\""));
    assert!(template.contains("\"proposal_id\": 101"));
}

#[tokio::test]
async fn ui_state_reloads_generated_tickets_and_continues_ticket_numbering() {
    let temp = tempdir().unwrap();
    let generated = temp.path().join("generated");
    std::fs::create_dir_all(generated.join("ticket-1")).unwrap();
    std::fs::create_dir_all(generated.join("ticket-2")).unwrap();
    std::fs::write(
        generated.join("ticket-1").join("conversation.md"),
        "first ticket requirement",
    )
    .unwrap();
    std::fs::write(
        generated.join("ticket-1").join("meta.json"),
        serde_json::to_vec_pretty(&json!({
            "rule_id": "ticket-1",
            "summary": "first ticket"
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        generated.join("ticket-1").join("receipt.json"),
        serde_json::to_vec_pretty(&json!({
            "status": "running",
            "base_url": "http://127.0.0.1:28090",
            "error": null
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        generated.join("ticket-2").join("conversation.md"),
        "second ticket requirement",
    )
    .unwrap();
    std::fs::write(
        generated.join("ticket-2").join("meta.json"),
        serde_json::to_vec_pretty(&json!({
            "rule_id": "ticket-2",
            "summary": "second ticket"
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        generated.join("ticket-2").join("receipt.json"),
        serde_json::to_vec_pretty(&json!({
            "status": "failed",
            "base_url": null,
            "error": "build failed"
        }))
        .unwrap(),
    )
    .unwrap();

    let app = app(AppState::for_tests_in(
        generated.clone(),
        FakePlanner::success(),
        FakeHandlerGenerator::success(),
        FakeRunner::success("http://203.0.113.10:4100"),
    ));

    let ui_state = app
        .clone()
        .oneshot(Request::get("/api/ui/state").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(ui_state.status(), StatusCode::OK);

    let ui_state_bytes = axum::body::to_bytes(ui_state.into_body(), usize::MAX)
        .await
        .unwrap();
    let ui_state_payload: Value = serde_json::from_slice(&ui_state_bytes).unwrap();
    assert_eq!(ui_state_payload["tickets"].as_array().unwrap().len(), 2);
    assert_eq!(ui_state_payload["tickets"][0]["ticket_id"], "ticket-1");
    assert_eq!(ui_state_payload["tickets"][0]["gateway_runtime"], "offline");
    assert_eq!(ui_state_payload["tickets"][1]["ticket_id"], "ticket-2");
    assert_eq!(ui_state_payload["tickets"][1]["error"], "build failed");

    let created = app
        .oneshot(
            Request::post("/api/tickets")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "requirement": "new requirement after reload"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::OK);

    let created_bytes = axum::body::to_bytes(created.into_body(), usize::MAX)
        .await
        .unwrap();
    let created_payload: Value = serde_json::from_slice(&created_bytes).unwrap();
    assert_eq!(created_payload["ticket_id"], "ticket-3");
    assert_eq!(created_payload["gateway_runtime"], "online");
}

#[tokio::test]
async fn gateway_proxy_endpoint_returns_raw_gateway_response() {
    let temp = tempdir().unwrap();
    let forwarder = FakeGatewayForwarder::success(
        serde_json::to_string(&json!({
            "status": "ok",
            "proof_type": "sp1",
            "batch_id": 101,
            "data": { "status": "registered" }
        }))
        .unwrap(),
    );
    let app = app(AppState::for_tests_with_gateway_forwarder(
        temp.path().join("generated"),
        FakePlanner::success(),
        FakeHandlerGenerator::success(),
        FakeRunner::success("http://203.0.113.10:4100"),
        Arc::new(forwarder.clone()),
    ));

    let created = app
        .clone()
        .oneshot(
            Request::post("/api/tickets")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "requirement": "Return error on the 4th /v3/proof/batch/shasta call"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::OK);

    let response = app
        .oneshot(
            Request::post("/api/tickets/ticket-1/gateway")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "target": "https://gateway.example",
                        "body": {
                            "aggregate": false,
                            "proof_type": "native",
                            "proposals": [{"proposal_id": 101}]
                        }
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let payload: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(payload["proof_type"], "sp1");
    assert_eq!(payload["status"], "ok");
    assert_eq!(
        forwarder.seen_target.lock().unwrap().as_deref(),
        Some("https://gateway.example")
    );
    assert_eq!(
        forwarder.seen_body.lock().unwrap().as_deref(),
        Some("{\"aggregate\":false,\"proof_type\":\"native\",\"proposals\":[{\"proposal_id\":101}]}")
    );
}

#[tokio::test]
async fn ticket_submission_returns_running_receipt() {
    let app = app(AppState::for_tests(
        FakePlanner::success(),
        FakeHandlerGenerator::success(),
        FakeRunner::success("http://203.0.113.10:4100"),
    ));

    let response = app
        .oneshot(
            Request::post("/api/tickets")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "requirement": "Return error on the 4th /v3/proof/batch/shasta call"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let payload: Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(payload["status"], "running");
    assert_eq!(payload["base_url"], "http://203.0.113.10:4100");
    assert_eq!(payload["rule_id"], "ticket-1");
    assert_eq!(payload["handler_mode"], "renderer");
}

#[tokio::test]
async fn ticket_submission_surfaces_fallback_handler_mode_and_validation_error() {
    let temp = tempdir().unwrap();
    let generated_root = temp.path().join("generated");
    let app = app(AppState::for_tests_in(
        generated_root.clone(),
        FakePlanner::success(),
        FakeHandlerGenerator::success(),
        FakeRunner::success("http://203.0.113.10:4100"),
    ));

    let response = app
        .oneshot(
            Request::post("/api/tickets")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "requirement": "Return error on the 4th /v3/proof/batch/shasta call"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let payload: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(payload["handler_mode"], "renderer");
    assert!(payload["handler_validation_error"].is_null());

    let receipt_bytes = std::fs::read(generated_root.join("ticket-1").join("receipt.json")).unwrap();
    let receipt: Value = serde_json::from_slice(&receipt_bytes).unwrap();
    assert_eq!(receipt["handler_mode"], "renderer");
}

#[tokio::test]
async fn ticket_submission_writes_generated_rule_files() {
    let temp = tempdir().unwrap();
    let app = app(AppState::for_tests_in(
        temp.path().join("generated"),
        FakePlanner::success(),
        FakeHandlerGenerator::success(),
        FakeRunner::success("http://203.0.113.10:4100"),
    ));

    let response = app
        .oneshot(
            Request::post("/api/tickets")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "requirement": "Return error on the 4th /v3/proof/batch/shasta call"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let generated = temp.path().join("generated");
    let rule_dir = generated.join("ticket-1");
    assert!(rule_dir.join("conversation.md").exists());
    assert!(rule_dir.join("meta.json").exists());
    assert!(rule_dir.join("spec.json").exists());
    assert!(rule_dir.join("ticket.rs").exists());
    assert!(rule_dir.join("llm/spec_prompt.md").exists());
    assert!(rule_dir.join("llm/spec_response.json").exists());
    assert!(rule_dir.join("llm/handler_prompt.md").exists());
    assert!(rule_dir.join("llm/handler_response.json").exists());
    assert!(rule_dir.join("build.log").exists());
    assert!(rule_dir.join("runtime.log").exists());
    assert!(rule_dir.join("receipt.json").exists());
    assert!(generated.join("index.json").exists());

    let handler = std::fs::read_to_string(rule_dir.join("ticket.rs")).unwrap();
    assert!(handler.contains("forced failure on 4th request"));
    assert!(handler.contains("pub fn handle_shasta_request"));

    let receipt: Value =
        serde_json::from_slice(&std::fs::read(rule_dir.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(receipt["status"], "running");
    assert_eq!(receipt["base_url"], "http://203.0.113.10:4100");
}

#[tokio::test]
async fn ticket_status_endpoint_returns_saved_ticket() {
    let temp = tempdir().unwrap();
    let app = app(AppState::for_tests_in(
        temp.path().join("generated"),
        FakePlanner::success(),
        FakeHandlerGenerator::success(),
        FakeRunner::success("http://203.0.113.10:4100"),
    ));

    let app = app.clone();
    let create = app
        .clone()
        .oneshot(
            Request::post("/api/tickets")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "requirement": "Return error on the 4th /v3/proof/batch/shasta call"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::OK);

    let response = app
        .oneshot(
            Request::get("/api/tickets/ticket-1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let payload: Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(payload["status"], "running");
    assert_eq!(payload["rule_id"], "ticket-1");
}

#[tokio::test]
#[ignore = "requires unsandboxed local port binding to launch mock_gateway"]
async fn local_runner_builds_starts_and_writes_receipt() {
    let temp = tempdir().unwrap();
    let app = app(AppState::for_real_runner_tests_in(
        temp.path().join("generated"),
        FakePlanner::success(),
        FakeHandlerGenerator::success(),
        LocalCargoRunner::default(),
    ));

    let response = app
        .oneshot(
            Request::post("/api/tickets")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "requirement": "Return error on the 4th /v3/proof/batch/shasta call"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let payload: Value = serde_json::from_slice(&bytes).unwrap();
    if payload["status"] != "running" {
        let rule_dir = temp.path().join("generated").join("ticket-1");
        if let Ok(build_log) = std::fs::read_to_string(rule_dir.join("build.log")) {
            eprintln!("build.log:\n{build_log}");
        }
        if let Ok(runtime_log) = std::fs::read_to_string(rule_dir.join("runtime.log")) {
            eprintln!("runtime.log:\n{runtime_log}");
        }
    }
    assert_eq!(payload["status"], "running");

    let base_url = payload["base_url"].as_str().unwrap();
    let health = reqwest::get(format!("{base_url}/health")).await.unwrap();
    assert_eq!(health.status(), reqwest::StatusCode::OK);

    let shasta_response = reqwest::Client::new()
        .post(format!("{base_url}/v3/proof/batch/shasta"))
        .json(&json!({
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
        .send()
        .await
        .unwrap();
    assert_eq!(shasta_response.status(), reqwest::StatusCode::OK);

    let generated = temp.path().join("generated").join("ticket-1");
    assert!(generated.join("build.log").exists());
    assert!(generated.join("runtime.log").exists());
    assert!(generated.join("receipt.json").exists());
}

#[tokio::test]
async fn ticket_submission_updates_legacy_index_without_base_url() {
    let temp = tempdir().unwrap();
    let generated_root = temp.path().join("generated");
    std::fs::create_dir_all(&generated_root).unwrap();
    std::fs::write(
        generated_root.join("index.json"),
        r#"
[
  {
    "rule_id": "example-fourth-call-error",
    "summary": "legacy example",
    "status": "ready"
  }
]
"#,
    )
    .unwrap();

    let app = app(AppState::for_tests_in(
        generated_root.clone(),
        FakePlanner::success(),
        FakeHandlerGenerator::success(),
        FakeRunner::success("http://203.0.113.10:4100"),
    ));

    let response = app
        .oneshot(
            Request::post("/api/tickets")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "requirement": "Return error on the 4th /v3/proof/batch/shasta call"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let payload: Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(payload["status"], "running");
    assert_eq!(payload["base_url"], "http://203.0.113.10:4100");

    let index: Value =
        serde_json::from_slice(&std::fs::read(generated_root.join("index.json")).unwrap()).unwrap();
    assert_eq!(index.as_array().unwrap().len(), 2);
    assert_eq!(index[0]["base_url"], "");
    assert_eq!(index[1]["base_url"], "http://203.0.113.10:4100");
}

#[derive(Clone)]
struct FailingPlanner;

#[async_trait]
impl MockPlanner for FailingPlanner {
    async fn plan(&self, _requirement: &str) -> anyhow::Result<SpecGeneration> {
        Err(anyhow::anyhow!("openrouter 401 unauthorized")).context("planner transport failed")
    }
}

#[tokio::test]
async fn ticket_submission_exposes_full_planner_error_chain() {
    let app = app(AppState::new(
        std::sync::Arc::new(FailingPlanner),
        std::sync::Arc::new(FakeHandlerGenerator::success()),
        std::sync::Arc::new(FakeRunner::success("http://127.0.0.1:4100")),
    ));

    let response = app
        .oneshot(
            Request::post("/api/tickets")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "requirement": "planner should fail"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let payload: Value = serde_json::from_slice(&bytes).unwrap();

    let error = payload["error"].as_str().unwrap();
    assert!(error.contains("planner failed to produce a mock spec"));
    assert!(error.contains("planner transport failed"));
    assert!(error.contains("openrouter 401 unauthorized"));
}
