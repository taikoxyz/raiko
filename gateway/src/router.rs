use crate::{backend_index, route_key_from_body_with_defaults, Config};
use axum::{
    body::{Body, Bytes},
    extract::State,
    http::{
        header::{AUTHORIZATION, CONTENT_TYPE},
        HeaderMap, HeaderValue, Method, StatusCode, Uri,
    },
    response::Response,
    routing::{any, get, post},
    Router,
};
use std::collections::HashSet;

const SHASTA_PATH: &str = "/proof/batch/shasta";
const SHASTA_V3_PATH: &str = "/v3/proof/batch/shasta";

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub client: reqwest::Client,
}

impl AppState {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }
}

pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/", get(health))
        .route("/health", get(health))
        .route("/healthz", get(health))
        .route(SHASTA_PATH, post(forward_shasta_request))
        .route(SHASTA_V3_PATH, post(forward_shasta_request))
        .fallback(any(forward_passthrough_request))
        .with_state(state)
}

async fn forward_shasta_request(
    State(state): State<AppState>,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, (StatusCode, String)> {
    check_api_key(&state.config.valid_api_keys(), &headers)
        .map_err(|e| (StatusCode::UNAUTHORIZED, e))?;
    let route_key = route_key_from_body_with_defaults(&body, &state.config.route_defaults())
        .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
    let backend_index = backend_index(&route_key, state.config.backend_replicas());
    let backend_base = state
        .config
        .backend_url(backend_index)
        .ok_or_else(|| (StatusCode::INTERNAL_SERVER_ERROR, "no backend for index".to_string()))?;
    let backend_url = format!("{backend_base}{uri}");

    tracing::info!(uri = %uri, backend_index = backend_index, backend_url = %backend_url, "Forwarding shasta");

    forward_request(&state.client, &headers, Method::POST, backend_url, body).await
}

async fn forward_passthrough_request(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, (StatusCode, String)> {
    check_api_key(&state.config.valid_api_keys(), &headers)
        .map_err(|e| (StatusCode::UNAUTHORIZED, e))?;
    let backend_url = format!("{}{}", state.config.shared_backend_url(), uri);
    tracing::info!(method = %method, uri = %uri, backend_url = %backend_url, "Forwarding passthrough");
    forward_request(&state.client, &headers, method, backend_url, body).await
}

async fn forward_request(
    client: &reqwest::Client,
    headers: &HeaderMap,
    method: Method,
    backend_url: String,
    body: Bytes,
) -> Result<Response, (StatusCode, String)> {
    let method = reqwest::Method::from_bytes(method.as_str().as_bytes())
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;

    let mut request = client.request(method, backend_url).body(body.to_vec());

    if let Some(content_type) = headers.get(CONTENT_TYPE).and_then(header_value_to_str) {
        request = request.header(reqwest::header::CONTENT_TYPE, content_type);
    }
    if let Some(authorization) = headers.get(AUTHORIZATION).and_then(header_value_to_str) {
        request = request.header(reqwest::header::AUTHORIZATION, authorization);
    }
    if let Some(api_key) = headers.get("x-api-key").and_then(header_value_to_str) {
        request = request.header("x-api-key", api_key);
    }

    let upstream = request
        .send()
        .await
        .map_err(|error| (StatusCode::BAD_GATEWAY, error.to_string()))?;

    let status = StatusCode::from_u16(upstream.status().as_u16())
        .map_err(|error| (StatusCode::BAD_GATEWAY, error.to_string()))?;
    let content_type = upstream
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = upstream
        .bytes()
        .await
        .map_err(|error| (StatusCode::BAD_GATEWAY, error.to_string()))?;

    let body_preview = String::from_utf8_lossy(body.as_ref());
    let preview = if body_preview.len() > 500 {
        format!("{}...", &body_preview[..500])
    } else {
        body_preview.to_string()
    };

    tracing::info!(status = %status, body = %preview, "Backend response");

    if !status.is_success() {
        tracing::warn!(
            status = %status,
            body = %preview,
            "Backend returned HTTP error"
        );
    } else if is_error_response_body(body.as_ref()) {
        tracing::warn!(
            body = %preview,
            "Backend returned 200 with error in body"
        );
    }

    let mut response = Response::builder().status(status);
    if let Some(content_type) = content_type {
        response = response.header(CONTENT_TYPE, content_type);
    }

    response
        .body(Body::from(body))
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}

async fn health() -> &'static str {
    "ok"
}

fn header_value_to_str(value: &HeaderValue) -> Option<&str> {
    value.to_str().ok()
}

/// Returns Ok(()) if API key is valid or check is disabled. Err(msg) if invalid/missing.
fn check_api_key(valid_keys: &HashSet<String>, headers: &HeaderMap) -> Result<(), String> {
    if valid_keys.is_empty() {
        return Ok(());
    }
    let key = extract_api_key_from_headers(headers);
    if key.is_empty() {
        tracing::warn!("No API key provided");
        return Err("No API key provided".to_string());
    }
    if valid_keys.contains(&key) {
        Ok(())
    } else {
        tracing::warn!(key = %key, "Invalid API key");
        Err("Invalid API key".to_string())
    }
}

/// TaskStatus variants that indicate failure (raiko returns 200 with error in data.status).
const TASK_STATUS_ERROR_KEYS: &[&str] = &[
    "anyhow_error",
    "guest_prover_failure",
    "network_failure",
    "io_failure",
    "proof_failure_generic",
    "proof_failure_out_of_memory",
    "invalid_or_unsupported_block",
    "task_db_corruption",
    "unspecified_failure_reason",
    "system_paused",
];

/// Returns true if the response body indicates an error (raiko returns 200 with error in JSON body).
fn is_error_response_body(body: &[u8]) -> bool {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(body) else {
        return false;
    };
    let obj = v.as_object();
    let Some(obj) = obj else {
        return false;
    };
    // raiko v2/v3 Status::Error: {"status": "error", "error": "...", "message": "..."}
    if let Some(status) = obj.get("status").and_then(|s| s.as_str()) {
        if status.eq_ignore_ascii_case("error") {
            return true;
        }
    }
    // HostError / legacy: {"error": "...", "message": "..."}
    if obj.contains_key("error") && obj.contains_key("message") {
        return true;
    }
    // raiko Status::Ok with data.status = TaskStatus error variant
    // e.g. {"status":"ok","data":{"status":{"anyhow_error":"..."}}}
    if let Some(data) = obj.get("data").and_then(|d| d.as_object()) {
        if let Some(task_status) = data.get("status") {
            if let Some(ts_obj) = task_status.as_object() {
                if ts_obj.keys().any(|k| TASK_STATUS_ERROR_KEYS.contains(&k.as_str())) {
                    return true;
                }
            }
        }
    }
    false
}

fn extract_api_key_from_headers(headers: &HeaderMap) -> String {
    if let Some(v) = headers.get("x-api-key").and_then(header_value_to_str) {
        return v.trim().to_string();
    }
    if let Some(v) = headers.get(AUTHORIZATION).and_then(header_value_to_str) {
        if let Some(bearer) = v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer ")) {
            return bearer.trim().to_string();
        }
    }
    String::new()
}
