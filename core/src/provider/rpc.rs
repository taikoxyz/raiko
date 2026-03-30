use alloy_primitives::{Address, Bytes, StorageKey, Uint, U256};
use alloy_provider::RootProvider;
use alloy_rpc_client::{ClientBuilder, RpcClient};
use alloy_rpc_types::{
    AccessListResult, Block, BlockId, BlockNumberOrTag, EIP1186AccountProofResponse,
    TransactionRequest,
};
use raiko_lib::clear_line;
use reth_revm::state::{AccountInfo, Bytecode};
use serde::Deserialize;
use std::{collections::{HashMap, HashSet}, future::Future, time::Duration};
use tokio::time::sleep;
use tracing::{debug, info, trace};

use crate::{
    interfaces::{RaikoError, RaikoResult},
    provider::BlockDataProvider,
    MerkleProof,
};

#[derive(Clone, Debug)]
pub struct RpcBlockDataProvider {
    pub provider: RootProvider,
    pub client: RpcClient,
}

impl RpcBlockDataProvider {
    /// async will be used for future preflight optimization
    pub async fn new(url: &str) -> RaikoResult<Self> {
        let url =
            reqwest::Url::parse(url).map_err(|_| RaikoError::RPC("Invalid RPC URL".to_owned()))?;
        debug!("provider rpc url: {:?}", url);
        Ok(Self {
            provider: RootProvider::new_http(url.clone()),
            client: ClientBuilder::default().http(url),
        })
    }

    pub fn provider(&self) -> &RootProvider {
        &self.provider
    }

    /// Use debug_traceCall with prestateTracer to get ALL state accesses for transactions.
    /// This captures nested contract calls unlike eth_createAccessList.
    async fn get_prestate_for_txs(
        &self,
        block_number: u64,
        tx_requests: &[TransactionRequest],
    ) -> RaikoResult<(Vec<Address>, Vec<(Address, U256)>)> {
        let block_id = BlockId::from(block_number);
        let tracer_config = serde_json::json!({
            "tracer": "prestateTracer",
            "tracerConfig": { "diffMode": false }
        });

        let mut batch = self.client.new_batch();
        let mut requests = Vec::with_capacity(tx_requests.len());

        for tx_req in tx_requests {
            requests.push(Box::pin(
                batch
                    .add_call::<_, PrestateTraceResult>(
                        "debug_traceCall",
                        &(tx_req, block_id, &tracer_config),
                    )
                    .map_err(|_| {
                        RaikoError::RPC("Failed adding debug_traceCall to batch".to_owned())
                    })?,
            ));
        }

        batch.send().await.map_err(|e| {
            RaikoError::RPC(format!("Error sending debug_traceCall batch: {e}"))
        })?;

        let mut all_addresses: HashSet<Address> = HashSet::new();
        let mut all_slots: HashSet<(Address, U256)> = HashSet::new();

        for request in requests {
            let Ok(result) = request.await else { continue };
            for (address, account_state) in result.0 {
                all_addresses.insert(address);
                if let Some(storage) = account_state.storage {
                    for (slot, _value) in storage {
                        all_slots.insert((address, U256::from_be_bytes(slot.0)));
                    }
                }
            }
        }

        info!(
            "debug_traceCall (prestateTracer): {} addresses, {} storage slots",
            all_addresses.len(),
            all_slots.len(),
        );

        Ok((
            all_addresses.into_iter().collect(),
            all_slots.into_iter().collect(),
        ))
    }

    async fn construct_and_send_batch(
        &self,
        blocks_to_fetch: &[(u64, bool)],
        max_batch_size: usize,
    ) -> RaikoResult<Vec<Block>> {
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
                request
                    .await
                    .map_err(|e| RaikoError::RPC(format!("Error collecting request data: {e}")))?,
            );
        }

        Ok(blocks)
    }
}

const MAX_RETRIES: u32 = 3;
const INITIAL_DELAY: Duration = Duration::from_secs(1);

