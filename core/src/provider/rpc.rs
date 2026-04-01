use alloy_primitives::{Address, Bytes, StorageKey, Uint, U256};
use alloy_provider::{ProviderBuilder, ReqwestProvider, RootProvider};
use alloy_rpc_client::{ClientBuilder, RpcClient};
use alloy_rpc_types::{Block, BlockId, BlockNumberOrTag, EIP1186AccountProofResponse};
use alloy_transport_http::Http;
use raiko_lib::clear_line;
use reqwest_alloy::Client;
use reth_primitives::revm_primitives::{AccountInfo, Bytecode};
use std::{collections::HashMap, env, future::Future, time::Duration};
use tokio::time::sleep;
use tracing::{debug, warn};

use crate::{
    interfaces::{RaikoError, RaikoResult},
    provider::BlockDataProvider,
    MerkleProof,
};

#[derive(Clone)]
pub struct RpcBlockDataProvider {
    pub provider: ReqwestProvider,
    pub client: RpcClient<Http<Client>>,
    block_numbers: Vec<u64>,
}

#[derive(Clone, Copy, Debug)]
struct RpcClientConfig {
    connect_timeout_secs: u64,
    request_timeout_secs: u64,
    max_retries: usize,
    initial_backoff_ms: u64,
    max_backoff_ms: u64,
}

impl RpcClientConfig {
    fn from_env() -> Self {
        Self {
            connect_timeout_secs: env_u64("RAIKO_RPC_CONNECT_TIMEOUT_SECS", 10),
            request_timeout_secs: env_u64("RAIKO_RPC_TIMEOUT_SECS", 120),
            max_retries: env_usize("RAIKO_RPC_MAX_RETRIES", 4),
            initial_backoff_ms: env_u64("RAIKO_RPC_RETRY_INITIAL_BACKOFF_MS", 1_000),
            max_backoff_ms: env_u64("RAIKO_RPC_RETRY_MAX_BACKOFF_MS", 8_000),
        }
    }
}

fn env_u64(name: &str, default: u64) -> u64 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
}

fn env_usize(name: &str, default: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
}

fn next_backoff_ms(current_ms: u64, max_ms: u64) -> u64 {
    current_ms.saturating_mul(2).min(max_ms.max(current_ms))
}

fn is_retryable_rpc_error(error: &RaikoError) -> bool {
    match error {
        RaikoError::RPC(message) => {
            if message.starts_with("Error sending batch request") {
                return true;
            }

            let message = message.to_ascii_lowercase();
            [
                "timeout",
                "timed out",
                "deadline",
                "connection",
                "connect",
                "refused",
                "reset",
                "broken pipe",
                "temporarily unavailable",
                "429",
                "502",
                "503",
                "504",
                "dns",
                "eof",
                "closed",
            ]
            .iter()
            .any(|needle| message.contains(needle))
        }
        _ => false,
    }
}

fn build_http_client() -> RaikoResult<Client> {
    let config = RpcClientConfig::from_env();
    Client::builder()
        .connect_timeout(Duration::from_secs(config.connect_timeout_secs))
        .timeout(Duration::from_secs(config.request_timeout_secs))
        .build()
        .map_err(|err| RaikoError::RPC(format!("Failed to build RPC HTTP client: {err}")))
}

