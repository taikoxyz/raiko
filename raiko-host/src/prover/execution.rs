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

pub async fn execute(cache: &Cache, ctx: &Context, req: &ProofRequest) -> Result<ProofResponse> {
    match req {
        ProofRequest::Sgx(req) => {
            // fetching data in the sgx guest
            let cache_key = CacheKey {
                proof_type: ProofType::Sgx,
                block: req.block,
            };
            let cached = cache.get(&cache_key);
            if let Some(proof) = cached {
                return Ok(ProofResponse::Sgx(SgxResponse { proof }));
            }
            let resp = execute_sgx(ctx, req).await?;
            cache.set(cache_key, resp.proof.clone());
            Ok(ProofResponse::Sgx(resp))
        }
        ProofRequest::PseZk(_) => {
            // 1. load input data into cache path
            let _ = prepare_input::<TaikoStrategyBundle>(ctx, req).await?;
            // 2. run proof
            unimplemented!("PseZk proof is not implemented yet")
        }
    }
}
