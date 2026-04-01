use alloy_primitives::{Address, Bytes, StorageKey, Uint, U256};
use alloy_provider::{ProviderBuilder, ReqwestProvider, RootProvider};
use alloy_rpc_client::{ClientBuilder, RpcClient};
use alloy_rpc_types::{Block, BlockId, BlockNumberOrTag, EIP1186AccountProofResponse};
use alloy_transport_http::Http;
use raiko_lib::clear_line;
use reqwest_alloy::Client;
use reth_primitives::revm_primitives::{AccountInfo, Bytecode};
use std::{collections::HashMap, env, time::Duration};
use tracing::debug;

use crate::{
    interfaces::{RaikoError, RaikoResult},
    provider::BlockDataProvider,
    MerkleProof,
};

/// Env: total per-request timeout for L1/L2 JSON-RPC (connect + send + response body).
/// Default 300s so large `eth_getProof` / batch calls can finish on slow nodes.
const ENV_RPC_HTTP_TIMEOUT_SECS: &str = "RAIKO_RPC_HTTP_TIMEOUT_SECS";
/// Env: TCP connect timeout only. Default 30s.
const ENV_RPC_HTTP_CONNECT_TIMEOUT_SECS: &str = "RAIKO_RPC_HTTP_CONNECT_TIMEOUT_SECS";

fn build_rpc_reqwest_client() -> RaikoResult<Client> {
    let timeout_secs: u64 = env::var(ENV_RPC_HTTP_TIMEOUT_SECS)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(300);
    let connect_secs: u64 = env::var(ENV_RPC_HTTP_CONNECT_TIMEOUT_SECS)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);

    Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .connect_timeout(Duration::from_secs(connect_secs))
        .build()
        .map_err(|e| RaikoError::RPC(format!("Failed to build RPC HTTP client: {e}")))
}

fn rpc_http_transport(url: reqwest::Url) -> RaikoResult<(Http<Client>, bool)> {
    let client = build_rpc_reqwest_client()?;
    let http = Http::with_client(client, url);
    let is_local = http.guess_local();
    Ok((http, is_local))
}

#[derive(Clone)]
pub struct RpcBlockDataProvider {
    pub provider: ReqwestProvider,
    pub client: RpcClient<Http<Client>>,
    block_numbers: Vec<u64>,
}

impl RpcBlockDataProvider {
    /// async will be used for future preflight optimization
    pub async fn new(url: &str, block_number: u64) -> RaikoResult<Self> {
        let url =
            reqwest::Url::parse(url).map_err(|_| RaikoError::RPC("Invalid RPC URL".to_owned()))?;
        debug!(
            "provider rpc url: {:?} for block_number {}",
            url, block_number
        );
        let (http, is_local) = rpc_http_transport(url)?;
        let rpc_client = RpcClient::new(http.clone(), is_local);
        Ok(Self {
            provider: ProviderBuilder::new().on_provider(RootProvider::new(rpc_client)),
            client: ClientBuilder::default().transport(http, is_local),
            block_numbers: vec![block_number, block_number + 1],
        })
    }

    pub async fn new_batch(url: &str, block_numbers: Vec<u64>) -> RaikoResult<Self> {
        assert!(
            !block_numbers.is_empty() && block_numbers.len() > 1,
            "batch block_numbers should have at least 2 elements"
        );
        let url =
            reqwest::Url::parse(url).map_err(|_| RaikoError::RPC("Invalid RPC URL".to_owned()))?;
        debug!(
            "Batch provider rpc: {:?} for block_number {}",
            url, block_numbers[0]
        );
        let (http, is_local) = rpc_http_transport(url)?;
        let rpc_client = RpcClient::new(http.clone(), is_local);
        Ok(Self {
            provider: ProviderBuilder::new().on_provider(RootProvider::new(rpc_client)),
            client: ClientBuilder::default().transport(http, is_local),
            block_numbers,
        })
    }

    pub fn provider(&self) -> &ReqwestProvider {
        &self.provider
    }
}

impl BlockDataProvider for RpcBlockDataProvider {
    async fn get_blocks(&self, blocks_to_fetch: &[(u64, bool)]) -> RaikoResult<Vec<Block>> {
        let mut all_blocks = Vec::with_capacity(blocks_to_fetch.len());

        let max_batch_size = 32;
        for blocks_to_fetch in blocks_to_fetch.chunks(max_batch_size) {
            let mut batch = self.client.new_batch();
            let mut requests = Vec::with_capacity(max_batch_size);

            for (block_number, full) in blocks_to_fetch {
                requests.push(Box::pin(
                    batch
                        .add_call(
                            "eth_getBlockByNumber",
                            &(BlockNumberOrTag::from(*block_number), full),
                        )
                        .map_err(|_| {
                            RaikoError::RPC(
                                "Failed adding eth_getBlockByNumber call to batch".to_owned(),
                            )
                        })?,
                ));
            }

            batch.send().await.map_err(|e| {
                RaikoError::RPC(format!(
                    "Error sending batch request for block {blocks_to_fetch:?}: {e}"
                ))
            })?;

            let mut blocks = Vec::with_capacity(max_batch_size);
            // Collect the data from the batch
            for request in requests {
                blocks.push(
                    request.await.map_err(|e| {
                        RaikoError::RPC(format!("Error collecting request data: {e}"))
                    })?,
                );
            }

            all_blocks.append(&mut blocks);
        }

        Ok(all_blocks)
    }

