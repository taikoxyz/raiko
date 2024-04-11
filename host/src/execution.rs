use alloy_consensus::Sealable;
use alloy_primitives::B256;
use raiko_lib::{
    builder::{BlockBuilderStrategy, TaikoStrategy},
    input::{GuestInput, GuestOutput, TaikoProverData, WrappedHeader},
    protocol_instance::{assemble_protocol_instance, ProtocolInstance},
    prover::{to_proof, Proof, Prover, ProverResult},
    Measurement,
};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::{
    error::HostResult,
    memory,
    metrics::{inc_guest_req_count, observe_guest_time, observe_prepare_input_time},
    preflight::preflight,
    request::ProofRequest,
};

/// Execute the proof generation.
pub async fn execute(
    proof_request: &ProofRequest,
    cached_input: Option<GuestInput>,
) -> HostResult<(GuestInput, Proof)> {
    // 1. Prepare input - use cached input if available, otherwise prepare new input
    let input = if let Some(cached_input) = cached_input {
        println!("Using cached input");
        cached_input
    } else {
        memory::reset_stats();
        let measurement = Measurement::start("Generating input...", false);
        let input = prepare_input(proof_request.clone()).await;
        let input_time = measurement.stop_with("=> Input generated");
        observe_prepare_input_time(
            proof_request.block_number,
            input_time.as_millis(),
            input.is_ok(),
        );
        memory::print_stats("Input generation peak memory used: ");
        input?
    };

    // 2. Test run the block
    memory::reset_stats();
    let build_result = TaikoStrategy::build_from(&input);
    let output = match &build_result {
        Ok((header, _mpt_node)) => {
            info!("Verifying final state using provider data ...");
            info!("Final block hash derived successfully. {}", header.hash());
            info!("Final block header derived successfully. {header:?}");
            let pi = proof_request
                .proof_type
                .instance_hash(assemble_protocol_instance(&input, header)?)?;
            // Make sure the blockhash from the node matches the one from the builder
            assert_eq!(header.hash().0, input.block_hash, "block hash unexpected");
            GuestOutput::Success((
                WrappedHeader {
                    header: header.clone(),
                },
                pi,
            ))
        }
        Err(_) => {
            warn!("Proving bad block construction!");
            GuestOutput::Failure
        }
    };
    memory::print_stats("Guest program peak memory used: ");

    // 3. Prove
    memory::reset_stats();
    let measurement = Measurement::start("Generating proof...", false);
    inc_guest_req_count(&proof_request.proof_type, proof_request.block_number);
    let res = proof_request
        .proof_type
        .run_prover(input.clone(), output, &serde_json::to_value(proof_request)?)
        .await
        .map(|proof| (input, proof));
    let guest_time = measurement.stop_with("=> Proof generated");
    observe_guest_time(
        &proof_request.proof_type,
        proof_request.block_number,
        guest_time.as_millis(),
        res.is_ok(),
    );
    memory::print_stats("Prover peak memory used: ");

    res
}

/// prepare input data for provers
pub async fn prepare_input(
    ProofRequest {
        block_number,
        rpc,
        l1_rpc,
        beacon_rpc,
        network,
        graffiti,
        prover,
        ..
    }: ProofRequest,
) -> HostResult<GuestInput> {
    tokio::task::spawn_blocking(move || {
        preflight(
            Some(rpc),
            block_number,
            network,
            TaikoProverData { graffiti, prover },
            Some(l1_rpc),
            Some(beacon_rpc),
        )
        .expect("Failed to fetch required data for block")
    })
    .await
    .map_err(|e| e.into())
}

pub struct NativeProver;

#[derive(Clone, Serialize, Deserialize)]
pub struct NativeResponse {
    output: GuestOutput,
}

impl Prover for NativeProver {
    async fn run(
        _input: GuestInput,
        output: GuestOutput,
        _request: &serde_json::Value,
    ) -> ProverResult<Proof> {
        to_proof(Ok(NativeResponse { output }))
    }

    fn instance_hash(_pi: ProtocolInstance) -> B256 {
        B256::default()
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_async_block() {
        let result = async { Result::<(), &'static str>::Err("error") };
        println!("must here");
        assert!(result.await.is_err());
    }
}
