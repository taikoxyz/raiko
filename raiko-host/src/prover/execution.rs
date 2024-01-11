use zeth_lib::taiko::block_builder::TaikoStrategyBundle;

use super::{
    context::Context,
    error::Result,
    prepare_input::prepare_input,
    proof::{cache::Cache, sgx::execute_sgx},
    request::{ProofRequest, ProofResponse},
};
// use crate::rolling::prune_old_caches;

pub async fn execute(_cache: &Cache, ctx: &Context, req: &ProofRequest) -> Result<ProofResponse> {
    // 1. load input data into cache path
    let _ = prepare_input::<TaikoStrategyBundle>(ctx, req).await?;
    // 2. run proof
    // prune_old_caches(&ctx.cache_path, ctx.max_caches);
    match req {
        ProofRequest::Sgx(req) => {
            let resp = execute_sgx(ctx, req).await?;
            Ok(ProofResponse::Sgx(resp))
        }
        ProofRequest::PseZk(_) => todo!(),
    }
}
