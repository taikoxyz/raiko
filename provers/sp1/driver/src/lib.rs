#![cfg(feature = "enable")]
use raiko_lib::{
    input::{GuestInput, GuestOutput},
    prover::{IdStore, IdWrite, Proof, Prover, ProverConfig, ProverError, ProverResult},
};
use serde::{Deserialize, Serialize};
use sp1_sdk::{ProverClient, SP1Stdin};
use std::env;
use tracing::info as tracing_info;

const ELF: &[u8] = include_bytes!("../../guest/elf/sp1-guest");

#[derive(Clone, Serialize, Deserialize)]
pub struct Sp1Response {
    pub proof: String,
}

impl From<Sp1Response> for Proof {
    fn from(value: Sp1Response) -> Self {
        Self {
            proof: Some(value.proof),
            quote: None,
            kzg_proof: None,
        }
    }
}

pub struct Sp1Prover;

impl Prover for Sp1Prover {
    async fn run(
        input: GuestInput,
        _output: &GuestOutput,
        _config: &ProverConfig,
        write: &mut dyn IdWrite,
    ) -> ProverResult<Proof> {
        // Write the input.
        let mut stdin = SP1Stdin::new();
        stdin.write(&input);

        // Generate the proof for the given program.
        let client = ProverClient::new();
        let (pk, vk) = client.setup(ELF);
        let proof = client
            .prove(&pk, stdin)
            .map_err(|_| ProverError::GuestError("Sp1: proving failed".to_owned()))?;

        // Verify proof.
        client
            .verify(&proof, &vk)
            .map_err(|_| ProverError::GuestError("Sp1: verification failed".to_owned()))?;

        // Save the proof.
        let proof_dir =
            env::current_dir().map_err(|_| ProverError::GuestError("Sp1: dir error".to_owned()))?;
        proof
            .save(
                proof_dir
                    .as_path()
                    .join("proof-with-io.json")
                    .to_str()
                    .unwrap(),
            )
            .map_err(|_| ProverError::GuestError("Sp1: saving proof failed".to_owned()))?;

        tracing_info!("successfully generated and verified proof for the program!");
        Ok(Sp1Response {
            proof: serde_json::to_string(&proof).unwrap(),
        }
        .into())
    }

    async fn cancel(_key: &str, _store: &mut dyn IdStore) -> ProverResult<()> {
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    const TEST_ELF: &[u8] = include_bytes!("../../guest/elf/test-sp1-guest");

    #[test]
    fn run_unittest_elf() {
        // TODO(Cecilia): imple GuestInput::mock() for unit test
        let client = ProverClient::new();
        let stdin = SP1Stdin::new();
        let (pk, vk) = client.setup(TEST_ELF);
        let proof = client.prove(&pk, stdin).expect("Sp1: proving failed");
        client
            .verify(&proof, &vk)
            .expect("Sp1: verification failed");
    }
}
