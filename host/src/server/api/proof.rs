use std::{fs::File, path::PathBuf};

use axum::{debug_handler, extract::State, routing::post, Json, Router};
use raiko_lib::{
    input::{get_input_path, GuestInput},
    Measurement,
};
use serde_json::Value;
use utoipa::OpenApi;

use crate::{
    error::{HostError, HostResult},
    metrics::{
        dec_current_req, inc_current_req, inc_guest_error, inc_guest_success, inc_host_error,
        inc_host_req_count, observe_total_time,
    },
    request::{ProofRequest, ProofRequestOpt},
    ProverState,
};

fn get_cached_input(
    cache_path: &Option<PathBuf>,
    block_number: u64,
    network: &str,
) -> Option<GuestInput> {
    cache_path
        .as_ref()
        .map(|dir| get_input_path(dir, block_number, network))
        .and_then(|path| {
            File::open(path)
                .map(|file| bincode::deserialize_from(file).ok())
                .ok()
                .flatten()
        })
}

fn set_cached_input(
    cache_path: &Option<PathBuf>,
    block_number: u64,
    network: &str,
    input: GuestInput,
) -> HostResult<()> {
    if let Some(dir) = cache_path.as_ref() {
        let path = get_input_path(dir, block_number, network);
        if !path.exists() {
            let file = File::create(&path).map_err(<std::io::Error as Into<HostError>>::into)?;
            println!("caching input for {path:?}");
            bincode::serialize_into(file, &input).map_err(|e| HostError::Anyhow(e.into()))?;
        }
    }
    Ok(())
}

#[utoipa::path(post, path = "/proof",
    tag = "Prooving",
    responses (
        (status = 200, description = "Successfuly created proof for request")
    )
)]
#[debug_handler(state = ProverState)]
/// Generate a proof for requested config.
///
/// Accepts a proof request and generates a proof with the specified guest prover.
/// The guest provers currently available are:
/// - native - constructs a block and checks for equality
/// - sgx - uses the sgx environment to construct a block and produce proof of execution
/// - sp1 - uses the sp1 prover
/// - risc0 - uses the risc0 prover
async fn proof_handler(
    State(ProverState { opts }): State<ProverState>,
    Json(req): Json<ProofRequestOpt>,
) -> HostResult<Json<Value>> {
    inc_current_req();
    // Override the existing proof request config from the config file and command line
    // options with the request from the client.
    let mut config = opts.proof_request_opt.clone();
    config.merge(&req);

    // Construct the actual proof request from the available configs.
    let proof_request = ProofRequest::try_from(config).map_err(|e| {
        dec_current_req();
        e
    })?;
    inc_host_req_count(proof_request.block_number);

    println!(
        "# Generating proof for block {} on {}",
        proof_request.block_number,
        proof_request.network.to_string()
    );

    // Check for a cached input for the given request config.
    let cached_input = get_cached_input(
        &opts.cache_path,
        proof_request.block_number,
        &proof_request.network.to_string(),
    );

    // Execute the proof generation.
    let total_time = Measurement::start("", false);
    let (input, proof) = proof_request.execute(cached_input).await.map_err(|e| {
        dec_current_req();
        let total_time = total_time.stop_with("====> Complete proof generated");
        observe_total_time(proof_request.block_number, total_time.as_millis(), true);
        match e {
            HostError::GuestError(e) => {
                inc_guest_error(&proof_request.proof_type, proof_request.block_number);
                HostError::GuestError(e)
            }
            e => {
                inc_host_error(proof_request.block_number);
                e
            }
        }
    })?;
    inc_guest_success(&proof_request.proof_type, proof_request.block_number);
    let total_time = total_time.stop_with("====> Complete proof generated");
    observe_total_time(proof_request.block_number, total_time.as_millis(), true);

    // Cache the input for future use.
    set_cached_input(
        &opts.cache_path,
        proof_request.block_number,
        &proof_request.network.to_string(),
        input,
    )
    .map_err(|e| {
        dec_current_req();
        e
    })?;

    dec_current_req();

    Ok(Json(proof))
}

#[derive(OpenApi)]
#[openapi(paths(proof_handler))]
struct Docs;

pub fn create_docs() -> utoipa::openapi::OpenApi {
    Docs::openapi()
}

pub fn create_router() -> Router<ProverState> {
    Router::new().route("/:proof", post(proof_handler))
}
