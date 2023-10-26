//! Prepare Input for guest
use std::fmt::Debug;

use ethers_core::types::Transaction as EthersTransaction;
use serde::{Deserialize, Serialize};
use zeth_lib::{
    block_builder::NetworkStrategyBundle, consts::TAIKO_MAINNET_CHAIN_SPEC, input::Input,
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
) -> Result<Input<N::TxEssence>>
where
    N::TxEssence: 'static + Send + TryFrom<EthersTransaction> + Serialize + Deserialize<'static>,
    <N::TxEssence as TryFrom<EthersTransaction>>::Error: Debug,
    <N::Database as revm::primitives::db::Database>::Error: Debug,
{
    match req {
        ProofRequest::Sgx(SgxRequest {
            l1_rpc: _,
            proposer_hash: _,
            l2_block,
            l2_rpc,
            protocol_instance,
            no_sgx: _,
        }) => {
            let l2_block = *l2_block;
            let cache_path = cache_file_path(&ctx.cache_path, l2_block);
            let init_spec = TAIKO_MAINNET_CHAIN_SPEC.clone();
            let _protocol_instance = protocol_instance.clone();
            let l2_rpc = l2_rpc.to_owned();
            // run sync task in blocking mode
            tokio::task::spawn_blocking(move || {
                zeth_lib::host::get_initial_data::<N>(
                    init_spec,
                    Some(cache_path),
                    Some(l2_rpc),
                    l2_block,
                    Some(_protocol_instance),
                )
            })
            .await?
            .map_err(Into::into)
            .map(Into::into)
        }
        ProofRequest::PseZk(PseZkRequest { .. }) => todo!(),
    }
}
