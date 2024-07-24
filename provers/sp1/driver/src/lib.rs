#![cfg(feature = "enable")]

use std::path::PathBuf;

use once_cell::sync::Lazy;
use raiko_lib::{
    input::{GuestInput, GuestOutput},
    prover::{IdStore, IdWrite, Proof, ProofKey, Prover, ProverConfig, ProverError, ProverResult},
};
use reth_primitives::B256;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use sp1_sdk::{
    network::client::NetworkClient,
    proto::network::{ProofMode, ProofStatus, UnclaimReason},
};
use sp1_sdk::{HashableKey, ProverClient, SP1PlonkBn254Proof, SP1Stdin, SP1VerifyingKey};
use std::{env, thread::sleep, time::Duration};

pub const ELF: &[u8] = include_bytes!("../../guest/elf/sp1-guest");
pub const FIXTURE_PATH: &str = "./provers/sp1/contracts/src/fixtures/";
pub const CONTRACT_PATH: &str = "./provers/sp1/contracts/src";
const SP1_PROVER_CODE: u8 = 1;

pub static VERIFIER: Lazy<Result<PathBuf, ProverError>> = Lazy::new(init_verifier);
#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sp1Param {
    pub recursion: RecursionMode,
    pub prover: ProverMode,
    pub verify: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecursionMode {
    /// The proof mode for an SP1 core proof.
    Core,
    /// The proof mode for a compressed proof.
    Compressed,
    /// The proof mode for a PlonK proof.
    Plonk,
}

