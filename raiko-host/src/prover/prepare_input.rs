//! Prepare Input for guest
use std::fmt::Debug;

use zeth_lib::{
    block_builder::NetworkStrategyBundle,
    consts::{ETH_MAINNET_CHAIN_SPEC, TAIKO_MAINNET_CHAIN_SPEC},
    taiko::input::TaikoInput,
    EthereumTxEssence,
};
use zeth_primitives::taiko::string_to_bytes32;

use super::{
    context::Context,
    error::Result,
    request::{ProofRequest, PseZkRequest, SgxRequest},
    utils::cache_file_path,
};

/// prepare input data for guests
pub async fn prepare_input<N: NetworkStrategyBundle<TxEssence = EthereumTxEssence>>(
    ctx: &Context,
    req: &ProofRequest,
) -> Result<TaikoInput<N::TxEssence>>
where
    <N::Database as revm::primitives::db::Database>::Error: Debug,
{
    match req {
        ProofRequest::Sgx(SgxRequest {
            block,
            l1_rpc,
            l2_rpc,
            prover,
            graffiti,
        }) => {
            let l2_block = *block;
            let l2_cache_path = cache_file_path(&ctx.cache_path, l2_block, false);
            let l2_spec = TAIKO_MAINNET_CHAIN_SPEC.clone();
            let l2_rpc = l2_rpc.to_owned();

            let l1_spec = ETH_MAINNET_CHAIN_SPEC.clone();
            let l1_cache_path = cache_file_path(&ctx.cache_path, l2_block, true);
            let l1_rpc = l1_rpc.to_owned();
            let prover = prover.to_owned();
            let graffiti = string_to_bytes32(graffiti.as_bytes());
            // run sync task in blocking mode
            tokio::task::spawn_blocking(move || {
                zeth_lib::taiko::host::get_taiko_initial_data::<N>(
                    Some(l1_cache_path),
                    l1_spec,
                    Some(l1_rpc),
                    prover,
                    Some(l2_cache_path),
                    l2_spec,
                    Some(l2_rpc),
                    l2_block,
                    graffiti.into(),
                )
            })
            .await?
            .map_err(Into::into)
            .map(Into::into)
        }
        ProofRequest::PseZk(PseZkRequest { .. }) => todo!(),
    }
}
