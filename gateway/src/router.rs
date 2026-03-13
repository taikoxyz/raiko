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

    tracing::info!("Forwarding to backend {backend_index}: {backend_url}");

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

    if !status.is_success() {
        let body_preview = String::from_utf8_lossy(body.as_ref());
        let preview = if body_preview.len() > 500 {
            format!("{}...", &body_preview[..500])
        } else {
            body_preview.to_string()
        };
        tracing::warn!(
            status = %status,
            body = %preview,
            "Backend returned error"
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
