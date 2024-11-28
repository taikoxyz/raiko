#![cfg(feature = "enable")]
use alloy_primitives::B256;
use raiko_lib::builder::calculate_block_header;
use raiko_lib::input::{BlobProofType, GuestInput, GuestOutput};
use raiko_lib::proof_type::ProofType;
use raiko_lib::protocol_instance::ProtocolInstance;
use raiko_lib::prover::Prover;
use raiko_lib::Measurement;
use serde_json::json;
use sp1_driver::Sp1Prover;
use std::path::PathBuf;

pub const DATA: &str = "./data/";

#[tokio::main]
async fn main() {
    dotenv::from_path("./provers/sp1/driver/.env").ok();

    // Setup the logger.
    sp1_sdk::utils::setup_logger();

    // Setup the inputs.
    let path = std::env::args()
        .last()
        .and_then(|s| {
            let p = PathBuf::from(DATA).join(s);
            if p.exists() {
                Some(p)
            } else {
                None
            }
        })
        .unwrap_or_else(|| PathBuf::from(DATA).join("taiko_mainnet-328833.json"));
    println!("Reading GuestInput from {path:?}");
    let json = std::fs::read_to_string(path).unwrap();

    // Deserialize the input.
    let mut input: GuestInput = serde_json::from_str(&json).unwrap();

    let header = calculate_block_header(&input);

    let _pi = ProtocolInstance::new(&input, &header, ProofType::SP1)
        .unwrap()
        .instance_hash();

    let output = GuestOutput {
        header: reth_primitives::Header::default(),
        hash: B256::default(),
    };
    // Param has higher priority than .env
    let param = json!({
        "sp1" : {
            "recursion": "core",
            "prover": "mock",
            "verify": false
        }
    });
    let time = Measurement::start("prove_groth16 & verify", false);
    input.taiko.blob_proof_type = BlobProofType::ProofOfEquivalence;
    Sp1Prover::run(input, &output, &param, None).await.unwrap();
    time.stop_with("==> Verification complete");
}
