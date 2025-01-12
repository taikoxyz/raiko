use axum::{extract::State, routing::post, Json, Router};
use serde_json::Value;
use utoipa::OpenApi;

use crate::{interfaces::HostResult, server::api::v1::Status};
use raiko_reqactor::Gateway;

#[utoipa::path(post, path = "/proof",
    tag = "Proving",
    request_body = ProofRequestOpt,
    responses (
        (status = 200, description = "Successfully created proof for request", body = Status)
    )
)]
// #[debug_handler(state = Gateway)]
/// Generate a proof for requested config.
///
/// Accepts a proof request and generates a proof with the specified guest prover.
/// The guest provers currently available are:
/// - native - constructs a block and checks for equality
/// - sgx - uses the sgx environment to construct a block and produce proof of execution
/// - sp1 - uses the sp1 prover
/// - risc0 - uses the risc0 prover
async fn proof_handler<P: raiko_reqpool::Pool>(
    State(_gateway): State<Gateway<P>>,
    Json(_req): Json<Value>,
) -> HostResult<Json<Status>> {
    unreachable!("deprecated")
}

#[derive(OpenApi)]
#[openapi(paths(proof_handler))]
struct Docs;

pub fn create_docs() -> utoipa::openapi::OpenApi {
    Docs::openapi()
}

pub fn create_router<P: raiko_reqpool::Pool + 'static>() -> Router<Gateway<P>> {
    Router::new().route("/", post(proof_handler))
}