    async fn get_accounts(
        &self,
        block_number: u64,
        accounts: &[Address],
    ) -> RaikoResult<Vec<AccountInfo>> {
        assert!(
            self.block_numbers.contains(&block_number),
            "Block number {} not found in {:?}",
            block_number,
            self.block_numbers
        );
        let mut all_accounts = Vec::with_capacity(accounts.len());

        let max_batch_size = 250;
        for accounts in accounts.chunks(max_batch_size) {
            let mut batch = self.client.new_batch();

            let mut nonce_requests = Vec::with_capacity(max_batch_size);
            let mut balance_requests = Vec::with_capacity(max_batch_size);
            let mut code_requests = Vec::with_capacity(max_batch_size);

            for address in accounts {
                nonce_requests.push(Box::pin(
                    batch
                        .add_call::<_, Uint<64, 1>>(
                            "eth_getTransactionCount",
                            &(address, Some(BlockId::from(block_number))),
                        )
                        .map_err(|_| {
                            RaikoError::RPC(
                                "Failed adding eth_getTransactionCount call to batch".to_owned(),
                            )
                        })?,
                ));
                balance_requests.push(Box::pin(
                    batch
                        .add_call::<_, Uint<256, 4>>(
                            "eth_getBalance",
                            &(address, Some(BlockId::from(block_number))),
                        )
                        .map_err(|_| {
                            RaikoError::RPC("Failed adding eth_getBalance call to batch".to_owned())
                        })?,
                ));
                code_requests.push(Box::pin(
                    batch
                        .add_call::<_, Bytes>(
                            "eth_getCode",
                            &(address, Some(BlockId::from(block_number))),
                        )
                        .map_err(|_| {
                            RaikoError::RPC("Failed adding eth_getCode call to batch".to_owned())
                        })?,
                ));
            }

            batch
                .send()
                .await
                .map_err(|e| RaikoError::RPC(format!("Error sending batch request {e}")))?;

            let mut accounts = vec![];
            // Collect the data from the batch
            for ((nonce_request, balance_request), code_request) in nonce_requests
                .into_iter()
                .zip(balance_requests.into_iter())
                .zip(code_requests.into_iter())
            {
                let (nonce, balance, code) = (
                    nonce_request.await.map_err(|e| {
                        RaikoError::RPC(format!("Failed to collect nonce request: {e}"))
                    })?,
                    balance_request.await.map_err(|e| {
                        RaikoError::RPC(format!("Failed to collect balance request: {e}"))
                    })?,
                    code_request.await.map_err(|e| {
                        RaikoError::RPC(format!("Failed to collect code request: {e}"))
                    })?,
                );

                let nonce = nonce.try_into().map_err(|_| {
                    RaikoError::Conversion("Failed to convert nonce to u64".to_owned())
                })?;

                let bytecode = Bytecode::new_raw(code);

                let account_info = AccountInfo::new(balance, nonce, bytecode.hash_slow(), bytecode);

                accounts.push(account_info);
            }

            all_accounts.append(&mut accounts);
        }

        Ok(all_accounts)
    }

    async fn get_storage_values(
        &self,
        block_number: u64,
        accounts: &[(Address, U256)],
    ) -> RaikoResult<Vec<U256>> {
        assert!(
            self.block_numbers.contains(&block_number),
            "Block number {} not found in {:?}",
            block_number,
            self.block_numbers
        );
        let mut all_values = Vec::with_capacity(accounts.len());

        let max_batch_size = 1000;
        for accounts in accounts.chunks(max_batch_size) {
            let mut batch = self.client.new_batch();

            let mut requests = Vec::with_capacity(max_batch_size);

            for (address, key) in accounts {
                requests.push(Box::pin(
                    batch
                        .add_call::<_, U256>(
                            "eth_getStorageAt",
                            &(address, key, Some(BlockId::from(block_number))),
                        )
                        .map_err(|_| {
                            RaikoError::RPC(
                                "Failed adding eth_getStorageAt call to batch".to_owned(),
                            )
                        })?,
                ));
            }

            batch
                .send()
                .await
                .map_err(|e| RaikoError::RPC(format!("Error sending batch request {e}")))?;

            let mut values = Vec::with_capacity(max_batch_size);
            // Collect the data from the batch
            for request in requests {
                values.push(
                    request.await.map_err(|e| {
                        RaikoError::RPC(format!("Error collecting request data: {e}"))
                    })?,
                );
            }

            all_values.append(&mut values);
        }

        Ok(all_values)
    }

