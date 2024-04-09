use alloy_primitives::B256;
use raiko_lib::{
    input::{GuestInput, GuestOutput, TaikoProverData},
    protocol_instance::ProtocolInstance,
    prover::{to_proof, Proof, Prover, ProverResult},
};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use super::error::Result;
use crate::{error::HostError, memory, preflight::preflight, request::ProofRequest};

pub async fn execute<D: Prover>(
    config: &serde_json::Value,
    cached_input: Option<GuestInput>,
) -> Result<(GuestInput, Proof)> {
    let total_proving_time = Measurement::start("", false);

    // Generate the input
    let input = if let Some(cached_input) = cached_input {
        println!("Using cached input");
        cached_input
    } else {
        memory::reset_stats();
        let measurement = Measurement::start("Generating input...", false);
        let input = prepare_input(config).await?;
        measurement.stop_with("=> Input generated");
        memory::print_stats("Input generation peak memory used: ");
        input
    };

    // 2. Test run the block
    memory::reset_stats();
    match TaikoStrategy::build_from(&input) {
        Ok((header, _mpt_node)) => {
            info!("Verifying final state using provider data ...");
            info!("Final block hash derived successfully. {}", header.hash());
            info!("Final block header derived successfully. {:?}", header);
            let pi = D::instance_hash(assemble_protocol_instance(&input, &header)?);
            // Make sure the blockhash from the node matches the one from the builder
            assert_eq!(header.hash().0, input.block_hash, "block hash unexpected");
            let output = GuestOutput::Success((
                WrappedHeader {
                    header: header.clone(),
                },
                pi,
            ));
            memory::print_stats("Guest program peak memory used: ");

            // Prove
            memory::reset_stats();
            let measurement = Measurement::start("Generating proof...", false);
            let res = D::run(input.clone(), output, config)
                .await
                .map(|proof| (input, proof))
                .map_err(|e| HostError::GuestError(e.to_string()))?;

            measurement.stop_with("=> Proof generated");
            memory::print_stats("Prover peak memory used: ");

            total_proving_time.stop_with("====> Complete proof generated");

            Ok(res)
        }
        Err(e) => {
            warn!("Proving bad block construction!");
            Err(HostError::GuestError(e.to_string()))
        }
    }
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

pub struct NativeDriver;

#[derive(Clone, Serialize, Deserialize)]
pub struct NativeResponse {
    output: GuestOutput,
}

impl Prover for NativeDriver {
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
