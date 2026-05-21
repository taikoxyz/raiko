use alloy_primitives::Address;
use alloy_provider::ProviderBuilder;
use alloy_sol_types::sol;
use raiko_lib::prover::{ProverError, ProverResult};
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
    let sp1_verifier_rpc_url = env::var("SP1_VERIFIER_RPC_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            ProverError::GuestError(
                "Sp1: SP1_VERIFIER_RPC_URL must be set when sp1.verify=true".to_string(),
            )
        })?;
    let sp1_verifier_addr = env::var("SP1_VERIFIER_ADDRESS")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            ProverError::GuestError(
                "Sp1: SP1_VERIFIER_ADDRESS must be set when sp1.verify=true".to_string(),
            )
        })
        .and_then(|addr| {
            Address::from_str(&addr).map_err(|e| {
                ProverError::GuestError(format!("Sp1: invalid SP1_VERIFIER_ADDRESS: {e}"))
            })
        })?;

    let provider =
        ProviderBuilder::new().on_http(Url::parse(&sp1_verifier_rpc_url).map_err(|e| {
            ProverError::GuestError(format!("Sp1: invalid SP1_VERIFIER_RPC_URL: {e}"))
        })?);
    let program_key: B256 = B256::from_str(&fixture.vkey)
        .map_err(|e| ProverError::GuestError(format!("Sp1: invalid verifier key: {e}")))?;
    let public_value = reth_primitives::hex::decode(&fixture.public_values)
        .map_err(|e| ProverError::GuestError(format!("Sp1: invalid public values: {e}")))?;
    let proof_bytes = fixture.proof.clone();

    info!(
        "verify sp1 proof with program key: {program_key:?} public value: {public_value:?} proof bytes: {}",
        proof_bytes.len()
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
