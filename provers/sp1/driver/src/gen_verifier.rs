#![cfg(feature = "enable")]
use alloy_primitives::{Address, B256};
use alloy_sol_types::{sol, SolType};
use raiko_lib::input::{GuestInput, RawGuestOutput, Transition};
use raiko_lib::{print_duration, Measurement};
use serde::{Deserialize, Serialize};
use sp1_sdk::artifacts::export_solidity_groth16_verifier;
use sp1_sdk::Prover;
use sp1_sdk::{HashableKey, MockProver, ProverClient, SP1Stdin};
use std::path::PathBuf;
use dotenv::dotenv;

pub const FIXUTRE_PATH: &str = "./provers/sp1/contracts/src/fixtures/fixture.json";

/// A fixture that can be used to test the verification of SP1 zkVM proofs inside Solidity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RaikoProofFixture {
    pub chain_id: u64,
    pub verifier_address: Address,
    pub transition: Transition,
    pub sgx_instance: Address, // only used for SGX
    pub prover: Address,
    meta_hash: B256,
    vkey: String,
    public_values: String,
    proof: String,
}

fn main() {
    // Setup the logger.
    sp1_sdk::utils::setup_logger();

    dotenv().ok();

    // Setup the prover client.
    let client = ProverClient::new();

    // Setup the program.
    let (pk, vk) = client.setup(sp1_driver::ELF);

    // Setup the inputs.;
    let mut stdin = SP1Stdin::new();
    println!("Reading input from file");
    let json = std::fs::read_to_string(sp1_driver::E2E_TEST_INPUT_PATH).unwrap();
    let input: GuestInput = serde_json::from_str(&json).unwrap();
    stdin.write(&input);

    // Generate the proof.
    let time = Measurement::start("prove_groth16", false);
    let proof = client
        .prove_groth16(&pk, stdin)
        .expect("failed to generate proof");
    time.stop_with("==>Proof generated");

    // Deserialize the public values.
    let bytes = proof.public_values.as_slice();
    let (chain_id, verifier_address, transition, sgx_instance, prover, meta_hash) =
        RawGuestOutput::abi_decode(bytes, false).unwrap();
    println!("===>bytes: {:?}", bytes);

    // Create the testing fixture so we can test things end-ot-end.
    let fixture = RaikoProofFixture {
        chain_id,
        verifier_address,
        transition,
        sgx_instance,
        prover,
        meta_hash,
        vkey: vk.bytes32().to_string(),
        public_values: proof.public_values.bytes().to_string(),
        proof: proof.bytes().to_string(),
    };
    println!("===>Fixture: {:#?}", fixture);

    // Save the fixture to a file.
    println!("Writing fixture to: {:?}", FIXUTRE_PATH);
    let fixture_path = PathBuf::from(FIXUTRE_PATH);
    std::fs::create_dir_all(&fixture_path).expect("failed to create fixture path");
    std::fs::write(
        fixture_path.join("fixture.json"),
        serde_json::to_string_pretty(&fixture).unwrap(),
    )
    .expect("failed to write fixture");

    export_contract().expect("failed to export contract");
}

fn export_contract() -> anyhow::Result<()> {
    sp1_sdk::utils::setup_logger();

    // Export the solidity verifier to the contracts/src directory.
    export_solidity_groth16_verifier(PathBuf::from("../contracts/src"))
        .expect("failed to export verifier");

    // Now generate the vkey digest to use in the contract.
    let prover = MockProver::new();
    let (_, vk) = prover.setup(sp1_driver::ELF);
    println!("VKEY_DIGEST={}", vk.bytes32());

    Ok(())
}
