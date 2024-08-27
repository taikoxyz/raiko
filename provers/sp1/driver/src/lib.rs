#![cfg(feature = "enable")]
#![feature(iter_advance_by)]

use once_cell::sync::Lazy;
use raiko_lib::{
    input::{GuestInput, GuestOutput},
    prover::{IdStore, IdWrite, Proof, ProofKey, Prover, ProverConfig, ProverError, ProverResult},
    Measurement,
};
use reth_primitives::B256;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use sp1_sdk::{
    action,
    network::client::NetworkClient,
    proto::network::{ProofMode, UnclaimReason},
};
use sp1_sdk::{HashableKey, ProverClient, SP1Stdin, SP1VerifyingKey};
use std::fs;
use std::path::PathBuf;
use std::{env, path::Path};
use tracing::info;

pub const ELF: &[u8] = include_bytes!("../../guest/elf/sp1-guest");
pub const FIXTURE_PATH: &str = "./provers/sp1/contracts/src/fixtures/";
pub const CONTRACT_PATH: &str = "./provers/sp1/contracts/src/exports/";
const SP1_PROVER_CODE: u8 = 1;

pub static VERIFIER: Lazy<Result<PathBuf, ProverError>> = Lazy::new(init_verifier);
#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sp1Param {
    pub recursion: RecursionMode,
    pub prover: Option<ProverMode>,
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
        let mode = param.prover.clone().unwrap_or_else(get_env_mock);

        let mut stdin = SP1Stdin::new();
        stdin.write(&input);

        // Generate the proof for the given program.
        let client = param
            .prover
            .map(|mode| match mode {
                ProverMode::Mock => ProverClient::mock(),
                ProverMode::Local => ProverClient::local(),
                ProverMode::Network => ProverClient::network(),
            })
            .unwrap_or_else(ProverClient::new);

        let (pk, vk) = client.setup(ELF);

        let prove_action = action::Prove::new(client.prover.as_ref(), &pk, stdin.clone());
        let prove_result = if !matches!(mode, ProverMode::Network) {
            tracing::debug!("Proving locally with recursion mode: {:?}", param.recursion);
            match param.recursion {
                RecursionMode::Core => prove_action.run(),
                RecursionMode::Compressed => prove_action.compressed().run(),
                RecursionMode::Plonk => prove_action.plonk().run(),
            }
            .map_err(|e| ProverError::GuestError(format!("Sp1: local proving failed: {}", e)))
            .unwrap()
        } else {
            let network_prover = sp1_sdk::NetworkProver::new();

            let proof_id = network_prover
                .request_proof(ELF, stdin, param.recursion.clone().into())
                .await
                .map_err(|e| {
                    ProverError::GuestError(format!("Sp1: requesting proof failed: {e}"))
                })?;
            if let Some(id_store) = id_store {
                id_store
                    .store_id(
                        (input.chain_spec.chain_id, output.hash, SP1_PROVER_CODE),
                        proof_id.clone(),
                    )
                    .await?;
            }
            info!(
                "Sp1 Prover: block {:?} - proof id {:?}",
                output.header.number, proof_id
            );
            network_prover
                .wait_proof::<sp1_sdk::SP1ProofWithPublicValues>(&proof_id, None)
                .await
                .map_err(|e| ProverError::GuestError(format!("Sp1: network proof failed {:?}", e)))
                .unwrap()
        };

        let proof = Proof {
            proof: serde_json::to_string(&prove_result).ok(),
            quote: None,
        };

        if param.verify {
            let time = Measurement::start("verify", false);
            verify_sol(vk, prove_result)?;
            time.stop_with("==> Verification complete");
        }

        Ok::<_, ProverError>(proof)
    }

    async fn cancel(key: ProofKey, id_store: Box<&mut dyn IdStore>) -> ProverResult<()> {
        let proof_id = match id_store.read_id(key).await {
            Ok(proof_id) => proof_id,
            Err(e) => {
                if e.to_string().contains("No data for query") {
                    return Ok(());
                } else {
                    return Err(ProverError::GuestError(e.to_string()));
                }
            }
        };
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

fn get_env_mock() -> ProverMode {
    match env::var("SP1_PROVER")
        .unwrap_or("local".to_string())
        .to_lowercase()
        .as_str()
    {
        "mock" => ProverMode::Mock,
        "local" => ProverMode::Local,
        "network" => ProverMode::Network,
        _ => ProverMode::Local,
    }
}

fn init_verifier() -> Result<PathBuf, ProverError> {
    // In cargo run, Cargo sets the working directory to the root of the workspace
    let mut current_dir = std::env::current_dir().unwrap();
    println!("Current dir: {:?}", current_dir);
    if current_dir.ends_with("driver") {
        env::set_current_dir(current_dir.join("../../../"))
            .expect("Failed to set current directory");
        current_dir = std::env::current_dir().unwrap();
    }
    println!("Current dir: {:?}", current_dir);
    let output_dir: PathBuf = current_dir.join(&CONTRACT_PATH);
    let artifacts_dir = sp1_sdk::install::try_install_circuit_artifacts();
    // Create the destination directory if it doesn't exist
    fs::create_dir_all(&output_dir)?;

    // Read the entries in the source directory
    for entry in fs::read_dir(artifacts_dir)? {
        let entry = entry?;
        let src = entry.path();

        // Check if the entry is a file and ends with .sol
        if src.is_file() && src.extension().map(|s| s == "sol").unwrap_or(false) {
            let out = output_dir.join(src.file_name().unwrap());
            fs::copy(&src, &out)?;
            println!("Copied: {:?}", src.file_name().unwrap());
        }
    }
    Ok(output_dir)
}

/// A fixture that can be used to test the verification of SP1 zkVM proofs inside Solidity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RaikoProofFixture {
    vkey: String,
    public_values: String,
    proof: String,
}

pub fn verify_sol(
    vk: SP1VerifyingKey,
    mut proof: sp1_sdk::SP1ProofWithPublicValues,
) -> ProverResult<()> {
    assert!(VERIFIER.is_ok());

    // Deserialize the public values.
    let pi_hash = proof.public_values.read::<[u8; 32]>();

    // Create the testing fixture so we can test things end-to-end.
    let fixture = RaikoProofFixture {
        vkey: vk.bytes32().to_string(),
        public_values: B256::from_slice(&pi_hash).to_string(),
        proof: format!("0x{}", reth_primitives::hex::encode(proof.bytes())),
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
        .arg("test")
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
    use serde_json::json;
    const TEST_ELF: &[u8] = include_bytes!("../../guest/elf/test-sp1-guest");

    #[test]
    fn test_deserialize_sp1_param() {
        let json = json!(
            {
                "recursion": "core",
                "prover": "network",
                "verify": true
            }
        );
        let param = Sp1Param {
            recursion: RecursionMode::Core,
            prover: Some(ProverMode::Network),
            verify: true,
        };
        let serialized = serde_json::to_value(&param).unwrap();
        assert_eq!(json, serialized);

        let deserialized: Sp1Param = serde_json::from_value(serialized).unwrap();
        println!("{:?} {:?}", json, deserialized);
    }

    #[test]
    fn test_init_verifier() {
        VERIFIER.as_ref().expect("Failed to init verifier");
    }

    #[test]
    fn run_unittest_elf() {
        // TODO(Cecilia): imple GuestInput::mock() for unit test
        let client = ProverClient::new();
        let stdin = SP1Stdin::new();
        let (pk, vk) = client.setup(TEST_ELF);
        let proof = client.prove(&pk, stdin).run().unwrap();
        client
            .verify(&proof, &vk)
            .expect("Sp1: verification failed");
    }
}
