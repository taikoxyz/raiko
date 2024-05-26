#![cfg(feature = "enable")]
use std::fmt::Debug;

use alloy_primitives::B256;
use alloy_sol_types::SolValue;

use hex::ToHex;

use raiko_lib::{
    input::{GuestInput, GuestOutput},
    protocol_instance::ProtocolInstance,
    prover::{to_proof, Proof, Prover, ProverConfig, ProverResult},
};
use raiko_primitives::keccak::keccak;
use risc0_zkvm::{serde::to_vec, sha::Digest};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use tracing::info as traicing_info;

pub mod bonsai;
pub mod methods;
pub mod snarks;
use crate::snarks::verify_groth16_snark;
use bonsai::maybe_prove;
pub use bonsai::*;
use methods::risc0_guest::{RISC0_GUEST_ELF, RISC0_GUEST_ID};

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
pub struct Risc0Prover;

impl Prover for Risc0Prover {
    async fn run(
        input: GuestInput,
        output: &GuestOutput,
        config: &ProverConfig,
    ) -> ProverResult<Proof> {
        let config = Risc0Param::deserialize(config.get("risc0").unwrap()).unwrap();

        println!("elf code length: {}", RISC0_GUEST_ELF.len());
        let encoded_input = to_vec(&input).expect("Could not serialize proving input!");

        let result = maybe_prove::<GuestInput, GuestOutput>(
            &config,
            encoded_input,
            RISC0_GUEST_ELF,
            output,
            Default::default(),
        )
        .await;

        let journal: String = result.clone().unwrap().1.journal.encode_hex();

        // Create/verify Groth16 SNARK
        if config.snark {
            let Some((stark_uuid, stark_receipt)) = result else {
                panic!("No STARK data to snarkify!");
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

        to_proof(Ok(Risc0Response { proof: journal }))
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
        receipt.verify(TEST_RISC0_GUEST_ID).unwrap();
    }
}
