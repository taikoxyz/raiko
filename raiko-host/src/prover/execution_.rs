use std::{fmt::Debug, str::FromStr, time::Instant};

use reth_primitives::B256;
use tracing::{info, warn};
use zeth_lib::{
    builder::{BlockBuilderStrategy, TaikoStrategy},
    consts::Network,
    input::{GuestInput, GuestOutput, TaikoProverData},
    protocol_instance::{assemble_protocol_instance, EvidenceType, ProtocolInstance},
    taiko_utils::HeaderHasher,
};
use zeth_primitives::Address;

use super::{
    context::Context,
    error::Result,
    proof::{
        cache::Cache, powdr::execute_powdr, risc0::execute_risc0, sgx::execute_sgx,
        succinct::execute_sp1,
    },
    request::{ProofRequest_, ProofType},
};
use crate::{
    host::host::preflight,
    metrics::{inc_sgx_success, observe_input, observe_sgx_gen},
};


pub trait GuestDriver {
    type ProofParam;
    type ProofResponse;

    fn new() -> Self;

    fn run(
        input: GuestInput,
        output: GuestOutput,
        param: Self::ProofParam,
    ) -> Result<Self::ProofResponse>;

    fn instance_hash(pi: ProtocolInstance) -> B256;
}

pub async fn execute<D: GuestDriver>(
    _cache: &Cache,
    ctx: &mut Context,
    req: &ProofRequest_<D::ProofParam>,
) -> Result<D::ProofResponse>
    where 
        D::ProofParam: Debug + Clone
 {
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
        Ok((header, mpt_node)) => {
            info!("Verifying final state using provider data ...");
            info!("Final block hash derived successfully. {}", header.hash());
            info!("Final block header derived successfully. {:?}", header);
            let pi = D::instance_hash(assemble_protocol_instance(&input, &header)?);
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
    
    D::run(input, output, req.proof_param.clone())
}

/// prepare input data for guests
pub async fn prepare_input<P>(
    ctx: &mut Context, 
    req: &ProofRequest_<P>
) -> Result<GuestInput> {
    // Todo(Cecilia): should contract address as args, curently hardcode
    let l1_cache = ctx.l1_cache_file.clone();
    let l2_cache = ctx.l2_cache_file.clone();
    let block_number = req.block_number;
    let l1_rpc = req.l1_rpc.clone();
    let l2_rpc = req.l2_rpc.clone();
    let beacon_rpc = req.beacon_rpc.clone();
    let chain = req.chain.clone();
    let graffiti = req.graffiti.clone();
    let prover = req.prover.clone();
    tokio::task::spawn_blocking(move || {
        preflight(
            Some(l1_rpc),
            Some(l2_rpc),
            block_number,
            Network::from_str(&chain).unwrap(),
            TaikoProverData {
                graffiti,
                prover,
            },
            Some(beacon_rpc),
        )
        .expect("Failed to fetch required data for block")
    })
    .await
    .map_err(Into::<super::error::Error>::into)
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
