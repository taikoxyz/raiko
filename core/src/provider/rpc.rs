use std::collections::HashMap;

use alloy_primitives::{Address, Bytes, Uint, U256};
use alloy_provider::{ProviderBuilder, ReqwestProvider, RootProvider};
use alloy_rpc_client::{ClientBuilder, RpcClient};
use alloy_rpc_types::{Block, BlockId, BlockNumberOrTag, EIP1186AccountProofResponse};
use alloy_transport_http::Http;
use raiko_lib::clear_line;
use reqwest_alloy::Client;
use reth_primitives::revm_primitives::{AccountInfo, Bytecode};

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
}

impl RpcBlockDataProvider {
    pub fn new(url: &str, block_number: u64) -> RaikoResult<Self> {
        let url =
            reqwest::Url::parse(url).map_err(|_| RaikoError::RPC("Invalid RPC URL".to_owned()))?;
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

            batch
                .send()
                .await
                .map_err(|_| RaikoError::RPC("Error sending batch request".to_owned()))?;

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
        let mut all_accounts = Vec::with_capacity(accounts.len());

        let max_batch_size = 250;
        for accounts in accounts.chunks(max_batch_size) {
            let mut batch = self.client.new_batch();

            let mut requests = Vec::with_capacity(max_batch_size);

            for address in accounts {
                requests.push((
                    Box::pin(
                        batch
                            .add_call::<_, Uint<64, 1>>(
                                "eth_getTransactionCount",
                                &(address, Some(BlockId::from(self.block_number))),
                            )
                            .map_err(|_| {
                                RaikoError::RPC(
                                    "Failed adding eth_getTransactionCount call to batch"
                                        .to_owned(),
                                )
                            })?,
                    ),
                    Box::pin(
                        batch
                            .add_call::<_, Uint<256, 4>>(
                                "eth_getBalance",
                                &(address, Some(BlockId::from(self.block_number))),
                            )
                            .map_err(|_| {
                                RaikoError::RPC(
                                    "Failed adding eth_getBalance call to batch".to_owned(),
                                )
                            })?,
                    ),
                    Box::pin(
                        batch
                            .add_call::<_, Bytes>(
                                "eth_getCode",
                                &(address, Some(BlockId::from(self.block_number))),
                            )
                            .map_err(|_| {
                                RaikoError::RPC(
                                    "Failed adding eth_getCode call to batch".to_owned(),
                                )
                            })?,
                    ),
                ));
            }

            batch
                .send()
                .await
                .map_err(|_| RaikoError::RPC("Error sending batch request".to_owned()))?;

            let mut accounts = Vec::with_capacity(max_batch_size);
            // Collect the data from the batch
            for (nonce_request, balance_request, code_request) in requests {
                let nonce = nonce_request
                    .await
                    .map_err(|e| RaikoError::RPC(format!("Failed to collect nonce request: {e}")))?
                    .try_into()
                    .map_err(|_| {
                        RaikoError::Conversion("Failed to convert nonce to u64".to_owned())
                    })?;

                let balance = balance_request.await.map_err(|e| {
                    RaikoError::RPC(format!("Failed to collect balance request: {e}"))
                })?;

                let bytecode = code_request
                    .await
                    .map_err(|e| RaikoError::RPC(format!("Failed to collect code request: {e}")))
                    .map(Bytecode::new_raw)?;

                let account_info = AccountInfo::new(balance, nonce, bytecode.hash_slow(), bytecode);

                accounts.push(account_info);
            }

            all_accounts.append(&mut accounts);
        }

        Ok(all_accounts)
    }

    async fn get_storage_values(&self, accounts: &[(Address, U256)]) -> RaikoResult<Vec<U256>> {
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
                            RaikoError::RPC(
                                "Failed adding eth_getStorageAt call to batch".to_owned(),
                            )
                        })?,
                ));
            }

            batch
                .send()
                .await
                .map_err(|_| RaikoError::RPC("Error sending batch request".to_owned()))?;

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

        let batch_limit = 1000;

        let batches = accounts
            .iter()
            // Flatten the hashmap into a vector of (address, key) pairs
            // We need to fetch even if there are no keys for an address
            .flat_map(|(&address, keys)| {
                if keys.is_empty() {
                    vec![(address, None)]
                } else {
                    keys.iter()
                        .map(|&key| (address, Some(key)))
                        .collect::<Vec<(Address, Option<U256>)>>()
                }
            })
            .collect::<Vec<_>>()
            // Split the vector into batches of size `batch_limit`
            .chunks(batch_limit)
            // Collect the batches into a vector of request parameters
            .map(|batch| {
                batch.iter().fold(
                    HashMap::<Address, Vec<U256>>::new(),
                    |mut acc, (address, key)| {
                        acc.entry(*address)
                            .and_modify(|keys| {
                                if let Some(key) = key {
                                    keys.push(*key);
                                }
                            })
                            .or_insert(if let Some(key) = key {
                                vec![*key]
                            } else {
                                vec![]
                            });
                        acc
                    },
                )
            })
            .collect::<Vec<_>>();

        for args_batch in batches {
            #[cfg(debug_assertions)]
            raiko_lib::inplace_print(&format!(
                "fetching storage proof {idx}/{num_storage_proofs}..."
            ));
            #[cfg(not(debug_assertions))]
            tracing::trace!("Fetching storage proof {idx}/{num_storage_proofs}...");

            let mut batch = self.client.new_batch();
            let mut requests = Vec::with_capacity(args_batch.len());

            for (address, keys) in args_batch {
                requests.push(Box::pin(
                    batch
                        .add_call::<_, EIP1186AccountProofResponse>(
                            "eth_getProof",
                            &(*address, keys, BlockId::from(block_number)),
                        )
                        .map_err(|_| {
                            RaikoError::RPC("Failed adding eth_getProof call to batch".to_owned())
                        })?,
                ));
            }

            // Send the batch
            batch
                .send()
                .await
                .map_err(|_| RaikoError::RPC("Error sending batch request".to_owned()))?;

            // Collect the data from the batch
            for request in requests {
                let mut proof = request
                    .await
                    .map_err(|e| RaikoError::RPC(format!("Error collecting request data: {e}")))?;
                idx += proof.storage_proof.len();
                storage_proofs
                    .entry(proof.address)
                    .and_modify(|map_proof| {
                        map_proof.storage_proof.append(&mut proof.storage_proof);
                    })
                    .or_insert(proof);
            }
        }
        clear_line();

        Ok(storage_proofs)
    }
}
