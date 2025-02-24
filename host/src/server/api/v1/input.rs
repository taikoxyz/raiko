use axum::{extract::State, routing::post, Json, Router};
use raiko_core::{interfaces::ProofRequest, provider::rpc::RpcBlockDataProvider, Raiko};
use raiko_lib::consts::SupportedChainSpecs;
use serde_json::Value;
use tokio::time::Instant;
use tracing::info;
use utoipa::OpenApi;

use crate::{
    interfaces::HostResult,
    metrics::observe_prepare_input_time,
    server::api::v1::{ProofResponse, Status},
};
use raiko_reqactor::Actor;

#[utoipa::path(post, path = "/input",
    tag = "Proving",
    request_body = ProofRequestOpt,
    responses (
        (status = 200, description = "Successfully created proof for request", body = Status)
    )
)]
/// Generate a proof for requested config.
///
/// Accepts a proof request and generates a proof with the specified guest prover.
/// The guest provers currently available are:
/// - native - constructs a block and checks for equality
/// - sgx - uses the sgx environment to construct a block and produce proof of execution
/// - sp1 - uses the sp1 prover
/// - risc0 - uses the risc0 prover
async fn input_handler(
    State(actor): State<Actor>,
    Json(req): Json<Value>,
) -> HostResult<Json<Status>> {
    // Override the existing proof request config from the config file and command line
    // options with the request from the client.
    let mut config = actor.default_request_config().clone();
    config.merge(&req)?;

    let proof_request = ProofRequest::try_from(config)?;

    let network = proof_request.network.clone();
    // Skip test on SP1 for now because it's too slow on CI
    let l1_network = proof_request.l1_network.clone();
    let taiko_chain_spec = SupportedChainSpecs::default()
        .get_chain_spec(&network)
        .unwrap();
    let l1_chain_spec = SupportedChainSpecs::default()
        .get_chain_spec(&l1_network)
        .unwrap();
    let block_number = proof_request.block_number;
    info!("generate guest input for block_number: {}", block_number);

    let start_time = Instant::now();
    let provider = RpcBlockDataProvider::new(&taiko_chain_spec.rpc, proof_request.block_number - 1)
        .expect("Could not create RpcBlockDataProvider");
    let raiko = Raiko::new(l1_chain_spec, taiko_chain_spec, proof_request.clone());
    let input = raiko
        .generate_input(provider)
        .await
        .expect("input generation failed");
    let elapsed = start_time.elapsed();
    observe_prepare_input_time(block_number, elapsed, true);
    info!(
        "generate guest input for block_number: {}, done",
        block_number
    );

    let start_time = Instant::now();
    raiko.get_output(&input).expect("output generation failed");
    let elapsed = start_time.elapsed();
    observe_prepare_input_time(block_number, elapsed, true);
    info!(
        "generate guest output for block_number: {}, done",
        block_number
    );

    Ok(axum::Json(Status::Ok {
        data: ProofResponse {
            input: Some(input),
            output: None,
        },
    }))
}

#[derive(OpenApi)]
#[openapi(paths(input_handler))]
struct Docs;

pub fn create_docs() -> utoipa::openapi::OpenApi {
    Docs::openapi()
}

pub fn create_router() -> Router<Actor> {
    Router::new().route("/", post(input_handler))
}
