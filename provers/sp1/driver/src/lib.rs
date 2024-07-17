#![cfg(feature = "enable")]
use raiko_lib::{
    input::{GuestInput, GuestOutput},
    prover::{IdStore, IdWrite, Proof, ProofKey, Prover, ProverConfig, ProverError, ProverResult},
};
use serde::{Deserialize, Serialize};
use sp1_sdk::{
    network::client::NetworkClient,
    proto::network::{ProofMode, ProofStatus, UnclaimReason},
    ProverClient, SP1Stdin,
};
use std::{env, thread::sleep, time::Duration};
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

const SP1_PROVER_CODE: u8 = 1;

impl Prover for Sp1Prover {
    async fn run(
        input: GuestInput,
        output: &GuestOutput,
        _config: &ProverConfig,
        writer: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        // Write the input.
        let mut stdin = SP1Stdin::new();
        stdin.write(&input);

        // Generate the proof for the given program.
        let client = ProverClient::new();
        let (pk, vk) = client.setup(ELF);
        let local = true;
        let proof = match local {
            true => {
                let proof = client
                    .prove(&pk, stdin)
                    .map_err(|_| ProverError::GuestError("Sp1: proving failed".to_owned()))?;
                Ok::<_, ProverError>(proof)
            }
            false => {
                let private_key = env::var("SP1_PRIVATE_KEY").map_err(|_| {
                    ProverError::GuestError(
                        "SP1_PRIVATE_KEY must be set for remote proving".to_owned(),
                    )
                })?;
                let network_client = NetworkClient::new(&private_key);
                let proof_id = network_client
                    .create_proof(&pk.elf, &stdin, ProofMode::Core, "v1.0.8-testnet")
                    .await
                    .map_err(|_| {
                        ProverError::GuestError("Sp1: creating proof failed".to_owned())
                    })?;
                if let Some(writer) = writer {
                    writer.store_id(
                        (input.chain_spec.chain_id, output.hash, SP1_PROVER_CODE),
                        proof_id.clone(),
                    )?;
                }
                let proof = {
                    let mut is_claimed = false;
                    loop {
                        let (status, maybe_proof) = network_client
                            .get_proof_status(&proof_id)
                            .await
                            .map_err(|_| {
                                ProverError::GuestError(
                                    "Sp1: getting proof status failed".to_owned(),
                                )
                            })?;

                        match status.status() {
                            ProofStatus::ProofFulfilled => {
                                break Ok(maybe_proof.unwrap());
                            }
                            ProofStatus::ProofClaimed => {
                                if !is_claimed {
                                    is_claimed = true;
                                }
                            }
                            ProofStatus::ProofUnclaimed => {
                                break Err(ProverError::GuestError(format!(
                                    "Proof generation failed: {}",
                                    status.unclaim_description()
                                )));
                            }
                            _ => {}
                        }
                        sleep(Duration::from_secs(2));
                    }
                }?;
                Ok::<_, ProverError>(proof)
            }
        }?;

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

    async fn cancel(key: ProofKey, store: Box<&mut dyn IdStore>) -> ProverResult<()> {
        let proof_id = store.read_id(key)?;
        let private_key = env::var("SP1_PRIVATE_KEY").map_err(|_| {
            ProverError::GuestError("SP1_PRIVATE_KEY must be set for remote proving".to_owned())
        })?;
        let network_client = NetworkClient::new(&private_key);
        network_client
            .unclaim_proof(proof_id, UnclaimReason::Abandoned, "".to_owned())
            .await
            .map_err(|_| ProverError::GuestError("Sp1: couldn't unclaim proof".to_owned()))?;
        store.remove_id(key)?;
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
