//! Prepare Input for guest
use std::{fmt::Debug, path::PathBuf};

use zeth_lib::{
    block_builder::NetworkStrategyBundle,
    consts::{ETH_MAINNET_CHAIN_SPEC, TAIKO_MAINNET_CHAIN_SPEC},
    host::Init,
    taiko::host::TaikoExtra,
    EthereumTxEssence,
};

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
) -> Result<(Init<N::TxEssence>, TaikoExtra)>
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
            let graffiti = *graffiti;
            // run sync task in blocking mode
            tokio::task::spawn_blocking(move || {
                zeth_lib::taiko::host::get_taiko_initial_data::<(Option<PathBuf>,Option<String>),N>(
                    (Some(l1_cache_path), Some(l1_rpc)),
                    (Some(l2_cache_path), Some(l2_rpc)),
                    l1_spec,
                    prover,
                    l2_spec,
                    l2_block,
                    graffiti,
                )
            })
            .await?
            .map_err(Into::into)
        }
        ProofRequest::PseZk(PseZkRequest { .. }) => todo!(),
    }
}
