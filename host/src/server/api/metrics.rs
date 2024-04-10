use axum::{body::Body, debug_handler, http::header, response::Response, routing::get, Router};
use prometheus::{Encoder, TextEncoder};
use utoipa::OpenApi;

use crate::{error::HostResult, ProverState};

#[utoipa::path(
    get,
    path = "/metrics",
    tag = "Metrics",
    responses (
        (status = 200, description = "The request was successful", body = Body),
    ),
)]
#[debug_handler(state = ProverState)]
/// Get prometheus metrics
async fn handler() -> HostResult<Response> {
    let encoder = TextEncoder::new();
    let mut buffer = vec![];
    let mf = prometheus::gather();
    encoder.encode(&mf, &mut buffer).unwrap();
    let resp = Response::builder()
        .header(header::CONTENT_TYPE, encoder.format_type())
        .body(Body::from(buffer))
        .unwrap();
    Ok(resp)
}

#[derive(OpenApi)]
#[openapi(paths(handler))]
struct Docs;

pub fn create_docs() -> utoipa::openapi::OpenApi {
    Docs::openapi()
}

pub fn create_router() -> Router<ProverState> {
    Router::new().route("/", get(handler))
}
