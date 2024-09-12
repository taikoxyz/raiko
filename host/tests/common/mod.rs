#![allow(dead_code)]

use std::str::FromStr;

use raiko_core::interfaces::{ProofRequestOpt, ProofType, ProverSpecificOpts};
use raiko_host::{server::serve, ProverState};
use raiko_lib::consts::{Network, SupportedChainSpecs};
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

mod client;
pub mod scenarios;

pub use client::ProofClient;

#[derive(Debug, Deserialize)]
struct RPCResult<T> {
    result: T,
}

type BlockHeightResponse = RPCResult<String>;

#[derive(Debug, Deserialize)]
struct Block {
    #[serde(rename = "gasUsed")]
    gas_used: String,
}

type BlockResponse = RPCResult<Block>;

pub async fn find_recent_block(network: Network) -> anyhow::Result<u64> {
    let supported_chains = SupportedChainSpecs::default();
    let client = reqwest::Client::new();
    let beacon = supported_chains
        .get_chain_spec(&network.to_string())
        .unwrap()
        .rpc;

    let response = client
        .post(beacon.clone())
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_blockNumber",
            "params": [],
            "id": 1
        }))
        .send()
        .await?
        .json::<BlockHeightResponse>()
        .await?;

    let newest_block_number = u64::from_str_radix(&response.result[2..], 16)?;

    let latest_blocks = (newest_block_number - 20)..=newest_block_number;

    let mut blocks = Vec::with_capacity(21);

    for block_number in latest_blocks {
        let response = client
            .post(beacon.clone())
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "method": "eth_getBlockByNumber",
                "params": [format!("0x{block_number:x}"), false],
                "id": 1
            }))
            .send()
            .await?
            .json::<BlockResponse>()
            .await?;

        let gas_used = u64::from_str_radix(&response.result.gas_used[2..], 16)?;

        blocks.push((block_number, gas_used));
    }

    let (block_number, _) = blocks.iter().min_by_key(|(_, gas_used)| *gas_used).unwrap();

    Ok(*block_number)
}

/// Start the Raiko server and return a cancellation token that can be used to stop the server.
pub async fn start_raiko() -> anyhow::Result<CancellationToken> {
    // Initialize the server state.
    dotenv::dotenv().ok();
    let state = ProverState::init().expect("Failed to initialize prover state");
    let token = CancellationToken::new();
    let clone = token.clone();

    // Run the server in a separate thread with the ability to cancel it when our testing is done.
    tokio::spawn(async move {
        tokio::select! {
            _ = token.cancelled() => {
                println!("Test done");
            }
            result = serve(state) => {
                match result {
                    Ok(()) => {
                        panic!("Unexpected server shutdown");
                    }
                    Err(error) => {
                        panic!("Server failed due to: {error:?}");
                    }
                };
            }
        }
    });

    Ok(clone)
}

pub async fn make_request() -> anyhow::Result<ProofRequestOpt> {
    // Get block to test with.
    let block_number = find_recent_block(Network::TaikoMainnet).await?;

    let proof_type =
        ProofType::from_str(&std::env::var("PROOF_TYPE").unwrap_or_else(|_| "native".to_owned()))?;

    Ok(ProofRequestOpt {
        block_number: Some(block_number),
        l1_inclusion_block_number: None,
        network: Some("taiko_mainnet".to_owned()),
        l1_network: Some("ethereum".to_string()),
        graffiti: Some(
            "8008500000000000000000000000000000000000000000000000000000000000".to_owned(),
        ),
        prover: Some("0x70997970C51812dc3A010C7d01b50e0d17dc79C8".to_owned()),
        proof_type: Some(proof_type.to_string()),
        blob_proof_type: Some("proof_of_equivalence".to_string()),
        prover_args: ProverSpecificOpts {
            native: None,
            sgx: None,
            sp1: Some(serde_json::json!({ "verify": false })),
            risc0: None,
        },
    })
}
