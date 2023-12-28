use std::time::Instant;

use zeth_lib::taiko::block_builder::TaikoStrategyBundle;

use super::{
    context::Context,
    error::Result,
    prepare_input::prepare_input,
    proof::{
        cache::{Cache, CacheKey},
        sgx::execute_sgx,
        ProofType,
    },
    request::{ProofRequest, ProofResponse, SgxResponse},
};
use crate::metrics::{inc_sgx_success, observe_sgx_gen};

pub async fn execute(cache: &Cache, ctx: &Context, req: &ProofRequest) -> Result<ProofResponse> {
    // 1. load input data into cache path
    let _ = prepare_input::<TaikoStrategyBundle>(ctx, req).await?;
    // 2. run proof
    match req {
        ProofRequest::Sgx(req) => {
            let start = Instant::now();
            let bid = req.block.clone();
            let cache_key = CacheKey {
                proof_type: ProofType::Sgx,
                block: req.block,
                prover: req.prover,
                graffiti: req.graffiti,
            };
            let cached = cache.get(&cache_key);
            if let Some(proof) = cached {
                return Ok(ProofResponse::Sgx(SgxResponse { proof }));
            }
            let resp = execute_sgx(ctx, req).await?;
            cache.set(cache_key, resp.proof.clone());
            let time_elapsed = Instant::now().duration_since(start).as_millis() as i64;
            observe_sgx_gen(bid, time_elapsed);
            inc_sgx_success(bid);
            Ok(ProofResponse::Sgx(resp))
        }
        ProofRequest::PseZk(_) => todo!(),
    }
}
