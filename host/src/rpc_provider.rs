pub use alloy_primitives::*;
use alloy_provider::{ProviderBuilder, ReqwestProvider, RootProvider};
use alloy_rpc_client::{ClientBuilder, RpcClient};
use alloy_rpc_types::{Block, BlockId, BlockNumberOrTag, EIP1186AccountProofResponse};
use alloy_transport_http::Http;
use raiko_lib::{clear_line, inplace_print};
use reqwest_alloy::Client;
use revm::primitives::{AccountInfo, Bytecode};
use std::collections::HashMap;

use crate::{
    error::{HostError, HostResult},
    raiko::BlockDataProvider,
    MerkleProof,
};

pub struct RpcBlockDataProvider {
    pub provider: ReqwestProvider,
    pub client: RpcClient<Http<Client>>,
    block_number: u64,
}

impl RpcBlockDataProvider {
    pub fn new(url: &str, block_number: u64) -> HostResult<Self> {
        let url =
            reqwest::Url::parse(url).map_err(|_| HostError::RPC("Invalid RPC URL".to_owned()))?;
        Ok(Self {
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
    async fn get_blocks(&self, blocks_to_fetch: &[(u64, bool)]) -> HostResult<Vec<Block>> {
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
                            HostError::RPC(
                                "Failed adding eth_getBlockByNumber call to batch".to_owned(),
                            )
                        })?,
                ));
            }

            batch
                .send()
                .await
                .map_err(|_| HostError::RPC("Error sending batch request".to_owned()))?;

            let mut blocks = Vec::with_capacity(max_batch_size);
            // Collect the data from the batch
            for request in requests {
                blocks.push(
                    request
                        .await
                        .map_err(|_| HostError::RPC("Error collecting request data".to_owned()))?,
                );
            }

            all_blocks.append(&mut blocks);
        }

        Ok(all_blocks)
    }

    async fn get_accounts(&self, accounts: &[Address]) -> HostResult<Vec<AccountInfo>> {
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
                            &(address, Some(BlockId::from(self.block_number))),
                        )
                        .map_err(|_| {
                            HostError::RPC(
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
                            HostError::RPC("Failed adding eth_getBalance call to batch".to_owned())
                        })?,
                ));
                code_requests.push(Box::pin(
                    batch
                        .add_call::<_, Bytes>(
                            "eth_getCode",
                            &(address, Some(BlockId::from(self.block_number))),
                        )
                        .map_err(|_| {
                            HostError::RPC("Failed adding eth_getCode call to batch".to_owned())
                        })?,
                ));
            }

            batch
                .send()
                .await
                .map_err(|_| HostError::RPC("Error sending batch request".to_owned()))?;

            let mut accounts = vec![];
            // Collect the data from the batch
            for ((nonce_request, balance_request), code_request) in nonce_requests
                .into_iter()
                .zip(balance_requests.into_iter())
                .zip(code_requests.into_iter())
            {
                let (nonce, balance, code) = (
                    nonce_request.await.map_err(|_| {
                        HostError::RPC("Failed to collect nonce request".to_owned())
                    })?,
                    balance_request.await.map_err(|_| {
                        HostError::RPC("Failed to collect balance request".to_owned())
                    })?,
                    code_request
                        .await
                        .map_err(|_| HostError::RPC("Failed to collect code request".to_owned()))?,
                );

                let nonce = nonce.try_into().map_err(|_| {
                    HostError::Conversion("Failed to convert nonce to u64".to_owned())
                })?;

                let bytecode = Bytecode::new_raw(code);

                let account_info = AccountInfo::new(balance, nonce, bytecode.hash_slow(), bytecode);

                accounts.push(account_info);
            }

            all_accounts.append(&mut accounts);
        }

        Ok(all_accounts)
    }

    async fn get_storage_values(&self, accounts: &[(Address, U256)]) -> HostResult<Vec<U256>> {
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
                            &(address, key, Some(BlockId::from(self.block_number))),
                        )
                        .map_err(|_| {
                            HostError::RPC(
                                "Failed adding eth_getStorageAt call to batch".to_owned(),
                            )
                        })?,
                ));
            }

            batch
                .send()
                .await
                .map_err(|_| HostError::RPC("Error sending batch request".to_owned()))?;

            let mut values = Vec::with_capacity(max_batch_size);
            // Collect the data from the batch
            for request in requests {
                values.push(
                    request
                        .await
                        .map_err(|_| HostError::RPC("Error collecting request data".to_owned()))?,
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
    ) -> HostResult<MerkleProof> {
        let mut storage_proofs: MerkleProof = HashMap::new();
        let mut idx = offset;

        let mut accounts = accounts.clone();

        let batch_limit = 1000;
        while !accounts.is_empty() {
            inplace_print(&format!(
                "fetching storage proof {idx}/{num_storage_proofs}..."
            ));

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
                                HostError::RPC(
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
                .map_err(|_| HostError::RPC("Error sending batch request".to_owned()))?;

            // Collect the data from the batch
            for request in requests {
                let mut proof = request
                    .await
                    .map_err(|_| HostError::RPC("Error collecting request data".to_owned()))?;
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
