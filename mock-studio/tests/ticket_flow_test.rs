use anyhow::Context;
use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::{json, Value};
use tempfile::tempdir;
use tower::ServiceExt;

use raiko_mock_studio::models::SpecGeneration;
use raiko_mock_studio::{
    app, openrouter::MockPlanner, AppState, FakeHandlerGenerator, FakePlanner, FakeRunner,
    LocalCargoRunner,
};

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
