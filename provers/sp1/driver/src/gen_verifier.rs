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
    // // Setup the logger.
    // sp1_sdk::utils::setup_logger();

    // // Setup the prover client.
    // let client = ProverClient::new();

    // // Setup the program.
    // let (pk, vk) = client.setup(sp1_driver::ELF);

    // // Setup the inputs.;
    // let mut stdin = SP1Stdin::new();
    println!("Reading input from filee");
    let json = std::fs::read_to_string(sp1_driver::E2E_TEST_INPUT_PATH).unwrap();
    let mut input: GuestInput = serde_json::from_str(&json).unwrap();
    let bytes = bincode::serialize(&input).unwrap();
    input = bincode::deserialize(&bytes).unwrap();


    // let json_bytes = serde_json::to_value(&input).unwrap();
    // input = serde_json::from_value(json_bytes).unwrap();


    // let a = bincode::serialize(&input.block.header).unwrap();
    // let b = bincode::serialize(&input.block.body[3].as_eip2930().unwrap().access_list.0[0]).unwrap();
    // let c = bincode::serialize(&input.block.ommers).unwrap();
    // let d = bincode::serialize(&input.block.withdrawals).unwrap();
    // let e = bincode::serialize(&input.block.requests).unwrap();

    // for tx in input.block.body.iter() {
    //     println!("tx_type: {:?}", tx.tx_type());
    //     let tx_bytes = bincode::serialize(tx).unwrap();
    //     let tx: TransactionSigned = bincode::deserialize(tx_bytes.as_slice()).unwrap();
    // }
    // let access_list = &input.block.body[3].as_eip2930().unwrap().access_list.0[0];
    // println!("access_list: {:?}", access_list);
    // let a = bincode::serialize(access_list).unwrap();



    // let mut al2 = alloy_eips::eip2930::AccessListItem::default();
    // al2.storage_keys = vec![B256::random(), B256::random()];
    // println!("access_list: {:?}", al2);
    // let al2b = bincode::serialize(&al2).unwrap();
    // let al2: AccessListItem = bincode::deserialize(al2b.as_slice()).unwrap();
    


    // let c = bincode::serialize(&input.parent_header).unwrap();
    // let d = bincode::serialize(&input.parent_state_trie).unwrap();
    // let e = bincode::serialize(&input.contracts).unwrap();
    // let f = bincode::serialize(&input.ancestor_headers).unwrap();
    // let g = bincode::serialize(&input.taiko).unwrap();

    // println!("tx_type: {:?}", input.block.body[3].as_eip2930());

    // let header: AccessListItem = bincode::deserialize(a.as_slice()).unwrap();
    // let body: AccessList = bincode::deserialize(b.as_slice()).unwrap();
    // let ommers: Vec<Header> = bincode::deserialize(c.as_slice()).unwrap();
    // let withdraw: Option<Withdrawals> = bincode::deserialize(d.as_slice()).unwrap();
    // let parent_header: Option<reth_primitives::Requests> = bincode::deserialize(e.as_slice()).unwrap();
    // let parent_state_trie: MptNode = bincode::deserialize(d.as_slice()).unwrap();
    // let contracts: Vec<Bytes> = bincode::deserialize(e.as_slice()).unwrap();
    // let ancestor_headers: Vec<Header> = bincode::deserialize(f.as_slice()).unwrap();
    // let taiko: TaikoGuestInput = bincode::deserialize(g.as_slice()).unwrap();

    // stdin.write_slice(&input_bytes);
    
    // Generate the proof.
    // let time = Measurement::start("prove_groth16", false);
    // let mut proof = client
    //     .prove(&pk, stdin)
    //     .expect("failed to generate proof");
    // time.stop_with("==> Proof generated");

    // // Deserialize the public values.
    // let pi_hash = proof.public_values.read::<B256>();
    // println!("===> pi: {:?}", pi_hash);

    // // Create the testing fixture so we can test things end-ot-end.
    // let fixture = RaikoProofFixture {
    //     pi_hash: pi_hash.to_string(),
    //     vkey: vk.bytes32().to_string(),
    //     public_values: proof.public_values.bytes().to_string(),
    //     proof: proof.bytes().to_string(),
    // };
    // println!("===> Fixture: {:#?}", fixture);

    // // Save the fixture to a file.
    // println!("Writing fixture to: {:?}", FIXUTRE_PATH);
    // let fixture_path = PathBuf::from(FIXUTRE_PATH);
    // std::fs::create_dir_all(&fixture_path).expect("failed to create fixture path");
    // std::fs::write(
    //     fixture_path.join("fixture.json"),
    //     serde_json::to_string_pretty(&fixture).unwrap(),
    // )
    // .expect("failed to write fixture");
}