use std::{fs::File, path::PathBuf};

use axum::{debug_handler, extract::State, routing::post, Json, Router};
use raiko_lib::{
    input::{get_input_path, GuestInput},
    prover::Proof,
};
use utoipa::OpenApi;

use crate::{
    error::{HostError, HostResult},
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
        .map(|path| {
            File::open(path)
                .map(|file| bincode::deserialize_from(file).ok())
                .ok()
                .flatten()
        })
        .flatten()
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
            let file =
                File::create(&path).map_err(|e| <std::io::Error as Into<HostError>>::into(e))?;
            println!("caching input for {path:?}");
            bincode::serialize_into(file, &input).map_err(|e| HostError::Anyhow(e.into()))?;
        }
    }
    Ok(())
}

#[utoipa::path(get, path = "/proof")]
#[debug_handler(state = ProverState)]
async fn handler(
    State(ProverState { opts }): State<ProverState>,
    Json(req): Json<ProofRequestOpt>,
) -> HostResult<Json<Proof>> {
    // Override the existing proof request config from the config file and command line
    // options with the request from the client.
    let mut config = opts.proof_request_opt.clone();
    config.merge(&req);

    // Construct the actual proof request from the available configs.
    let proof_request = ProofRequest::try_from(config)?;

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
    let (input, proof) = proof_request.execute(cached_input).await?;

    // Cache the input for future use.
    set_cached_input(
        &opts.cache_path,
        proof_request.block_number,
        &proof_request.network.to_string(),
        input,
    )?;

    Ok(Json(proof))
}

#[derive(OpenApi)]
#[openapi(paths(handler))]
struct Docs;

pub fn create_docs() -> utoipa::openapi::OpenApi {
    Docs::openapi()
}

pub fn create_router() -> Router<ProverState> {
    Router::new().route("/:proof", post(handler))
}
