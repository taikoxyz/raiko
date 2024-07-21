#![cfg(feature = "enable")]
use alloy_primitives::B256;
use raiko_lib::input::GuestInput;
use raiko_lib::Measurement;
use serde::{Deserialize, Serialize};
use sp1_sdk::artifacts::try_install_plonk_bn254_artifacts;
use sp1_sdk::Prover;
use sp1_sdk::{HashableKey, ProverClient, SP1Stdin};
use std::path::PathBuf;

pub const FIXTURE_PATH: &str = "./provers/sp1/contracts/src/fixtures/";
pub const CONTRACT_PATH: &str = "./provers/sp1/contracts/src";

/// A fixture that can be used to test the verification of SP1 zkVM proofs inside Solidity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RaikoProofFixture {
    vkey: String,
    public_values: String,
    proof: String,
}

fn main() {
    dotenv::from_path("./provers/sp1/driver/.env").ok();
    let args = std::env::args();

    // Setup the logger.
    sp1_sdk::utils::setup_logger();

    // Install the plonk verifier from local Sp1 version.
    let artifacts_dir = try_install_plonk_bn254_artifacts();

    // Read all Solidity files from the artifacts_dir.
    let sol_files = std::fs::read_dir(artifacts_dir)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("sol"))
        .collect::<Vec<_>>();

    // Write each Solidity file to the contracts directory.
    let contracts_src_dir = std::path::Path::new(CONTRACT_PATH);
    for sol_file in sol_files {
        let sol_file_path = sol_file.path();
        let sol_file_contents = std::fs::read(&sol_file_path).unwrap();
        std::fs::write(
            contracts_src_dir.join(sol_file_path.file_name().unwrap()),
            sol_file_contents,
        )
        .unwrap();
    }

    // Setup the prover client.
    let client = ProverClient::new();

    // Setup the program.
    let (pk, vk) = client.setup(sp1_driver::ELF);

    // Setup the inputs.;
    let mut stdin = SP1Stdin::new();
    let path = args
        .last()
        .map(|s| {
            let p = PathBuf::from(FIXTURE_PATH).join(s);
            if p.exists() {
                p
            } else {
                PathBuf::from(sp1_driver::E2E_TEST_INPUT_PATH)
            }
        })
        .unwrap_or_else(|| PathBuf::from(sp1_driver::E2E_TEST_INPUT_PATH));
    println!("Reading GuestInput from {:?}", path);
    let json = std::fs::read_to_string(path).unwrap();
    let input: GuestInput = serde_json::from_str(&json).unwrap();
    stdin.write_slice(&bincode::serialize(&input).unwrap());

    // Generate the proof.
    let time = Measurement::start("prove_groth16", false);
    let mut proof = client
        .prove_plonk(&pk, stdin)
        .expect("failed to generate proof");
    time.stop_with("==> Proof generated");

    // Deserialize the public values.
    let pi_hash = proof.public_values.read::<[u8; 32]>();
    println!("===> pi: {:?}", pi_hash);

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
        std::fs::create_dir_all(&fixture_path).expect("failed to create fixture path");
    }
    std::fs::write(
        fixture_path.join("fixture.json"),
        serde_json::to_string_pretty(&fixture).unwrap(),
    )
    .expect("failed to write fixture");
}
