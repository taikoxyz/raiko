use std::{fs::File, path::PathBuf};

use crate::metrics::observe_guest_time;
use crate::metrics::observe_prepare_input_time;
use axum::{debug_handler, extract::State, routing::post, Json, Router};
use raiko_lib::{
    consts::get_network_spec,
    input::{get_input_path, GuestInput},
    Measurement,
};
use serde_json::Value;
use tracing::{debug, info};
use utoipa::OpenApi;

use crate::{
    error::{HostError, HostResult},
    memory,
    metrics::{
        dec_current_req, inc_current_req, inc_guest_error, inc_guest_success, inc_host_error,
        inc_host_req_count, observe_total_time,
    },
    raiko::Raiko,
    request::ProofRequest,
    rpc_provider::RpcBlockDataProvider,
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
            info!("caching input for {path:?}");
            bincode::serialize_into(file, &input).map_err(|e| HostError::Anyhow(e.into()))?;
        }
    }
    Ok(())
}

#[utoipa::path(post, path = "/proof",
    tag = "Proving",
    request_body = ProofRequestOpt,
    responses (
        (status = 200, description = "Successfully created proof for request", body = ProofResponse)
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
    Json(req): Json<Value>,
) -> HostResult<Json<Value>> {
    inc_current_req();
    // Override the existing proof request config from the config file and command line
    // options with the request from the client.
    let mut config = opts.proof_request_opt.clone();
    config.merge(&req)?;

    // Construct the actual proof request from the available configs.
    let proof_request = ProofRequest::try_from(config).map_err(|e| {
        dec_current_req();
        e
    })?;
    inc_host_req_count(proof_request.block_number);

    debug!(
        "# Generating proof for block {} on {}",
        proof_request.block_number, proof_request.network
    );

    // Check for a cached input for the given request config.
    let cached_input = get_cached_input(
        &opts.cache_path,
        proof_request.block_number,
        &proof_request.network.to_string(),
    );

    let chain_spec = get_network_spec(proof_request.network);

    // Execute the proof generation.
    let total_time = Measurement::start("", false);

    let raiko = Raiko::new(chain_spec, proof_request.clone());
    let input = if let Some(cached_input) = cached_input {
        debug!("Using cached input");
        cached_input
    } else {
        memory::reset_stats();
        let measurement = Measurement::start("Generating input...", false);
        let provider =
            RpcBlockDataProvider::new(&proof_request.rpc.clone(), proof_request.block_number - 1)?;
        let input = raiko.generate_input(provider).await?;
        let input_time = measurement.stop_with("=> Input generated");
        observe_prepare_input_time(proof_request.block_number, input_time, true);
        memory::print_stats("Input generation peak memory used: ");
        input
    };
    memory::reset_stats();
    let output = raiko.get_output(&input)?;
    memory::print_stats("Guest program peak memory used: ");

    memory::reset_stats();
    let measurement = Measurement::start("Generating proof...", false);
    let proof = raiko.prove(input.clone(), &output).await.map_err(|e| {
        dec_current_req();
        let total_time = total_time.stop_with("====> Proof generation failed");
        observe_total_time(proof_request.block_number, total_time, false);
        match e {
            HostError::Guest(e) => {
                inc_guest_error(&proof_request.proof_type, proof_request.block_number);
                HostError::Guest(e)
            }
            e => {
                inc_host_error(proof_request.block_number);
                e
            }
        }
    })?;
    let guest_time = measurement.stop_with("=> Proof generated");
    observe_guest_time(
        &proof_request.proof_type,
        proof_request.block_number,
        guest_time,
        true,
    );
    memory::print_stats("Prover peak memory used: ");

    inc_guest_success(&proof_request.proof_type, proof_request.block_number);
    let total_time = total_time.stop_with("====> Complete proof generated");
    observe_total_time(proof_request.block_number, total_time, true);

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
    Router::new().route("/", post(proof_handler))
}