impl BlockDataProvider for RpcBlockDataProvider {
    async fn get_blocks(&self, blocks_to_fetch: &[(u64, bool)]) -> RaikoResult<Vec<Block>> {
        let mut all_blocks = Vec::with_capacity(blocks_to_fetch.len());

        let max_batch_size = 32;
        for blocks_to_fetch in blocks_to_fetch.chunks(max_batch_size) {
            let mut blocks = retry_in_case_of_error(
                || self.construct_and_send_batch(blocks_to_fetch, max_batch_size),
                MAX_RETRIES,
                INITIAL_DELAY,
            )
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
        let mut storage_proofs: MerkleProof = HashMap::new();
        let mut idx = offset;

        let mut accounts = accounts.clone();

        let batch_limit = 1000;
        while !accounts.is_empty() {
            if cfg!(debug_assertions) {
                raiko_lib::inplace_print(&format!(
                    "fetching storage proof {idx}/{num_storage_proofs}..."
                ));
            } else {
                trace!("Fetching storage proof {idx}/{num_storage_proofs}...");
            }

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

    async fn get_access_list_for_txs(
        &self,
        block_number: u64,
        tx_requests: &[TransactionRequest],
    ) -> RaikoResult<(Vec<Address>, Vec<(Address, U256)>)> {
        if tx_requests.is_empty() {
            return Ok((vec![], vec![]));
        }

        // Try debug_traceCall with prestateTracer first (captures ALL state accesses
        // including nested calls), fall back to eth_createAccessList if unavailable.
        match self
            .get_prestate_for_txs(block_number, tx_requests)
            .await
        {
            Ok(result) => return Ok(result),
            Err(e) => {
                info!("debug_traceCall unavailable, falling back to eth_createAccessList: {e}");
            }
        }

        let block_id = BlockId::from(block_number);
        let mut batch = self.client.new_batch();
        let mut requests = Vec::with_capacity(tx_requests.len());

        for tx_req in tx_requests {
            requests.push(Box::pin(
                batch
                    .add_call::<_, AccessListResult>("eth_createAccessList", &(tx_req, block_id))
                    .map_err(|_| {
                        RaikoError::RPC(
                            "Failed adding eth_createAccessList call to batch".to_owned(),
                        )
                    })?,
            ));
        }

        batch.send().await.map_err(|e| {
            RaikoError::RPC(format!("Error sending eth_createAccessList batch: {e}"))
        })?;

        let mut all_addresses: HashSet<Address> = HashSet::new();
        let mut all_slots: HashSet<(Address, U256)> = HashSet::new();

        for request in requests {
            // Ignore per-tx errors — access list is best-effort
            let Ok(result) = request.await else { continue };
            for item in result.access_list.0 {
                all_addresses.insert(item.address);
                for key in item.storage_keys {
                    all_slots.insert((item.address, U256::from_be_bytes(key.0)));
                }
            }
        }

        info!(
            "eth_createAccessList: {} addresses, {} storage slots",
            all_addresses.len(),
            all_slots.len(),
        );

        Ok((
            all_addresses.into_iter().collect(),
            all_slots.into_iter().collect(),
        ))
    }

    async fn trace_block_prestate(
        &self,
        block_number: u64,
    ) -> Option<RaikoResult<PrestateTraceResult>> {
        Some(RpcBlockDataProvider::trace_block_prestate(self, block_number).await)
    }
}

/// Prestate tracer result: map of address → account state
#[derive(Debug, Deserialize)]
pub struct PrestateTraceResult(pub HashMap<Address, PrestateAccountState>);

#[derive(Debug, Deserialize)]
pub struct PrestateAccountState {
    pub balance: Option<U256>,
    pub nonce: Option<u64>,
    pub code: Option<Bytes>,
    pub storage: Option<HashMap<StorageKey, U256>>,
}

/// Wrapper for debug_traceBlockByNumber response: each tx trace is wrapped in {"txHash", "result"}
#[derive(Debug, Deserialize)]
struct BlockTraceResult {
    result: PrestateTraceResult,
}

impl RpcBlockDataProvider {
    /// Trace an entire block with prestateTracer to get ALL state accessed during execution.
    /// Returns merged prestate across all transactions in the block.
    pub async fn trace_block_prestate(
        &self,
        block_number: u64,
    ) -> RaikoResult<PrestateTraceResult> {
        let tracer_config = serde_json::json!({
            "tracer": "prestateTracer",
            "tracerConfig": { "diffMode": false }
        });

        let block_id = BlockNumberOrTag::from(block_number);

        let results: Vec<BlockTraceResult> = self
            .client
            .request("debug_traceBlockByNumber", (block_id, &tracer_config))
            .await
            .map_err(|e| {
                RaikoError::RPC(format!("debug_traceBlockByNumber failed: {e}"))
            })?;

        // Merge all per-tx prestates into one
        let mut merged: HashMap<Address, PrestateAccountState> = HashMap::new();
        for tx_result in results {
            for (address, state) in tx_result.result.0 {
                let entry = merged.entry(address).or_insert(PrestateAccountState {
                    balance: None,
                    nonce: None,
                    code: None,
                    storage: None,
                });
                // Take the first non-None value for each field (prestate is the state before execution)
                if entry.balance.is_none() {
                    entry.balance = state.balance;
                }
                if entry.nonce.is_none() {
                    entry.nonce = state.nonce;
                }
                if entry.code.is_none() {
                    entry.code = state.code;
                }
                // Merge storage: keep first value for each slot
                if let Some(storage) = state.storage {
                    let entry_storage = entry.storage.get_or_insert_with(HashMap::new);
                    for (slot, value) in storage {
                        entry_storage.entry(slot).or_insert(value);
                    }
                }
            }
        }

        info!(
            "debug_traceBlockByNumber (prestateTracer): {} addresses, {} storage slots",
            merged.len(),
            merged.values().filter_map(|s| s.storage.as_ref()).map(|s| s.len()).sum::<usize>(),
        );

        Ok(PrestateTraceResult(merged))
    }
}

async fn retry_in_case_of_error<F, Fut, T>(
    operation: F,
    max_retries: u32,
    initial_delay: Duration,
) -> RaikoResult<T>
where
    F: Fn() -> Fut,
    Fut: Future<Output = RaikoResult<T>> + Send,
{
    let mut delay = initial_delay;
    let mut last_error = RaikoError::RPC("".to_owned());

    for attempt in 1..=max_retries {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                last_error = e;
                trace!(
                    "Batch request failed (attempt {}/{}), retrying in {:?}...",
                    attempt,
                    max_retries,
                    delay
                );
                sleep(delay).await;
                delay *= 2; // Exponential backoff
            }
        }
    }

    Err(RaikoError::RPC(format!(
        "Failed to send batch after {max_retries} attempts: {}",
        last_error
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[tokio::test]
    async fn test_retry_in_case_of_error() {
        let attempt_counter = AtomicU32::new(0);
        let operation = || async {
            let attempt = attempt_counter.fetch_add(1, Ordering::SeqCst);
            if attempt < 2 {
                Err(RaikoError::RPC("Simulated failure".to_owned()))
            } else {
                Ok("success")
            }
        };

        let result = retry_in_case_of_error(
            operation,
            3,
            Duration::from_millis(1), // Short delay for tests
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(attempt_counter.load(Ordering::SeqCst), 3);
        assert_eq!(result.unwrap(), "success");

        // Test max retries exceeded
        let attempt_counter = AtomicU32::new(0);
        let failing_operation = || async {
            attempt_counter.fetch_add(1, Ordering::SeqCst);
            Err(RaikoError::RPC("Always fails".to_owned()))
        };

        let result: RaikoResult<()> =
            retry_in_case_of_error(failing_operation, 2, Duration::from_millis(1)).await;

        assert!(result.is_err());
        assert_eq!(attempt_counter.load(Ordering::SeqCst), 2);
        assert!(matches!(result, Err(RaikoError::RPC(_))));
    }
}
