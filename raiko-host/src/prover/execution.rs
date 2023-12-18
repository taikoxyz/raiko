use zeth_lib::taiko::block_builder::TaikoStrategyBundle;

use super::{
    context::{Context, SgxContext, GuestContext},
    error::Result,
    prepare_input::prepare_input,
    proof::{sgx::execute_sgx, powdr::execute_powdr},
    request::{ProofRequest, ProofResponse},
};

pub async fn execute(ctx: &Context, req: &ProofRequest) -> Result<ProofResponse> {
    // load input data into cache path
    let _ = prepare_input::<TaikoStrategyBundle>(ctx, req).await?;
    // run proof
    match (&ctx.guest_context, req) {
        (GuestContext::Sgx(sgx_ctx), ProofRequest::Sgx(req)) => 
            execute_sgx(&ctx.guest_path, &ctx.cache_path, sgx_ctx, req)
            .await
            .map(ProofResponse::Sgx)
            .map_err(Into::into),
        (GuestContext::Powdr(powdr_ctx), ProofRequest::Powdr(req)) => 
            execute_powdr(&ctx.guest_path, &ctx.cache_path, powdr_ctx, req)
                .await
                .map(ProofResponse::Powdr)
                .map_err(Into::into),
        (GuestContext::Sgx(sgx_ctx), ProofRequest::Sgx(req)) => todo!(),
        _ => panic!("Request for the wrong prover!")
    }
}
