use std::time::Duration;

use lazy_static::lazy_static;
use raiko_core::interfaces::ProofRequestOpt;
use raiko_host::server::api::{v1::Status as StatusV1, v2::Status};
use raiko_lib::consts::Network;

const URL: &str = "http://localhost:8080";

const TAIKOSCAN_URL: &str = "https://api.taikoscan.io/api";
lazy_static! {
    static ref API_KEY: String =
        std::env::var("TAIKOSCAN_API_KEY").unwrap_or_else(|_| "YourApiKeyToken".to_owned());
}

pub async fn find_recent_block(network: Network) -> anyhow::Result<u64> {
    let api_key = API_KEY.clone();
    let newest_block_number = match network {
        Network::TaikoMainnet => {
            let response = reqwest::get(&format!(
                "{TAIKOSCAN_URL}?module=proxy&action=eth_blockNumber&apikey={api_key}"
            ))
            .await?
            .json::<serde_json::Value>()
            .await?;
            let block_number = u64::from_str_radix(&response["result"].as_str().unwrap()[2..], 16)?;
            Ok(block_number)
        }
        _ => Err(anyhow::anyhow!("Unsupported network")),
    }?;

    let latest_blocks = (newest_block_number - 20)..=newest_block_number;

    let mut blocks = Vec::with_capacity(21);

    for block_number in latest_blocks {
        tokio::time::sleep(Duration::from_secs(5)).await;
        let response = reqwest::get(&format!(
            "{TAIKOSCAN_URL}?module=proxy&action=eth_getBlockByNumber&tag=0x{block_number:x}&boolean=false&apikey={api_key}"
        ))
        .await?
        .json::<serde_json::Value>()
        .await?;

        if response["result"].is_null() {
            continue;
        }

        let block = response["result"].clone();
        let gas_used = u64::from_str_radix(&block["gasUsed"].as_str().unwrap()[2..], 16)?;

        blocks.push((block_number, gas_used));
    }

    let (block_number, _) = blocks.iter().min_by_key(|(_, gas_used)| *gas_used).unwrap();

    Ok(*block_number)
}

pub struct ProofClient {
    reqwest_client: reqwest::Client,
}

impl ProofClient {
    pub fn new() -> Self {
        Self {
            reqwest_client: reqwest::Client::new(),
        }
    }

    pub async fn send_proof_v1(&self, proof_request: ProofRequestOpt) -> anyhow::Result<StatusV1> {
        let response = self
            .reqwest_client
            .post(&format!("{URL}/v1/proof"))
            .json(&proof_request)
            .send()
            .await?;

        if response.status().is_success() {
            let proof_response = response.json::<StatusV1>().await?;
            Ok(proof_response)
        } else {
            Err(anyhow::anyhow!("Failed to send proof request"))
        }
    }

    pub async fn send_proof_v2(&self, proof_request: ProofRequestOpt) -> anyhow::Result<Status> {
        let response = self
            .reqwest_client
            .post(&format!("{URL}/v2/proof"))
            .json(&proof_request)
            .send()
            .await?;

        if response.status().is_success() {
            let proof_response = response.json::<Status>().await?;
            Ok(proof_response)
        } else {
            Err(anyhow::anyhow!("Failed to send proof request"))
        }
    }
}
