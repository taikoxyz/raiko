use alloy_primitives::{Address, Bytes, StorageKey, Uint, U256};
use alloy_provider::{ProviderBuilder, ReqwestProvider, RootProvider};
use alloy_rpc_client::{ClientBuilder, RpcClient};
use alloy_rpc_types::{Block, BlockId, BlockNumberOrTag, EIP1186AccountProofResponse};
use alloy_transport_http::Http;
use raiko_lib::clear_line;
use reqwest_alloy::Client;
use reth_primitives::revm_primitives::{AccountInfo, Bytecode};
use std::collections::HashMap;

use crate::{
    interfaces::{RaikoError, RaikoResult},
    provider::BlockDataProvider,
    MerkleProof,
};

#[derive(Clone)]
pub struct RpcBlockDataProvider {
    pub provider: ReqwestProvider,
    pub client: RpcClient<Http<Client>>,
    block_number: u64,

    #[cfg(any(test, feature = "test-utils"))]
    pub persistent_block_data:
        std::sync::Arc<tokio::sync::Mutex<super::persistent_map::PersistentBlockData>>,
}

impl RpcBlockDataProvider {
    pub fn new(url: &str, block_number: u64) -> RaikoResult<Self> {
        let url =
            reqwest::Url::parse(url).map_err(|_| RaikoError::RPC("Invalid RPC URL".to_owned()))?;

        Ok(Self {
            #[cfg(any(test, feature = "test-utils"))]
            persistent_block_data: ::std::sync::Arc::new(tokio::sync::Mutex::new(
                super::persistent_map::PersistentBlockData::new(format!(
                    "../testdata/{}",
                    url.to_string()
                        .trim_start_matches("https://")
                        .trim_end_matches("/"),
                )),
            )),

            provider: ProviderBuilder::new().on_provider(RootProvider::new_http(url.clone())),
            client: ClientBuilder::default().http(url),
            block_number,
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

    async fn get_accounts(&self, accounts: &[Address]) -> RaikoResult<Vec<AccountInfo>> {
        #[cfg(any(test, feature = "test-utils"))]
        let all_accounts = &mut self
            .persistent_block_data
            .lock()
            .await
            .accounts(self.block_number);

        #[cfg(not(any(test, feature = "test-utils")))]
        let mut all_accounts = HashMap::with_capacity(accounts.len());

        let to_fetch_accounts: Vec<_> = accounts
            .iter()
            .filter(|address| !all_accounts.contains_key(*address))
            .collect();

        let max_batch_size = 250;
        for accounts in to_fetch_accounts.chunks(max_batch_size) {
            let mut batch = self.client.new_batch();

            let mut nonce_requests = Vec::with_capacity(max_batch_size);
            let mut balance_requests = Vec::with_capacity(max_batch_size);
            let mut code_requests = Vec::with_capacity(max_batch_size);

            for address in accounts {
                nonce_requests.push(Box::pin(
                    batch
                        .add_call::<_, Uint<64, 1>>(
                            "eth_getTransactionCount",
                            &(address, Some(BlockId::from(self.block_number))),
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
                            &(address, Some(BlockId::from(self.block_number))),
                        )
                        .map_err(|_| {
                            RaikoError::RPC("Failed adding eth_getBalance call to batch".to_owned())
                        })?,
                ));
                code_requests.push(Box::pin(
                    batch
                        .add_call::<_, Bytes>(
                            "eth_getCode",
                            &(address, Some(BlockId::from(self.block_number))),
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

            // Collect the data from the batch
            for (((address, nonce_request), balance_request), code_request) in accounts
                .iter()
                .zip(nonce_requests.into_iter())
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
                all_accounts.insert(**address, account_info);
            }
        }

        Ok(accounts
            .iter()
            .map(|address| all_accounts.get(address).expect("checked above").clone())
            .collect())
    }

    async fn get_storage_values(&self, accounts: &[(Address, U256)]) -> RaikoResult<Vec<U256>> {
        #[cfg(any(test, feature = "test-utils"))]
        let all_values = &mut self
            .persistent_block_data
            .lock()
            .await
            .storage_values(self.block_number);

        #[cfg(not(any(test, feature = "test-utils")))]
        let mut all_values: HashMap<(Address, U256), U256> = HashMap::with_capacity(accounts.len());

        let to_fetch_slots: Vec<_> = accounts
            .iter()
            .filter(|(address, slot)| !all_values.contains_key(&(*address, *slot).into()))
            .collect();

        let max_batch_size = 1000;
        for accounts in to_fetch_slots.chunks(max_batch_size) {
            let mut batch = self.client.new_batch();

            let mut requests = Vec::with_capacity(max_batch_size);

            for (address, key) in accounts {
                requests.push(Box::pin(
                    batch
                        .add_call::<_, U256>(
                            "eth_getStorageAt",
                            &(address, key, Some(BlockId::from(self.block_number))),
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

            // Collect the data from the batch
            for ((address, slot), request) in accounts.iter().zip(requests.into_iter()) {
                let value = request
                    .await
                    .map_err(|e| RaikoError::RPC(format!("Error collecting request data: {e}")))?;
                all_values.insert((*address, *slot).into(), value);
            }
        }

        Ok(accounts
            .iter()
            .map(|(address, slot)| all_values.get(&(*address, *slot).into()).unwrap())
            .cloned()
            .collect())
    }

    async fn get_merkle_proofs(
        &self,
        block_number: u64,
        accounts: HashMap<Address, Vec<U256>>,
        offset: usize,
        num_storage_proofs: usize,
    ) -> RaikoResult<MerkleProof> {
        #[cfg(any(test, feature = "test-utils"))]
        let account_proofs = &mut self
            .persistent_block_data
            .lock()
            .await
            .account_proofs(block_number);

        #[cfg(any(test, feature = "test-utils"))]
        let account_storage_proofs = &mut self
            .persistent_block_data
            .lock()
            .await
            .account_storage_proofs(block_number);

        #[cfg(not(any(test, feature = "test-utils")))]
        let account_proofs = &mut HashMap::with_capacity(accounts.len());
        #[cfg(not(any(test, feature = "test-utils")))]
        let account_storage_proofs = &mut HashMap::<
            (Address, U256),
            alloy_rpc_types::EIP1186StorageProof,
        >::with_capacity(accounts.len());

        let mut idx = offset;

        let mut accounts_mut = accounts.clone();

        let batch_limit = 1;
        while !accounts_mut.is_empty() {
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
            while !accounts_mut.is_empty() && batch_size < batch_limit {
                let mut address_to_remove = None;

                if let Some((address, keys)) = accounts_mut.iter_mut().next() {
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
                    let keys_to_process = keys.drain(0..num_keys_to_process).collect::<Vec<_>>();
                    let to_fetch_keys = keys_to_process
                        .iter()
                        .filter(|key| {
                            !account_storage_proofs.contains_key(&(*address, **key).into())
                        })
                        .cloned()
                        .collect::<Vec<_>>();
                    if !to_fetch_keys.is_empty() || !account_proofs.contains_key(address) {
                        requests.push(Box::pin(
                            batch
                                .add_call::<_, EIP1186AccountProofResponse>(
                                    "eth_getProof",
                                    &(
                                        *address,
                                        to_fetch_keys
                                            .iter()
                                            .map(|key| StorageKey::from(*key))
                                            .collect::<Vec<_>>(),
                                        BlockId::from(block_number),
                                    ),
                                )
                                .map_err(|_| {
                                    RaikoError::RPC(
                                        "Failed adding eth_getProof call to batch".to_owned(),
                                    )
                                })?,
                        ));
                    }

                    // Keep track of how many keys were processed
                    // Add an additional 1 for the account proof itself
                    batch_size += 1 + to_fetch_keys.len();
                }

                // Remove the address if all keys were processed for this account
                if let Some(address) = address_to_remove {
                    accounts_mut.remove(&address);
                }
            }

            if requests.is_empty() {
                continue;
            }

            // Send the batch
            batch
                .send()
                .await
                .map_err(|e| RaikoError::RPC(format!("Error sending batch request {e}")))?;

            // Collect the data from the batch
            for request in requests {
                let proof = request
                    .await
                    .map_err(|e| RaikoError::RPC(format!("Error collecting request data: {e}")))?;
                idx += proof.storage_proof.len();

                if !account_proofs.contains_key(&proof.address) {
                    let mut account_only_proof = proof.clone();
                    account_only_proof.storage_proof = vec![];
                    account_proofs.insert(proof.address, account_only_proof);
                }

                for slot_proof in proof.storage_proof {
                    account_storage_proofs.insert(
                        (proof.address.clone(), slot_proof.key.0.into()).into(),
                        slot_proof,
                    );
                }
            }
        }
        clear_line();

        Ok(accounts
            .into_iter()
            .map(|(address, keys)| {
                let mut account_proof = account_proofs.get(&address).unwrap().clone();
                account_proof.storage_proof = vec![];
                for key in keys {
                    let storage_proof = account_storage_proofs
                        .get(&(address.clone(), key).into())
                        .expect("checked above");
                    account_proof.storage_proof.push(storage_proof.clone());
                }
                (address, account_proof)
            })
            .collect())
    }
}
