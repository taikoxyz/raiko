use alloy_primitives::Address;
use alloy_provider::ProviderBuilder;
use alloy_sol_types::sol;
use raiko_lib::prover::ProverResult;
use reth_primitives::B256;
use std::{env, str::FromStr};
use tracing::{error, info};
use url::Url;

use crate::RaikoProofFixture;

sol!(
    #[sol(rpc)]
    #[allow(dead_code)]
    contract ISP1Verifier {
        #[derive(Debug)]
        function verifyProof(
            bytes32 programVKey,
            bytes calldata publicValues,
            bytes calldata proofBytes
        ) external view;
    }
);

/// using pre-deployed contract to verify the proof, the only problem is to double check the verification version.
pub(crate) async fn verify_sol_by_contract_call(fixture: &RaikoProofFixture) -> ProverResult<()> {
    let sp1_verifier_rpc_url = env::var("SP1_VERIFIER_RPC_URL").expect("env SP1_VERIFIER_RPC_URL");
    let sp1_verifier_addr = {
        let addr = env::var("SP1_VERIFIER_ADDRESS").expect("env SP1_VERIFIER_RPC_URL");
        Address::from_str(&addr).unwrap()
    };

    let provider = ProviderBuilder::new().on_http(Url::parse(&sp1_verifier_rpc_url).unwrap());
    let program_key: B256 = B256::from_str(&fixture.vkey).unwrap();
    let public_value = reth_primitives::hex::decode(&fixture.public_values).unwrap();
    let proof_bytes = fixture.proof.clone();

    info!(
        "verify sp1 proof with program key: {program_key:?} public value: {public_value:?} proof: {:?}",
        reth_primitives::hex::encode(&proof_bytes)
    );

    let sp1_verifier = ISP1Verifier::new(sp1_verifier_addr, provider);
    let call_builder =
        sp1_verifier.verifyProof(program_key, public_value.into(), proof_bytes.into());
    let verify_call_res = call_builder.call().await;

    if verify_call_res.is_ok() {
        info!("SP1 proof verified successfully using {sp1_verifier_addr:?}!");
    } else {
        error!("SP1 proof verification failed: {verify_call_res:?}!");
    }

    Ok(())
}