fn build_rpc_client(url: reqwest::Url) -> RaikoResult<RpcClient<Http<Client>>> {
    let http_client = build_http_client()?;
    let transport = Http::with_client(http_client, url);
    let is_local = transport.guess_local();
    Ok(ClientBuilder::default().transport(transport, is_local))
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
        let client = build_rpc_client(url.clone())?;
        Ok(Self {
            provider: ProviderBuilder::new().on_provider(RootProvider::new(client.clone())),
            client,
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
        let client = build_rpc_client(url.clone())?;
        Ok(Self {
            provider: ProviderBuilder::new().on_provider(RootProvider::new(client.clone())),
            client,
            block_numbers,
        })
    }

    pub fn provider(&self) -> &ReqwestProvider {
        &self.provider
    }

    async fn with_rpc_retry<T, F, Fut>(
        &self,
        operation: &'static str,
        mut make_request: F,
    ) -> RaikoResult<T>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = RaikoResult<T>>,
    {
        let config = RpcClientConfig::from_env();
        let total_attempts = config.max_retries.saturating_add(1).max(1);
        let mut backoff_ms = config.initial_backoff_ms;

        for attempt in 1..=total_attempts {
            match make_request().await {
                Ok(value) => return Ok(value),
                Err(err) if attempt < total_attempts && is_retryable_rpc_error(&err) => {
                    if backoff_ms > 0 {
                        warn!(
                            operation = operation,
                            attempt,
                            total_attempts,
                            delay_ms = backoff_ms,
                            error = %err,
                            "Retrying transient RPC failure"
                        );
                        sleep(Duration::from_millis(backoff_ms)).await;
                        backoff_ms = next_backoff_ms(backoff_ms, config.max_backoff_ms);
                    } else {
                        warn!(
                            operation = operation,
                            attempt,
                            total_attempts,
                            error = %err,
                            "Retrying transient RPC failure without backoff"
                        );
                    }
                }
                Err(err) => return Err(err),
            }
        }

        unreachable!("rpc retry loop returns on success or final error")
    }
}

impl BlockDataProvider for RpcBlockDataProvider {
    async fn get_blocks(&self, blocks_to_fetch: &[(u64, bool)]) -> RaikoResult<Vec<Block>> {
        let mut all_blocks = Vec::with_capacity(blocks_to_fetch.len());

        let max_batch_size = 32;
        for blocks_to_fetch in blocks_to_fetch.chunks(max_batch_size) {
            let mut blocks = self
                .with_rpc_retry("eth_getBlockByNumber", || async {
                    let mut batch = self.client.new_batch();
                    let mut requests = Vec::with_capacity(blocks_to_fetch.len());

                    for (block_number, full) in blocks_to_fetch {
                        requests.push(Box::pin(
                            batch
                                .add_call(
                                    "eth_getBlockByNumber",
                                    &(BlockNumberOrTag::from(*block_number), full),
                                )
                                .map_err(|_| {
                                    RaikoError::RPC(
                                        "Failed adding eth_getBlockByNumber call to batch"
                                            .to_owned(),
                                    )
                                })?,
                        ));
                    }

                    batch.send().await.map_err(|e| {
                        RaikoError::RPC(format!(
                            "Error sending batch request for block {blocks_to_fetch:?}: {e}"
                        ))
                    })?;

                    let mut blocks = Vec::with_capacity(blocks_to_fetch.len());
                    for request in requests {
                        blocks.push(request.await.map_err(|e| {
                            RaikoError::RPC(format!("Error collecting request data: {e}"))
                        })?);
                    }

                    Ok(blocks)
                })
                .await?;
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
            let mut fetched_accounts = self
                .with_rpc_retry("eth_getAccountState", || async {
                    let mut batch = self.client.new_batch();

                    let mut nonce_requests = Vec::with_capacity(accounts.len());
                    let mut balance_requests = Vec::with_capacity(accounts.len());
                    let mut code_requests = Vec::with_capacity(accounts.len());

                    for address in accounts {
                        nonce_requests.push(Box::pin(
                            batch
                                .add_call::<_, Uint<64, 1>>(
                                    "eth_getTransactionCount",
                                    &(address, Some(BlockId::from(block_number))),
                                )
                                .map_err(|_| {
                                    RaikoError::RPC(
                                        "Failed adding eth_getTransactionCount call to batch"
                                            .to_owned(),
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
                                    RaikoError::RPC(
                                        "Failed adding eth_getBalance call to batch".to_owned(),
                                    )
                                })?,
                        ));
                        code_requests.push(Box::pin(
                            batch
                                .add_call::<_, Bytes>(
                                    "eth_getCode",
                                    &(address, Some(BlockId::from(block_number))),
                                )
                                .map_err(|_| {
                                    RaikoError::RPC(
                                        "Failed adding eth_getCode call to batch".to_owned(),
                                    )
                                })?,
                        ));
                    }

                    batch
                        .send()
                        .await
                        .map_err(|e| RaikoError::RPC(format!("Error sending batch request {e}")))?;

                    let mut accounts = Vec::with_capacity(accounts.len());
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
                        let account_info =
                            AccountInfo::new(balance, nonce, bytecode.hash_slow(), bytecode);
                        accounts.push(account_info);
                    }

                    Ok(accounts)
                })
                .await?;

            all_accounts.append(&mut fetched_accounts);
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
            let mut values = self
                .with_rpc_retry("eth_getStorageAt", || async {
                    let mut batch = self.client.new_batch();

                    let mut requests = Vec::with_capacity(accounts.len());

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

                    let mut values = Vec::with_capacity(accounts.len());
                    for request in requests {
                        values.push(request.await.map_err(|e| {
                            RaikoError::RPC(format!("Error collecting request data: {e}"))
                        })?);
                    }

                    Ok(values)
                })
                .await?;

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

            let (proofs, remaining_accounts) = self
                .with_rpc_retry("eth_getProof", || {
                    let mut pending_accounts = accounts.clone();

                    async move {
                        let mut batch = self.client.new_batch();
                        let mut requests = Vec::new();

                        let mut batch_size = 0;
                        while !pending_accounts.is_empty() && batch_size < batch_limit {
                            let mut address_to_remove = None;

                            if let Some((address, keys)) = pending_accounts.iter_mut().next() {
                                let num_keys_to_process = if batch_size + keys.len() < batch_limit {
                                    keys.len()
                                } else {
                                    batch_limit - batch_size
                                };

                                if num_keys_to_process == keys.len() {
                                    address_to_remove = Some(*address);
                                }

                                let keys_to_process = keys
                                    .drain(0..num_keys_to_process)
                                    .map(StorageKey::from)
                                    .collect::<Vec<_>>();

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
                                                "Failed adding eth_getProof call to batch"
                                                    .to_owned(),
                                            )
                                        })?,
                                ));

                                batch_size += 1 + keys_to_process.len();
                            }

                            if let Some(address) = address_to_remove {
                                pending_accounts.remove(&address);
                            }
                        }

                        batch.send().await.map_err(|e| {
                            RaikoError::RPC(format!("Error sending batch request {e}"))
                        })?;

                        let mut proofs = Vec::with_capacity(requests.len());
                        for request in requests {
                            proofs.push(request.await.map_err(|e| {
                                RaikoError::RPC(format!("Error collecting request data: {e}"))
                            })?);
                        }

                        Ok((proofs, pending_accounts))
                    }
                })
                .await?;
            accounts = remaining_accounts;

            for mut proof in proofs {
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
