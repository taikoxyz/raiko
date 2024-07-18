#![cfg(feature = "enable")]
use alloy_primitives::{Address, B256};
use alloy_sol_types::{sol, SolType};
use dotenv::dotenv;
use raiko_lib::consts::ChainSpec;
use raiko_lib::input::{self, GuestInput, RawGuestOutput, TaikoGuestInput, Transition};
use raiko_lib::primitives::mpt::MptNode;
use raiko_lib::{print_duration, Measurement};
use reth_primitives::{AccessList, AccessListItem, Withdrawals};
use serde::{Deserialize, Serialize};
use sp1_sdk::Prover;
use sp1_sdk::{HashableKey, MockProver, ProverClient, SP1Stdin};
use std::env;
use std::path::PathBuf;
use bincode::Options;
pub const FIXUTRE_PATH: &str = "./provers/sp1/contracts/src/fixtures/fixture.json";

/// A fixture that can be used to test the verification of SP1 zkVM proofs inside Solidity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RaikoProofFixture {
    /// Protocoal Instance hash.
    pi_hash: String,
    vkey: String,
    public_values: String,
    proof: String,
}

use reth_primitives::{
    Block, Header,
    revm_primitives::{Bytes, HashMap, U256},
    TransactionSigned,
};

fn main() {
    dotenv::from_path("./provers/sp1/driver/.env").ok();
    // Setup the logger.
    sp1_sdk::utils::setup_logger();

    // Setup the prover client.
    let client = ProverClient::new();

    // Setup the program.
    let (pk, vk) = client.setup(sp1_driver::ELF);

    // Setup the inputs.;
    let mut stdin = SP1Stdin::new();
    println!("Reading input from file");
    let json = std::fs::read_to_string(sp1_driver::E2E_TEST_INPUT_PATH).unwrap();
    let mut input: GuestInput = serde_json::from_str(&json).unwrap();
    stdin.write_slice(&bincode::serialize(&input).unwrap());
    
    // Generate the proof.
    let time = Measurement::start("prove_groth16", false);
    let mut proof = client
        .prove_plonk(&pk, stdin)
        .expect("failed to generate proof");
    time.stop_with("==> Proof generated");

    // Deserialize the public values.
    let pi_hash = proof.public_values.read::<B256>();
    println!("===> pi: {:?}", pi_hash);

    // Create the testing fixture so we can test things end-ot-end.
    let fixture = RaikoProofFixture {
        pi_hash: pi_hash.to_string(),
        vkey: vk.bytes32().to_string(),
        public_values: proof.public_values.bytes().to_string(),
        proof: proof.bytes().to_string(),
    };
    println!("===> Fixture: {:#?}", fixture);

    // Save the fixture to a file.
    println!("Writing fixture to: {:?}", FIXUTRE_PATH);
    let fixture_path = PathBuf::from(FIXUTRE_PATH);
    std::fs::create_dir_all(&fixture_path).expect("failed to create fixture path");
    std::fs::write(
        fixture_path.join("fixture.json"),
        serde_json::to_string_pretty(&fixture).unwrap(),
    )
    .expect("failed to write fixture");
}