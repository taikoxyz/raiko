use raiko_core::interfaces::ProofRequestOpt;
use raiko_host::{
    server::{
        api::{
            v1::Status as StatusV1,
            v2::{CancelStatus, PruneStatus, Status},
        },
        serve,
    },
    ProverState,
};
use raiko_lib::consts::{Network, SupportedChainSpecs};
use raiko_tasks::{TaskDescriptor, TaskStatus};
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

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

const URL: &str = "http://localhost:8080";

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

    pub async fn cancel_proof(
        &self,
        proof_request: ProofRequestOpt,
    ) -> anyhow::Result<CancelStatus> {
        let response = self
            .reqwest_client
            .post(&format!("{URL}/v2/proof/cancel"))
            .json(&proof_request)
            .send()
            .await?;

        if response.status().is_success() {
            let cancel_response = response.json::<CancelStatus>().await?;
            Ok(cancel_response)
        } else {
            Err(anyhow::anyhow!("Failed to send proof request"))
        }
    }

    pub async fn prune_proof(&self) -> anyhow::Result<PruneStatus> {
        let response = self
            .reqwest_client
            .post(&format!("{URL}/v2/proof/prune"))
            .send()
            .await?;

        if response.status().is_success() {
            let prune_response = response.json::<PruneStatus>().await?;
            Ok(prune_response)
        } else {
            Err(anyhow::anyhow!("Failed to send proof request"))
        }
    }

    pub async fn report_proof(&self) -> anyhow::Result<Vec<(TaskDescriptor, TaskStatus)>> {
        let response = self
            .reqwest_client
            .get(&format!("{URL}/v2/proof/report"))
            .send()
            .await?;

        if response.status().is_success() {
            let report_response = response.json::<Vec<(TaskDescriptor, TaskStatus)>>().await?;
            Ok(report_response)
        } else {
            Err(anyhow::anyhow!("Failed to send proof request"))
        }
    }
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
                        assert!(false, "Unexpected server shutdown");
                    }
                    Err(error) => {
                        assert!(false, "Server failed due to: {error:?}");
                    }
                };
            }
        }
    });

    Ok(clone)
}
