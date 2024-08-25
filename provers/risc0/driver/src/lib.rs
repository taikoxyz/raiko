#![cfg(feature = "enable")]

#[cfg(feature = "bonsai-auto-scaling")]
use crate::bonsai::auto_scaling::shutdown_bonsai;
use crate::methods::risc0_guest::RISC0_GUEST_ELF;
use alloy_primitives::{hex::ToHexExt, B256};
pub use bonsai::*;
use log::warn;
use raiko_lib::{
    input::{GuestInput, GuestOutput},
    prover::{IdStore, IdWrite, Proof, ProofKey, Prover, ProverConfig, ProverError, ProverResult},
};
use risc0_zkvm::serde::to_vec;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use std::fmt::Debug;
use tracing::debug;

pub mod bonsai;
pub mod methods;
pub mod snarks;

#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Risc0Param {
    pub bonsai: bool,
    pub snark: bool,
    pub profile: bool,
    pub execution_po2: u32,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Risc0Response {
    pub proof: String,
}

impl From<Risc0Response> for Proof {
    fn from(value: Risc0Response) -> Self {
        Self {
            proof: Some(value.proof),
            quote: None,
        }
    }
}

pub struct Risc0Prover;

const RISC0_PROVER_CODE: u8 = 3;

impl Prover for Risc0Prover {
    async fn run(
        input: GuestInput,
        output: &GuestOutput,
        config: &ProverConfig,
        id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let mut id_store = id_store;
        let config = Risc0Param::deserialize(config.get("risc0").unwrap()).unwrap();
        let proof_key = (
            input.chain_spec.chain_id,
            output.hash.clone(),
            RISC0_PROVER_CODE,
        );

        debug!("elf code length: {}", RISC0_GUEST_ELF.len());
        let encoded_input = to_vec(&input).expect("Could not serialize proving input!");

        let result = maybe_prove::<GuestInput, B256>(
            &config,
            encoded_input,
            RISC0_GUEST_ELF,
            &output.hash,
            Default::default(),
            proof_key,
            &mut id_store,
        )
        .await;

        let proof_gen_result = if result.is_some() {
            if config.snark && config.bonsai {
                let (stark_uuid, stark_receipt) = result.clone().unwrap();
                bonsai::bonsai_stark_to_snark(stark_uuid, stark_receipt)
                    .await
                    .map(|r0_response| r0_response.into())
                    .map_err(|e| ProverError::GuestError(e.to_string()))
            } else {
                warn!("proof is not in snark mode, please check.");
                let (_, stark_receipt) = result.clone().unwrap();

                Ok(Risc0Response {
                    proof: stark_receipt.journal.encode_hex_with_prefix(),
                }
                .into())
            }
        } else {
            Err(ProverError::GuestError(
                "Failed to generate proof".to_string(),
            ))
        };

        #[cfg(feature = "bonsai-auto-scaling")]
        if config.bonsai {
            // shutdown bonsai
            shutdown_bonsai()
                .await
                .map_err(|e| ProverError::GuestError(e.to_string()))?;
        }

        proof_gen_result
    }

    async fn cancel(key: ProofKey, id_store: Box<&mut dyn IdStore>) -> ProverResult<()> {
        let uuid = match id_store.read_id(key).await {
            Ok(uuid) => uuid,
            Err(e) => {
                if e.to_string().contains("No data for query") {
                    return Ok(());
                } else {
                    return Err(ProverError::GuestError(e.to_string()));
                }
            }
        };
        cancel_proof(uuid)
            .await
            .map_err(|e| ProverError::GuestError(e.to_string()))?;
        id_store.remove_id(key).await
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use methods::test_risc0_guest::{TEST_RISC0_GUEST_ELF, TEST_RISC0_GUEST_ID};
    use risc0_zkvm::{default_prover, ExecutorEnv};

    #[test]
    fn run_unittest_elf() {
        std::env::set_var("RISC0_PROVER", "local");
        let env = ExecutorEnv::builder().build().unwrap();
        let prover = default_prover();
        let receipt = prover.prove(env, TEST_RISC0_GUEST_ELF).unwrap();
        receipt.receipt.verify(TEST_RISC0_GUEST_ID).unwrap();
    }
}
