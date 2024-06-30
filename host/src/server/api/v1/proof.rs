use std::{fs::File, path::PathBuf};

use axum::{debug_handler, extract::State, routing::post, Json, Router};
use raiko_core::{
    interfaces::{ProofRequest, RaikoError},
    provider::{rpc::RpcBlockDataProvider, BlockDataProvider},
    Raiko,
};
use raiko_lib::{
    input::{get_input_path, GuestInput},
    Measurement,
};
use serde_json::Value;
use tracing::{debug, info};
use utoipa::OpenApi;

use crate::{
    interfaces::{HostError, HostResult},
    memory,
    metrics::{
        dec_current_req, inc_current_req, inc_guest_error, inc_guest_req_count, inc_guest_success,
        inc_host_error, inc_host_req_count, observe_guest_time, observe_prepare_input_time,
        observe_total_time,
    },
    server::api::v1::ProofResponse,
    ProverState,
};

fn get_cached_input(
    cache_path: &Option<PathBuf>,
    block_number: u64,
    network: &str,
) -> Option<GuestInput> {
    let dir = cache_path.as_ref()?;

    let path = get_input_path(dir, block_number, network);

    let file = File::open(path).ok()?;

    bincode::deserialize_from(file).ok()
}

fn set_cached_input(
    cache_path: &Option<PathBuf>,
    block_number: u64,
    network: &str,
    input: &GuestInput,
) -> HostResult<()> {
    let Some(dir) = cache_path.as_ref() else {
        return Ok(());
    };

    let path = get_input_path(dir, block_number, network);
    info!("caching input for {path:?}");

    let file = File::create(&path).map_err(<std::io::Error as Into<HostError>>::into)?;
    bincode::serialize_into(file, input).map_err(|e| HostError::Anyhow(e.into()))
}

async fn validate_cache_input(
    cached_input: Option<GuestInput>,
    provider: &RpcBlockDataProvider,
) -> HostResult<GuestInput> {
    if let Some(cache_input) = cached_input {
        debug!("Using cached input");
        let blocks = provider
            .get_blocks(&[(cache_input.block_number, false)])
            .await?;
        let block = blocks
            .first()
            .ok_or_else(|| RaikoError::RPC("No block data for the requested block".to_owned()))?;

        let cached_block_hash = cache_input.block.header.hash_slow();
        let real_block_hash = block.header.hash.unwrap();
        debug!(
            "cache_block_hash={:?}, real_block_hash={:?}",
            cached_block_hash, real_block_hash
        );

        // double check if cache is valid
        if cached_block_hash == real_block_hash {
            Ok(cache_input)
        } else {
            Err(HostError::InvalidRequestConfig(
                "Cached input is not valid".to_owned(),
            ))
        }
    } else {
        Err(HostError::InvalidRequestConfig(
            "Cached input is not enabled".to_owned(),
        ))
    }
}