impl From<RecursionMode> for ProofMode {
    fn from(value: RecursionMode) -> Self {
        match value {
            RecursionMode::Core => ProofMode::Core,
            RecursionMode::Compressed => ProofMode::Compressed,
            RecursionMode::Plonk => ProofMode::Plonk,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProverMode {
    Mock,
    Local,
    Network,
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

#[derive(Clone, Serialize, Deserialize)]
pub struct Sp1Response {
    pub proof: String,
}

pub struct Sp1Prover;

impl Prover for Sp1Prover {
    async fn run(
        input: GuestInput,
        output: &GuestOutput,
        config: &ProverConfig,
        id_store: Option<&mut dyn IdWrite>,
    ) -> ProverResult<Proof> {
        let param = Sp1Param::deserialize(config.get("sp1").unwrap()).unwrap();

        let mut stdin = SP1Stdin::new();
        stdin.write(&input);

        // Generate the proof for the given program.
        let client = match param.prover {
            ProverMode::Mock => ProverClient::mock(),
            ProverMode::Local => ProverClient::local(),
            ProverMode::Network => ProverClient::network(),
        };
        let (pk, _vk) = client.setup(ELF);

        if !matches!(param.prover, ProverMode::Network) {
            match param.recursion {
                RecursionMode::Core => client.prove(&pk, stdin).map(|p| serde_json::to_string(&p)),
                RecursionMode::Compressed => client
                    .prove_compressed(&pk, stdin)
                    .map(|p| serde_json::to_string(&p)),
                RecursionMode::Plonk => client
                    .prove_plonk(&pk, stdin)
                    .map(|p| serde_json::to_string(&p)),
            }
            .map(|s| Proof {
                proof: s.ok(),
                quote: None,
                kzg_proof: None,
            })
            .map_err(|e| ProverError::GuestError(format!("Sp1: proving failed: {}", e)))
        } else {
            let private_key = env::var("SP1_PRIVATE_KEY").map_err(|_| {
                ProverError::GuestError("SP1_PRIVATE_KEY must be set for remote proving".to_owned())
            })?;
            let network_client = NetworkClient::new(&private_key);

            let proof_id = network_client
                .create_proof(
                    &pk.elf,
                    &stdin,
                    param.recursion.clone().into(),
                    "v1.0.8-testnet",
                )
                .await
                .map_err(|_| ProverError::GuestError("Sp1: creating proof failed".to_owned()))?;

            if let Some(id_store) = id_store {
                id_store.store_id(
                    (input.chain_spec.chain_id, output.hash, SP1_PROVER_CODE),
                    proof_id.clone(),
                ).await?;
            }
            let proof = {
                let mut is_claimed = false;
                loop {
                    let (status, maybe_proof) = match param.recursion {
                        RecursionMode::Core => network_client
                            .get_proof_status::<sp1_sdk::SP1Proof>(&proof_id)
                            .await
                            .map(|(s, p)| (s, p.and_then(|p| serde_json::to_string(&p).ok()))),
                        RecursionMode::Compressed => network_client
                            .get_proof_status::<sp1_sdk::SP1CompressedProof>(&proof_id)
                            .await
                            .map(|(s, p)| (s, p.and_then(|p| serde_json::to_string(&p).ok()))),
                        RecursionMode::Plonk => network_client
                            .get_proof_status::<sp1_sdk::SP1PlonkBn254Proof>(&proof_id)
                            .await
                            .map(|(s, p)| (s, p.and_then(|p| serde_json::to_string(&p).ok()))),
                    }
                    .map_err(|_| {
                        ProverError::GuestError("Sp1: getting proof status failed".to_owned())
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
            }
            .ok();
            Ok::<_, ProverError>(Proof {
                proof,
                quote: None,
                kzg_proof: None,
            })
        }
    }

    async fn cancel(key: ProofKey, id_store: Box<&mut dyn IdStore>) -> ProverResult<()> {
        let proof_id = id_store.read_id(key).await?;
        let private_key = env::var("SP1_PRIVATE_KEY").map_err(|_| {
            ProverError::GuestError("SP1_PRIVATE_KEY must be set for remote proving".to_owned())
        })?;
        let network_client = NetworkClient::new(&private_key);
        network_client
            .unclaim_proof(proof_id, UnclaimReason::Abandoned, "".to_owned())
            .await
            .map_err(|_| ProverError::GuestError("Sp1: couldn't unclaim proof".to_owned()))?;
        id_store.remove_id(key).await?;
        Ok(())
    }
}

fn init_verifier() -> Result<PathBuf, ProverError> {
    // Install the plonk verifier from local Sp1 version.
    let artifacts_dir = sp1_sdk::artifacts::try_install_plonk_bn254_artifacts();

    // Read all Solidity files from the artifacts_dir.
    let sol_files = std::fs::read_dir(artifacts_dir)
        .map_err(|_| ProverError::GuestError("Failed to read Sp1 verifier artifacts".to_string()))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("sol"))
        .collect::<Vec<_>>();

    // Write each Solidity file to the contracts directory.
    let contracts_src_dir = std::path::Path::new(CONTRACT_PATH);
    for sol_file in sol_files {
        let sol_file_path = sol_file.path();
        let sol_file_contents = std::fs::read(&sol_file_path).unwrap();
        std::fs::write(
            &contracts_src_dir.join(sol_file_path.file_name().unwrap()),
            sol_file_contents,
        )
        .map_err(|e| ProverError::GuestError(format!("Failed to write Solidity file: {}", e)))?;
    }

    Ok(contracts_src_dir.to_owned())
}

/// A fixture that can be used to test the verification of SP1 zkVM proofs inside Solidity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RaikoProofFixture {
    vkey: String,
    public_values: String,
    proof: String,
}

pub fn verify_sol(vk: SP1VerifyingKey, mut proof: SP1PlonkBn254Proof) -> ProverResult<()> {
    assert!(VERIFIER.is_ok());

    // Deserialize the public values.
    let pi_hash = proof.public_values.read::<[u8; 32]>();

    // Create the testing fixture so we can test things end-to-end.
    let fixture = RaikoProofFixture {
        vkey: vk.bytes32().to_string(),
        public_values: B256::from_slice(&pi_hash).to_string(),
        proof: proof.bytes().to_string(),
    };
    println!("===> Fixture: {:#?}", fixture);

    // Save the fixture to a file.
    println!("Writing fixture to: {:?}", FIXTURE_PATH);
    let fixture_path = PathBuf::from(FIXTURE_PATH);
    if !fixture_path.exists() {
        std::fs::create_dir_all(&fixture_path).map_err(|e| {
            ProverError::GuestError(format!("Failed to create fixture path: {}", e))
        })?;
    }
    std::fs::write(
        fixture_path.join("fixture.json"),
        serde_json::to_string_pretty(&fixture).unwrap(),
    )
    .map_err(|e| ProverError::GuestError(format!("Failed to write fixture: {}", e)))?;

    let child = std::process::Command::new("forge")
        .arg("test -vv")
        .current_dir(CONTRACT_PATH)
        .stdout(std::process::Stdio::inherit()) // Inherit the parent process' stdout
        .spawn();
    println!("Verification started {:?}", child);
    child.map_err(|e| ProverError::GuestError(format!("Failed to run forge: {}", e)))?;

    Ok(())
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
