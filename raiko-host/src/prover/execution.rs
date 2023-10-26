use super::{
    context::Context,
    error::Result,
    prepare_input::prepare_input,
    proof::sgx::execute_sgx,
    request::{ProofRequest, ProofResponse},
};
use zeth_lib::block_builder::TaikoStrategyBundle;

pub async fn execute(ctx: &Context, req: &ProofRequest) -> Result<ProofResponse> {
    // load input data into cache path
    let _ = prepare_input::<TaikoStrategyBundle>(ctx, req).await?;
    // run proof
    match req {
        ProofRequest::Sgx(req) => execute_sgx(ctx, req)
            .await
            .map(ProofResponse::Sgx)
            .map_err(Into::into),
        ProofRequest::PseZk(_) => todo!(),
    }
}