async fn handle_proof(
    ProverState {
        opts,
        chain_specs: support_chain_specs,
    }: ProverState,
    req: Value,
) -> HostResult<ProofResponse> {
    // Override the existing proof request config from the config file and command line
    // options with the request from the client.
    let mut config = opts.proof_request_opt.clone();
    config.merge(&req)?;

    // Construct the actual proof request from the available configs.
    let proof_request = ProofRequest::try_from(config)?;
    inc_host_req_count(proof_request.block_number);
    inc_guest_req_count(&proof_request.proof_type, proof_request.block_number);

    info!(
        "# Generating proof for block {} on {}",
        proof_request.block_number, proof_request.network
    );

    // Check for a cached input for the given request config.
    let cached_input = get_cached_input(
        &opts.cache_path,
        proof_request.block_number,
        &proof_request.network.to_string(),
    );

    let l1_chain_spec = support_chain_specs
        .get_chain_spec(&proof_request.l1_network.to_string())
        .ok_or_else(|| HostError::InvalidRequestConfig("Unsupported l1 network".to_string()))?;

    let taiko_chain_spec = support_chain_specs
        .get_chain_spec(&proof_request.network.to_string())
        .ok_or_else(|| HostError::InvalidRequestConfig("Unsupported raiko network".to_string()))?;

    // Execute the proof generation.
    let total_time = Measurement::start("", false);

    let raiko = Raiko::new(
        l1_chain_spec.clone(),
        taiko_chain_spec.clone(),
        proof_request.clone(),
    );
    let provider = RpcBlockDataProvider::new(
        &taiko_chain_spec.rpc.clone(),
        proof_request.block_number - 1,
    )?;
    let input = match validate_cache_input(cached_input, &provider).await {
        Ok(cache_input) => cache_input,
        Err(_) => {
            // no valid cache
            memory::reset_stats();
            let measurement = Measurement::start("Generating input...", false);
            let input = raiko.generate_input(provider).await?;
            let input_time = measurement.stop_with("=> Input generated");
            observe_prepare_input_time(proof_request.block_number, input_time, true);
            memory::print_stats("Input generation peak memory used: ");
            input
        }
    };
    memory::reset_stats();
    let output = raiko.get_output(&input)?;
    memory::print_stats("Guest program peak memory used: ");

    memory::reset_stats();
    let measurement = Measurement::start("Generating proof...", false);
    let proof = raiko.prove(input.clone(), &output).await.map_err(|e| {
        let total_time = total_time.stop_with("====> Proof generation failed");
        observe_total_time(proof_request.block_number, total_time, false);
        match e {
            RaikoError::Guest(e) => {
                inc_guest_error(&proof_request.proof_type, proof_request.block_number);
                HostError::Core(e.into())
            }
            e => {
                inc_host_error(proof_request.block_number);
                e.into()
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
        &input,
    )?;

    ProofResponse::try_from(proof)
}

#[utoipa::path(post, path = "/proof",
    tag = "Proving",
    request_body = ProofRequestOpt,
    responses (
        (status = 200, description = "Successfully created proof for request", body = Status)
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
    State(prover_state): State<ProverState>,
    Json(req): Json<Value>,
) -> HostResult<ProofResponse> {
    inc_current_req();
    handle_proof(prover_state, req).await.map_err(|e| {
        dec_current_req();
        e
    })
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

#[cfg(test)]
mod test {
    use super::*;
    use alloy_primitives::{Address, B256};
    use raiko_core::interfaces::ProofType;
    use raiko_lib::consts::{Network, SupportedChainSpecs};
    use raiko_lib::input::BlobProofType;

    async fn create_cache_input(
        l1_network: &String,
        network: &String,
        block_number: u64,
    ) -> (GuestInput, RpcBlockDataProvider) {
        let l1_chain_spec = SupportedChainSpecs::default()
            .get_chain_spec(l1_network)
            .unwrap();
        let taiko_chain_spec = SupportedChainSpecs::default()
            .get_chain_spec(network)
            .unwrap();
        let proof_request = ProofRequest {
            block_number,
            network: network.to_string(),
            l1_network: l1_network.to_string(),
            graffiti: B256::ZERO,
            prover: Address::ZERO,
            proof_type: ProofType::Native,
            blob_proof_type: BlobProofType::ProofOfCommitment,
            prover_args: Default::default(),
        };
        let raiko = Raiko::new(
            l1_chain_spec.clone(),
            taiko_chain_spec.clone(),
            proof_request.clone(),
        );
        let provider = RpcBlockDataProvider::new(
            &taiko_chain_spec.rpc.clone(),
            proof_request.block_number - 1,
        )
        .expect("provider init ok");

        let input = raiko
            .generate_input(provider.clone())
            .await
            .expect("input generation failed");
        (input, provider.clone())
    }

    #[tokio::test]
    async fn test_generate_input_from_cache() {
        let l1 = &Network::Holesky.to_string();
        let l2 = &Network::TaikoA7.to_string();
        let block_number: u64 = 69627;
        let (input, provider) = create_cache_input(l1, l2, block_number).await;
        let cache_path = Some("./".into());
        assert!(set_cached_input(&cache_path, block_number, l2, &input).is_ok());
        let cached_input = get_cached_input(&cache_path, block_number, l2).expect("load cache");
        assert!(validate_cache_input(Some(cached_input), &provider)
            .await
            .is_ok());

        let new_l1 = &Network::Ethereum.to_string();
        let new_l2 = &Network::TaikoMainnet.to_string();
        let (new_input, _) = create_cache_input(new_l1, new_l2, block_number).await;
        // save to old l2 cache slot
        assert!(set_cached_input(&cache_path, block_number, l2, &new_input).is_ok());
        let inv_cached_input = get_cached_input(&cache_path, block_number, l2).expect("load cache");

        // should fail with old provider
        assert!(validate_cache_input(Some(inv_cached_input), &provider)
            .await
            .is_err());
    }
}
