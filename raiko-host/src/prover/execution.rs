use std::{fmt::Debug, str::FromStr, time::Instant};

use alloy_sol_types::SolValue;
use reth_primitives::B256;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use zeth_lib::{
    builder::{BlockBuilderStrategy, TaikoStrategy},
    consts::Network,
    input::{GuestInput, GuestOutput, TaikoProverData},
    protocol_instance::{assemble_protocol_instance, ProtocolInstance},
    taiko_utils::HeaderHasher,
};
use zeth_primitives::keccak::keccak;

use super::{context::Context, error::Result, request::ProofRequest};
use crate::{host::host::preflight, metrics::observe_input};

pub trait GuestDriver {
    type ProofParam: Debug + Clone;
    type ProofResponse: Serialize;

    async fn run(
        input: GuestInput,
        output: GuestOutput,
        param: Self::ProofParam,
    ) -> Result<Self::ProofResponse>;

    fn instance_hash(pi: ProtocolInstance) -> B256;
}

pub async fn execute<D: GuestDriver>(
    ctx: &mut Context,
    req: &ProofRequest<D::ProofParam>,
) -> Result<D::ProofResponse> {
    println!("- {:?}", req);
    // 1. load input data into cache path
    let start = Instant::now();
    let input = prepare_input(ctx, req).await?;
    let elapsed = Instant::now().duration_since(start).as_millis() as i64;
    observe_input(elapsed);
    // 2. pre-build the block
    let build_result = TaikoStrategy::build_from(&input);
    // TODO: cherry-pick risc0 latest output
    let output = match &build_result {
        Ok((header, _mpt_node)) => {
            info!("Verifying final state using provider data ...");
            info!("Final block hash derived successfully. {}", header.hash());
            info!("Final block header derived successfully. {:?}", header);
            let pi = D::instance_hash(assemble_protocol_instance(&input, header)?);
            // Make sure the blockhash from the node matches the one from the builder
            assert_eq!(header.hash().0, input.block_hash, "block hash unexpected");
            GuestOutput::Success((header.clone(), pi))
        }
        Err(_) => {
            warn!("Proving bad block construction!");
            GuestOutput::Failure
        }
    };
    let elapsed = Instant::now().duration_since(start).as_millis() as i64;
    observe_input(elapsed);

    D::run(input, output, req.proof_param.clone()).await
}

/// prepare input data for guests
pub async fn prepare_input<P>(ctx: &mut Context, req: &ProofRequest<P>) -> Result<GuestInput> {
    // Todo(Cecilia): should contract address as args, curently hardcode
    let _l1_cache = ctx.l1_cache_file.clone();
    let _l2_cache = ctx.l2_cache_file.clone();
    let block_number = req.block_number;
    let l1_rpc = req.l1_rpc.clone();
    let l2_rpc = req.l2_rpc.clone();
    let beacon_rpc = req.beacon_rpc.clone();
    let chain = req.chain.clone();
    let graffiti = req.graffiti;
    let prover = req.prover;
    tokio::task::spawn_blocking(move || {
        preflight(
            Some(l1_rpc),
            Some(l2_rpc),
            block_number,
            Network::from_str(&chain).unwrap(),
            TaikoProverData { graffiti, prover },
            Some(beacon_rpc),
        )
        .expect("Failed to fetch required data for block")
    })
    .await
    .map_err(Into::<super::error::HostError>::into)
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

pub struct NativeDriver;

#[derive(Clone, Serialize, Deserialize)]
pub struct NativeResponse {
    output: GuestOutput,
}

impl GuestDriver for NativeDriver {
    type ProofParam = ();
    type ProofResponse = NativeResponse;

    async fn run(
        _input: GuestInput,
        output: GuestOutput,
        _param: Self::ProofParam,
    ) -> Result<Self::ProofResponse> {
        Ok(NativeResponse { output })
    }

    fn instance_hash(_pi: ProtocolInstance) -> B256 {
        B256::default()
    }
}

cfg_if::cfg_if! {
    if #[cfg(feature = "succinct")] {

        pub struct Sp1Driver;

        impl GuestDriver for Sp1Driver {
            type ProofParam = ();
            type ProofResponse = sp1_guest::Sp1Response;

            async fn run(
                input: GuestInput,
                _output: GuestOutput,
                _param: Self::ProofParam,
            ) -> Result<Self::ProofResponse> {
                let res = sp1_guest::execute(input).await?;
                Ok(res)
            }

            fn instance_hash(pi: ProtocolInstance) -> B256 {
                let data = (
                    pi.transition.clone(),
                    pi.prover,
                    pi.meta_hash()
                ).abi_encode();

                keccak(data).into()
            }
        }
    } else if #[cfg(feature = "risc0")] {

        pub struct Risc0Driver;

        impl GuestDriver for Risc0Driver {
            type ProofParam = risc0_guest::Risc0ProofParams;
            type ProofResponse = risc0_guest::Risc0Response;

            async fn run(
                input: GuestInput,
                output: GuestOutput,
                param: Self::ProofParam,
            ) -> Result<Self::ProofResponse> {
                let res = risc0_guest::execute(input, output, &param).await?;
                Ok(res)
            }

            fn instance_hash(pi: ProtocolInstance) -> B256 {
                let data = (
                    pi.transition.clone(),
                    pi.prover,
                    pi.meta_hash()
                ).abi_encode();

                keccak(data).into()
            }
        }
    }
}
