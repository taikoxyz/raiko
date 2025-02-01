/// boost rpc is a rpc provider that uses trace-and-dump API to retrieve all the data at once.
/// It is useful for product as it saves lots of nework round trips.
use alloy_primitives::{Address, Bytes, FixedBytes, B256, U256};
use alloy_provider::{ProviderBuilder, ReqwestProvider, RootProvider};
use alloy_rpc_client::{ClientBuilder, RpcClient};
use alloy_rpc_types::{
    eth::{Block as AlloyEthBlock, EIP1186AccountProofResponse, Header as AlloyEthHeader},
    Block, BlockNumberOrTag,
};
use alloy_transport_http::Http;
use reqwest_alloy::Client;
use reth_primitives::{
    revm_primitives::{AccountInfo, Bytecode},
    U64,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{error, info};

use crate::{
    interfaces::{RaikoError, RaikoResult},
    preflight,
    provider::BlockDataProvider,
    MerkleProof,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
/// provingPreflightResult is defined as the minimum data required to replay current block's txlist.
/// as a boost, it can be retrieved from a note who supports trace-and-dump debug_preflight() API.
pub struct PreFlightBoostRpcData {
    /// The block to be proven.
    // pub block: alloy_rpc_types_eth::Block,
    pub block: AlloyEthBlock,
    /// The account proofs.
    pub pre_account_proofs: Vec<EIP1186AccountProofResponse>,
    /// The account proofs.
    pub post_account_proofs: Vec<EIP1186AccountProofResponse>,
    /// The contracts used.
    pub contracts: HashMap<B256, Bytes>,
    /// The ancestor used.
    pub ancestor_hashes: Vec<B256>,
}

#[derive(Clone)]
pub struct BlockDataBoostProvider {
    pub provider: ReqwestProvider,
    pub client: RpcClient<Http<Client>>,
    // use parent because it's the base of tx execution
    block_numbers: Vec<u64>,
    preflight_data: HashMap<u64, PreFlightBoostRpcData>,
    url: String,
}

impl BlockDataBoostProvider {
    pub fn new(url: &str, block_numbers: &[u64]) -> RaikoResult<Self> {
        let url =
            reqwest::Url::parse(url).map_err(|_| RaikoError::RPC("Invalid RPC URL".to_owned()))?;

        let client = ClientBuilder::default().http(url.clone());

        Ok(Self {
            provider: ProviderBuilder::new().on_provider(RootProvider::new_http(url.clone())),
            client,
            block_numbers: block_numbers.to_vec(),
            preflight_data: HashMap::new(),
            url: url.to_string(),
        })
    }

    fn preflight_data_is_available(&self, block_number: u64) -> RaikoResult<()> {
        if self.preflight_data.is_empty() || !self.block_numbers.contains(&block_number) {
            Err(RaikoError::RPC(
                "Preflight data not available for block {block_number:?}".to_owned(),
            ))
        } else {
            Ok(())
        }
    }

    pub fn provider(&self) -> &ReqwestProvider {
        &self.provider
    }

    pub async fn fetch_preflight_data(&mut self) -> RaikoResult<()> {
        let num = self.block_numbers[0];
        let url = self.url.clone() + "trace/" + num.to_string().as_str();
        info!("Fetching preflight data from {}", url);
        let response = reqwest::get(url.clone()).await.map_err(|e| {
            RaikoError::RPC(format!("Failed to fetch preflight data from {url}: {e}"))
        })?;
        let boost_rpc_data = response
            .json()
            .await
            .map_err(|e| RaikoError::RPC(format!("Failed to parse preflight data: {e}")))?;
        self.preflight_data.insert(num, boost_rpc_data);
        Ok(())
    }

    // temporary functions to get account/storage/mkl and mark missing ones as None
    // which will be further processed by fall back to legacy rpc.
    // to be removed after the preflight data is all available

    pub async fn try_get_accounts(
        &self,
        block_number: u64,
        accounts: &[Address],
    ) -> RaikoResult<Vec<Option<AccountInfo>>> {
        let account_infos = self.get_accounts(block_number, accounts).await?;
        let account_opt_infos: Vec<Option<AccountInfo>> = account_infos
            .iter()
            .zip(accounts.iter())
            .map(|(info, addr)| {
                if empty_account_info(info) {
                    error!("Empty account info for address {}", addr);
                    None
                } else {
                    Some(info.clone())
                }
            })
            .collect();
        Ok(account_opt_infos)
    }

    pub async fn try_get_storage_values(
        &self,
        block_number: u64,
        account_keys: &[(Address, U256)],
    ) -> RaikoResult<Vec<Option<U256>>> {
        let storage_values = self.get_storage_values(block_number, account_keys).await?;
        assert_eq!(storage_values.len(), account_keys.len());
        let storage_values: Vec<Option<U256>> = storage_values
            .iter()
            .zip(account_keys.iter())
            .map(|(value, (addr, key))| {
                if is_missing_storage(value) {
                    error!(
                        "Missing storage value for (address,key): ({}, {})",
                        addr, key
                    );
                    None
                } else {
                    Some(value.clone())
                }
            })
            .collect();
        Ok(storage_values)
    }

    pub async fn try_get_merkle_proofs(
        &self,
        block_number: u64,
        accounts: HashMap<Address, Vec<U256>>,
        offset: usize,
        num_storage_proofs: usize,
    ) -> RaikoResult<(MerkleProof, HashMap<Address, Vec<U256>>)> {
        let merkle_proof: HashMap<Address, alloy_rpc_types::EIP1186AccountProofResponse> = self
            .get_merkle_proofs(block_number, accounts.clone(), offset, num_storage_proofs)
            .await?;
        let mut missed_accounts: HashMap<Address, Vec<U256>> = HashMap::new();
        for (address, keys) in accounts.iter() {
            assert!(
                merkle_proof.contains_key(address),
                "{}",
                format!(
                    "address {} is missing in mkl proofs {:?}",
                    address,
                    merkle_proof.keys()
                )
                .as_str()
            );
            let account_proof = merkle_proof.get(address).unwrap();
            let mut missed_keys = Vec::new();

            assert_eq!(account_proof.storage_proof.len(), keys.len());
            if keys.is_empty() {
                if account_proof.account_proof.is_empty() {
                    missed_accounts.insert(*address, missed_keys);
                }
            } else {
                keys.iter()
                    .zip(account_proof.storage_proof.iter())
                    .for_each(|(key, proof)| {
                        if proof.value.is_zero() && proof.proof.is_empty() {
                            missed_keys.push(*key);
                        }
                    });
                if !missed_keys.is_empty() {
                    missed_accounts.insert(*address, missed_keys);
                }
            }
        }

        Ok((merkle_proof, missed_accounts))
    }
}

/// check if account info is a empty account placeholder
fn empty_account_info(account_info: &AccountInfo) -> bool {
    account_info.balance.is_zero()
        && account_info.nonce == 0
        && account_info
            .code
            .clone()
            .is_some_and(|code| code.is_empty())
        && account_info.code_hash.is_zero()
}

/// use U256::MAX as a placeholder for missing storage values
fn is_missing_storage(value: &U256) -> bool {
    *value == U256::MAX
}

impl BlockDataProvider for BlockDataBoostProvider {
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
        let preflight_block_num = block_number + 1;
        self.preflight_data_is_available(preflight_block_num)?;

        let pre_block_image = self.preflight_data.get(&preflight_block_num).unwrap();
        let mut all_accounts: Vec<AccountInfo> = Vec::with_capacity(accounts.len());
        let none_place_holder = alloy_rpc_types::EIP1186AccountProofResponse::default();
        let none_byte_holder = reth_primitives::Bytes::default();
        accounts.iter().for_each(|address| {
            let account_proof: &alloy_rpc_types::EIP1186AccountProofResponse = match pre_block_image
                .pre_account_proofs
                .iter()
                .find(|account_proof| {
                    account_proof.address.to_checksum(None) == address.to_checksum(None)
                }) {
                Some(account_proof) => account_proof,
                None => {
                    info!(
                        "Unable to find account proof for address {} in parent account proofs",
                        address.to_checksum(None)
                    );
                    &none_place_holder
                }
            };

            all_accounts.push(AccountInfo::new(
                account_proof.balance,
                account_proof.nonce.as_limbs()[0],
                account_proof.code_hash,
                Bytecode::new_raw(
                    pre_block_image
                        .contracts
                        .get(&account_proof.code_hash)
                        .or_else(|| Some(&none_byte_holder))
                        .unwrap()
                        .clone(),
                ),
            ));
        });
        Ok(all_accounts)
    }

    async fn get_storage_values(
        &self,
        block_number: u64,
        accounts: &[(Address, U256)],
    ) -> RaikoResult<Vec<U256>> {
        let preflight_block_num = block_number + 1;
        self.preflight_data_is_available(preflight_block_num)?;

        let pre_block_image = self.preflight_data.get(&preflight_block_num).unwrap();
        let mut all_values = Vec::with_capacity(accounts.len());
        let empty_storage_value = U256::MAX;
        for (address, key) in accounts {
            let storage_proof = pre_block_image
                .pre_account_proofs
                .iter()
                .find(|account_proof| {
                    account_proof.address.to_checksum(None) == address.to_checksum(None)
                })
                .and_then(|account_proof| {
                    account_proof
                        .storage_proof
                        .iter()
                        .find(|storage_proof| storage_proof.key.0 == FixedBytes::<32>::from(*key))
                });

            if let Some(storage_proof) = storage_proof {
                all_values.push(storage_proof.value);
            } else {
                error!("Unable to find storage proof for address {address} and key {key}");
                all_values.push(empty_storage_value);
            }
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
        info!(
            "Getting merkle proofs for block {} with offset {} and num_storage_proofs {}",
            block_number, offset, num_storage_proofs
        );
        // todo: better way to get pre & post mpt proof
        // if offset is 0, then we need to get current block's pre mpt proof, which is block_number + 1
        // if offset is 1, then we need to get current block's post mpt proof, which is block_number
        // this complicated logic is becuase previous logic use parent & current as pre & post respectively
        let prefligh_block_num = if offset == 0 {
            block_number + 1
        } else {
            block_number
        };
        self.preflight_data_is_available(prefligh_block_num)?;
        let preflight_block_image = self.preflight_data.get(&prefligh_block_num).unwrap();
        let account_proofs = if offset == 0 {
            &preflight_block_image.pre_account_proofs
        } else {
            &preflight_block_image.post_account_proofs
        };

        let mut storage_proofs: MerkleProof = HashMap::new();

        for (address, keys) in accounts.iter() {
            if let Some(account_proof) = account_proofs
                .iter()
                .find(|account_proof| account_proof.address == *address)
            {
                let mut account_proof_holder = account_proof.clone();
                account_proof_holder.storage_proof = Vec::with_capacity(num_storage_proofs);
                for key in keys.iter().take(num_storage_proofs) {
                    if let Some(proof) = account_proof
                        .storage_proof
                        .iter()
                        .find(|storage_proof| storage_proof.key.0 == FixedBytes::<32>::from(*key))
                    {
                        account_proof_holder.storage_proof.push(proof.clone());
                    } else {
                        account_proof_holder.storage_proof.push(
                            alloy_rpc_types::EIP1186StorageProof {
                                key: alloy_rpc_types::serde_helpers::JsonStorageKey::from(*key),
                                value: U256::ZERO,
                                proof: Vec::new(),
                            },
                        );
                    }
                }
                storage_proofs.insert(*address, account_proof_holder.clone());
            } else {
                let mut all_missed_key_proofs = Vec::new();
                for key in keys.iter().take(num_storage_proofs) {
                    all_missed_key_proofs.push(alloy_rpc_types::EIP1186StorageProof {
                        key: alloy_rpc_types::serde_helpers::JsonStorageKey::from(*key),
                        value: U256::ZERO,
                        proof: Vec::new(),
                    });
                }
                storage_proofs.insert(
                    *address,
                    alloy_rpc_types::EIP1186AccountProofResponse {
                        address: *address,
                        balance: U256::ZERO,
                        nonce: U64::ZERO,
                        code_hash: FixedBytes::<32>::default(),
                        storage_proof: all_missed_key_proofs,
                        storage_hash: FixedBytes::<32>::default(),
                        account_proof: Vec::new(),
                    },
                );
            }
        }
        Ok(storage_proofs)
    }
}

#[cfg(test)]
mod test {
    use super::BlockDataBoostProvider;
    use crate::provider::BlockDataProvider;
    use alloy_primitives::address;

    #[tokio::test]
    async fn test_preflight_rpc() {
        let block_num = std::env::var("BLOCK_NUM")
            .unwrap_or_else(|_| "800000".to_string())
            .parse::<u64>()
            .unwrap();

        let mut preflight_rpc_provider =
            BlockDataBoostProvider::new("http://localhost:8090", &[block_num]).unwrap();

        match preflight_rpc_provider.fetch_preflight_data().await {
            Ok(_) => {}
            Err(e) => {
                println!("Error: {:?}", e);
                assert!(false);
            }
        }

        let accounts = preflight_rpc_provider
            .get_accounts(
                block_num,
                &[address!("9b4AdDB2ee1DE62Da87D8Ec03A9D02877625d829")],
            )
            .await
            .unwrap();
        assert_eq!(accounts.len(), 1);

        let block = preflight_rpc_provider
            .get_blocks(&[(block_num, true)])
            .await
            .unwrap();
        assert_eq!(block.len(), 1);
    }
}
