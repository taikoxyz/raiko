//! Prepare Input for guest
use std::fmt::Debug;

use ethers_core::types::Transaction as EthersTransaction;
use serde::{Deserialize, Serialize};
use zeth_lib::{
    block_builder::NetworkStrategyBundle,
    consts::{ETH_MAINNET_CHAIN_SPEC, TAIKO_MAINNET_CHAIN_SPEC},
    input::Input,
    taiko::input::TaikoInput,
};

use super::{
    context::Context,
    error::Result,
    request::{ProofRequest, PseZkRequest, SgxRequest},
    utils::cache_file_path,
};

/// prepare input data for guests
pub async fn prepare_input<N: NetworkStrategyBundle>(
    ctx: &Context,
    req: &ProofRequest,
) -> Result<TaikoInput<N::TxEssence>>
where
    N::TxEssence: 'static + Send + TryFrom<EthersTransaction> + Serialize + Deserialize<'static>,
    <N::TxEssence as TryFrom<EthersTransaction>>::Error: Debug,
    <N::Database as revm::primitives::db::Database>::Error: Debug,
{
    match req {
        ProofRequest::Sgx(SgxRequest {
            block,
            l1_rpc,
            l2_rpc,
            prover,
            propose_block_tx,
        }) => {
            let l2_block = *block;
            let l2_cache_path = cache_file_path(&ctx.cache_path, l2_block, false);
            let l2_spec = TAIKO_MAINNET_CHAIN_SPEC.clone();
            let l2_rpc = l2_rpc.to_owned();

            let l1_spec = ETH_MAINNET_CHAIN_SPEC.clone();
            let l1_cache_path = cache_file_path(&ctx.cache_path, l2_block, true);
            let l1_rpc = l1_rpc.to_owned();
            let propose_block_tx = propose_block_tx.to_owned();
            let prover = prover.to_owned();
            // run sync task in blocking mode
            tokio::task::spawn_blocking(move || {
                zeth_lib::taiko::host::get_taiko_initial_data::<N>(
                    Some(l1_cache_path),
                    l1_spec,
                    Some(l1_rpc),
                    propose_block_tx,
                    prover,
                    Some(l2_cache_path),
                    l2_spec,
                    Some(l2_rpc),
                    l2_block,
                )
            })
            .await?
            .map_err(Into::into)
            .map(Into::into)
        }
        ProofRequest::PseZk(PseZkRequest { .. }) => todo!(),
    }
}
