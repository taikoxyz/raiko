pub use alloy_primitives::*;
use alloy_provider::{ProviderBuilder, ReqwestProvider, RootProvider};
use alloy_rpc_client::{ClientBuilder, RpcClient};
use alloy_rpc_types::{Block, BlockId, BlockNumberOrTag, EIP1186AccountProofResponse};
use alloy_transport_http::Http;
use anyhow::Result;
use raiko_lib::{clear_line, inplace_print};
use reqwest_alloy::Client;
use revm::primitives::{AccountInfo, Bytecode};
use std::collections::HashMap;

use crate::{raiko::BlockDataProvider, MerkleProof};

pub struct RpcBlockDataProvider {
    pub provider: ReqwestProvider,
    pub client: RpcClient<Http<Client>>,
    block_number: u64,
}

impl RpcBlockDataProvider {
    pub fn new(url: &str, block_number: u64) -> Self {
        let url = reqwest::Url::parse(&url).expect("invalid rpc url");
        Self {
            provider: ProviderBuilder::new().on_provider(RootProvider::new_http(url.clone())),
            client: ClientBuilder::default().http(url),
            block_number,
        }
    }

    pub fn provider(&self) -> &ReqwestProvider {
        &self.provider
    }
}

impl BlockDataProvider for RpcBlockDataProvider {
    async fn get_blocks(
        &self,
        blocks_to_fetch: &[(u64, bool)],
    ) -> Result<Vec<Block>, anyhow::Error> {
        let mut all_blocks = Vec::new();

        let max_batch_size = 32;
        for blocks_to_fetch in blocks_to_fetch.chunks(max_batch_size) {
            let mut batch = self.client.new_batch();
            let mut requests = vec![];

            for (block_number, full) in blocks_to_fetch.iter() {
                requests.push(Box::pin(batch.add_call(
                    "eth_getBlockByNumber",
                    &(BlockNumberOrTag::from(*block_number), full),
                )?));
            }

            batch.send().await?;

            let mut blocks = vec![];
            // Collect the data from the batch
            for request in requests.into_iter() {
                blocks.push(request.await?);
            }

            all_blocks.append(&mut blocks);
        }

        Ok(all_blocks)
    }

    async fn get_accounts(&self, accounts: &[Address]) -> Result<Vec<AccountInfo>, anyhow::Error> {
        let mut all_accounts = Vec::new();

        let max_batch_size = 250;
        for accounts in accounts.chunks(max_batch_size) {
            let mut batch = self.client.new_batch();

            let mut nonce_requests = Vec::new();
            let mut balance_requests = Vec::new();
            let mut code_requests = Vec::new();

            for address in accounts {
                nonce_requests.push(Box::pin(
                    batch
                        .add_call::<_, Uint<64, 1>>(
                            "eth_getTransactionCount",
                            &(address, Some(BlockId::from(self.block_number))),
                        )
                        .unwrap(),
                ));
                balance_requests.push(Box::pin(
                    batch
                        .add_call::<_, Uint<256, 4>>(
                            "eth_getBalance",
                            &(address, Some(BlockId::from(self.block_number))),
                        )
                        .unwrap(),
                ));
                code_requests.push(Box::pin(
                    batch
                        .add_call::<_, Bytes>(
                            "eth_getCode",
                            &(address, Some(BlockId::from(self.block_number))),
                        )
                        .unwrap(),
                ));
            }

            batch.send().await?;

            let mut accounts = vec![];
            // Collect the data from the batch
            for (nonce_request, (balance_request, code_request)) in nonce_requests
                .into_iter()
                .zip(balance_requests.into_iter().zip(code_requests.into_iter()))
            {
                let (nonce, balance, code) = (
                    nonce_request.await?,
                    balance_request.await?,
                    code_request.await?,
                );

                let account_info = AccountInfo::new(
                    balance,
                    nonce.try_into().unwrap(),
                    Bytecode::new_raw(code.clone()).hash_slow(),
                    Bytecode::new_raw(code),
                );

                accounts.push(account_info);
            }

            all_accounts.append(&mut accounts);
        }

        Ok(all_accounts)
    }

    async fn get_storage_values(
        &self,
        accounts: &[(Address, U256)],
    ) -> Result<Vec<U256>, anyhow::Error> {
        let mut all_values = Vec::new();

        let max_batch_size = 1000;
        for accounts in accounts.chunks(max_batch_size) {
            let mut batch = self.client.new_batch();

            let mut requests = Vec::new();

            for (address, key) in accounts {
                requests.push(Box::pin(
                    batch
                        .add_call::<_, U256>(
                            "eth_getStorageAt",
                            &(address, key, Some(BlockId::from(self.block_number))),
                        )
                        .unwrap(),
                ));
            }

            batch.send().await?;

            let mut values = vec![];
            // Collect the data from the batch
            for request in requests.into_iter() {
                values.push(request.await?);
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
    ) -> Result<MerkleProof, anyhow::Error> {
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
                        address_to_remove = Some(address.clone());
                    }

                    // Extract the keys to process
                    let keys_to_process = keys
                        .drain(0..num_keys_to_process)
                        .map(|v| StorageKey::from(v))
                        .collect::<Vec<_>>();

                    // Add the request
                    requests.push(Box::pin(
                        batch
                            .add_call::<_, EIP1186AccountProofResponse>(
                                "eth_getProof",
                                &(
                                    address.clone(),
                                    keys_to_process.clone(),
                                    BlockId::from(block_number),
                                ),
                            )
                            .unwrap(),
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
            batch.send().await?;

            // Collect the data from the batch
            for request in requests.into_iter() {
                let mut proof = request.await?;
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