    async fn get_merkle_proofs(
        &self,
        block_number: u64,
        accounts: HashMap<Address, Vec<U256>>,
        offset: usize,
        num_storage_proofs: usize,
    ) -> RaikoResult<MerkleProof> {
        assert!(
            self.block_numbers.contains(&block_number),
            "Block number {} not found in {:?}",
            block_number,
            self.block_numbers
        );
        let mut storage_proofs: MerkleProof = HashMap::new();
        let mut idx = offset;

        let mut accounts = accounts.clone();

        let batch_limit = 1000;
        while !accounts.is_empty() {
            #[cfg(debug_assertions)]
            raiko_lib::inplace_print(&format!(
                "fetching storage proof {idx}/{num_storage_proofs}..."
            ));
            #[cfg(not(debug_assertions))]
            tracing::trace!("Fetching storage proof {idx}/{num_storage_proofs}...");

            // Create a batch for all storage proofs
            let mut batch = self.client.new_batch();

            // Collect all requests
            let mut requests = Vec::new();

            let mut batch_size = 0;
            while !accounts.is_empty() && batch_size < batch_limit {
                let mut address_to_remove = None;

                if let Some((address, keys)) = accounts.iter_mut().next() {
                    // Calculate how many keys we can still process
                    let num_keys_to_process = if batch_size + keys.len() < batch_limit {
                        keys.len()
                    } else {
                        batch_limit - batch_size
                    };

                    // If we can process all keys, remove the address from the map after the loop
                    if num_keys_to_process == keys.len() {
                        address_to_remove = Some(*address);
                    }

                    // Extract the keys to process
                    let keys_to_process = keys
                        .drain(0..num_keys_to_process)
                        .map(StorageKey::from)
                        .collect::<Vec<_>>();

                    // Add the request
                    requests.push(Box::pin(
                        batch
                            .add_call::<_, EIP1186AccountProofResponse>(
                                "eth_getProof",
                                &(
                                    *address,
                                    keys_to_process.clone(),
                                    BlockId::from(block_number),
                                ),
                            )
                            .map_err(|_| {
                                RaikoError::RPC(
                                    "Failed adding eth_getProof call to batch".to_owned(),
                                )
                            })?,
                    ));

                    // Keep track of how many keys were processed
                    // Add an additional 1 for the account proof itself
                    batch_size += 1 + keys_to_process.len();
                }

                // Remove the address if all keys were processed for this account
                if let Some(address) = address_to_remove {
                    accounts.remove(&address);
                }
            }

            // Send the batch
            batch
                .send()
                .await
                .map_err(|e| RaikoError::RPC(format!("Error sending batch request {e}")))?;

            // Collect the data from the batch
            for request in requests {
                let mut proof = request
                    .await
                    .map_err(|e| RaikoError::RPC(format!("Error collecting request data: {e}")))?;
                idx += proof.storage_proof.len();
                if let Some(map_proof) = storage_proofs.get_mut(&proof.address) {
                    map_proof.storage_proof.append(&mut proof.storage_proof);
                } else {
                    storage_proofs.insert(proof.address, proof);
                }
            }
        }
        clear_line();

        Ok(storage_proofs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;

    fn set_short_rpc_timeouts_for_test() {
        std::env::set_var(ENV_RPC_HTTP_TIMEOUT_SECS, "1");
        std::env::set_var(ENV_RPC_HTTP_CONNECT_TIMEOUT_SECS, "1");
    }

    fn clear_rpc_timeout_env() {
        std::env::remove_var(ENV_RPC_HTTP_TIMEOUT_SECS);
        std::env::remove_var(ENV_RPC_HTTP_CONNECT_TIMEOUT_SECS);
    }

    /// Local TCP server: read the JSON-RPC POST then stall so the client hits `reqwest` timeout.
    async fn spawn_stall_after_accept_json_rpc() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind test listener");
        let addr = listener.local_addr().expect("listener addr");
        tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = [0u8; 8192];
                let _ = stream.read(&mut buf).await;
                tokio::time::sleep(Duration::from_secs(600)).await;
            }
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    #[serial]
    async fn get_blocks_returns_rpc_error_when_http_times_out() {
        set_short_rpc_timeouts_for_test();
        let url = spawn_stall_after_accept_json_rpc().await;
        let provider = RpcBlockDataProvider::new(&url, 1)
            .await
            .expect("provider new with short timeout");
        let result = provider.get_blocks(&[(1, false)]).await;
        clear_rpc_timeout_env();

        let err = result.expect_err("expected RPC failure when server does not respond in time");
        let RaikoError::RPC(payload) = &err else {
            panic!("expected RaikoError::RPC, got {err:?}");
        };
        let lower = payload.to_lowercase();
        // Reqwest may report `operation timed out` or a generic `error sending request for url (...)`
        // when the overall request timeout fires.
        assert!(
            lower.contains("timeout")
                || lower.contains("timed out")
                || payload.contains("error sending request for url"),
            "expected timeout or stalled-request error, got: {err}"
        );
    }
}
