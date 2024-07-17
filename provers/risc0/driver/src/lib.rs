#![cfg(feature = "enable")]

use alloy_primitives::B256;
use hex::ToHex;
use raiko_lib::{
    input::{GuestInput, GuestOutput},
    prover::{IdStore, IdWrite, Proof, ProofKey, Prover, ProverConfig, ProverError, ProverResult},
};
use risc0_zkvm::{serde::to_vec, sha::Digest};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use std::fmt::Debug;
use tracing::{debug, info as traicing_info};

use crate::{
    methods::risc0_guest::{RISC0_GUEST_ELF, RISC0_GUEST_ID},
    snarks::verify_groth16_snark,
};

pub use bonsai::*;

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
            kzg_proof: None,
        }
    }
}

pub struct Risc0Prover;

impl Prover for Risc0Prover {
    async fn run(
        input: GuestInput,
        output: &GuestOutput,
        config: &ProverConfig,
        _write: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let config = Risc0Param::deserialize(config.get("risc0").unwrap()).unwrap();

        debug!("elf code length: {}", RISC0_GUEST_ELF.len());
        let encoded_input = to_vec(&input).expect("Could not serialize proving input!");

        let result = maybe_prove::<GuestInput, B256>(
            &config,
            encoded_input,
            RISC0_GUEST_ELF,
            &output.hash,
            Default::default(),
        )
        .await;

        let journal: String = result.clone().unwrap().1.journal.encode_hex();

        // Create/verify Groth16 SNARK
        if config.snark {
            let Some((stark_uuid, stark_receipt)) = result else {
                return Err(ProverError::GuestError(
                    "No STARK data to snarkify!".to_owned(),
                ));
            };
            let image_id = Digest::from(RISC0_GUEST_ID);
            let (snark_uuid, snark_receipt) =
                snarks::stark2snark(image_id, stark_uuid, stark_receipt)
                    .await
                    .map_err(|err| format!("Failed to convert STARK to SNARK: {err:?}"))?;

            traicing_info!("Validating SNARK uuid: {snark_uuid}");

            verify_groth16_snark(image_id, snark_receipt)
                .await
                .map_err(|err| format!("Failed to verify SNARK: {err:?}"))?;
        }

        Ok(Risc0Response { proof: journal }.into())
    }

    async fn cancel(_key: ProofKey, _store: Box<&mut dyn IdStore>) -> ProverResult<()> {
        Ok(())
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
