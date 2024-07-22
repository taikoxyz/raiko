#![cfg(feature = "enable")]

use std::path::PathBuf;

use once_cell::sync::Lazy;
use raiko_lib::{
    input::{GuestInput, GuestOutput},
    prover::{to_proof, Proof, Prover, ProverConfig, ProverError, ProverResult},
};
use reth_primitives::B256;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use sp1_sdk::{HashableKey, ProverClient, SP1PlonkBn254Proof, SP1Stdin, SP1VerifyingKey};

pub const ELF: &[u8] = include_bytes!("../../guest/elf/sp1-guest");
pub const FIXTURE_PATH: &str = "./provers/sp1/contracts/src/fixtures/";
pub const CONTRACT_PATH: &str = "./provers/sp1/contracts/src";

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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProverMode {
    Mock,
    Local,
    Network,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Sp1Response {
    pub proof: String,
}

macro_rules! save_and_return {
    ($proof:ident) => {
        return to_proof(Ok(Sp1Response {
            proof: serde_json::to_string(&$proof).unwrap(),
        }));
    };
}

pub struct Sp1Prover;

impl Prover for Sp1Prover {
    async fn run(
        input: GuestInput,
        _output: &GuestOutput,
        config: &ProverConfig,
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

        let (pk, vk) = client.setup(ELF);

        match param.recursion {
            RecursionMode::Core => {
                let proof = client.prove(&pk, stdin).expect("Sp1: proving failed");
                if param.verify {
                    println!("Cannot run solidity verifier with core proof");
                }
                save_and_return!(proof);
            }
            RecursionMode::Compressed => {
                let proof = client
                    .prove_compressed(&pk, stdin)
                    .expect("Sp1: proving failed");
                if param.verify {
                    println!("Cannot run solidity verifier with compressed proof");
                }
                save_and_return!(proof);
            }
            RecursionMode::Plonk => {
                let proof = client.prove_plonk(&pk, stdin).expect("Sp1: proving failed");
                // Only plonk proof can be verify by smart contract
                if param.verify {
                    verify_sol(vk, proof.clone()).expect("Sp1: verification failed");
                }
                save_and_return!(proof);
            }
        };
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
