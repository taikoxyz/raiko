use raiko_lib::consts::{Network, SupportedChainSpecs};
use rand::Rng;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct RPCResult<T> {
    pub(crate) result: T,
}

pub(crate) type BlockHeightResponse = RPCResult<String>;

#[derive(Debug, Deserialize)]
pub(crate) struct Block {
    #[serde(rename = "gasUsed")]
    pub(crate) gas_used: String,
}

pub(crate) type BlockResponse = RPCResult<Block>;

// NOTE: In order to avoid request collision during multiple tests running in parallel,
//       we select a random block number to make the proof request unique.
pub async fn randomly_select_block(network: Network) -> anyhow::Result<u64> {
    let supported_chains = SupportedChainSpecs::default();
    let client = reqwest::Client::new();
    let beacon = supported_chains
        .get_chain_spec(&network.to_string())
        .unwrap()
        .rpc;

    let tip_block_number = get_block_number(network).await?;
    let random_block_number = rand::thread_rng().gen_range(1..tip_block_number);

    for block_number in random_block_number..tip_block_number {
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
        // TODO why we have to check gas_used > 0? should we treat it as a bug of raiko?
        if gas_used > 0 {
            return Ok(block_number);
        }
    }
    Ok(random_block_number)
}

pub(crate) async fn get_block_number(network: Network) -> anyhow::Result<u64> {
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

    let block_number = u64::from_str_radix(&response.result[2..], 16)?;
    Ok(block_number)
}
