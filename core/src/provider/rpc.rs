use alloy_primitives::{Address, Bytes, StorageKey, Uint, U256};
use alloy_provider::{ProviderBuilder, ReqwestProvider, RootProvider};
use alloy_rpc_client::{ClientBuilder, RpcClient};
use alloy_rpc_types::{Block, BlockId, BlockNumberOrTag, EIP1186AccountProofResponse};
use alloy_transport_http::Http;
use raiko_lib::clear_line;
use reqwest_alloy::Client;
use reth_primitives::revm_primitives::{AccountInfo, Bytecode};
use std::collections::HashMap;
use tracing::{info, warn};

use crate::{
    interfaces::{RaikoError, RaikoResult},
    provider::{boost_rpc::BlockDataBoostProvider, BlockDataProvider},
    MerkleProof,
};

#[derive(Clone)]
pub struct RpcBlockDataProvider {
    pub provider: ReqwestProvider,
    pub client: RpcClient<Http<Client>>,
    block_numbers: Vec<u64>,
    boost_provider: Option<BlockDataBoostProvider>,
}

impl RpcBlockDataProvider {
    pub async fn new(url: &str, block_number: u64) -> RaikoResult<Self> {
        let url =
            reqwest::Url::parse(url).map_err(|_| RaikoError::RPC("Invalid RPC URL".to_owned()))?;
        info!("RPC URL: {:?} block_number {}", url, block_number);

        let boost_provider = Self::init_boost_rpc_from_env(&[block_number + 1]).await;
        Ok(Self {
            provider: ProviderBuilder::new().on_provider(RootProvider::new_http(url.clone())),
            client: ClientBuilder::default().http(url),
            block_numbers: vec![block_number, block_number + 1],
            boost_provider,
        })
    }

    async fn init_boost_rpc(url: &str, block_numbers: &[u64]) -> Option<BlockDataBoostProvider> {
        let mut preflight_provider = BlockDataBoostProvider::new(url, block_numbers)
            .expect("new BlockDataBoostProvider should be ok");
        match preflight_provider.fetch_preflight_data().await {
            Ok(_) => Some(preflight_provider),
            Err(e) => {
                warn!("Error fetching preflight data: {:?}", e);
                None
            }
        }
    }

    async fn init_boost_rpc_from_env(block_numbers: &[u64]) -> Option<BlockDataBoostProvider> {
        let boost_rpc_url = std::env::var("BOOST_RPC_URL").ok();
        if let Some(url) = boost_rpc_url {
            // boost provider input block number should be the current blockp
            Self::init_boost_rpc(&url, block_numbers).await
        } else {
            warn!("BOOST_RPC_URL not set, using legacy RPC");
            None
        }
    }

    pub async fn new_batch(url: &str, block_numbers: Vec<u64>) -> RaikoResult<Self> {
        assert!(
            !block_numbers.is_empty() && block_numbers.len() > 1,
            "batch block_numbers should have at least 2 elements"
        );
        let url =
            reqwest::Url::parse(url).map_err(|_| RaikoError::RPC("Invalid RPC URL".to_owned()))?;
        info!("BATCH RPC URL: {:?} block_number {}", url, block_numbers[0]);

        let boost_provider = Self::init_boost_rpc_from_env(&block_numbers[1..]).await;
        Ok(Self {
            provider: ProviderBuilder::new().on_provider(RootProvider::new_http(url.clone())),
            client: ClientBuilder::default().http(url),
            block_numbers,
            boost_provider,
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
        info!("get_accounts block_number: {}", block_number);
        assert!(
            self.block_numbers.contains(&block_number),
            "Block number {} not found in {:?}",
            block_number,
            self.block_numbers
        );
        let (account_info_opts, missed_accounts) =
            if let Some(preflight_provider) = &self.boost_provider {
                match preflight_provider
                    .try_get_accounts(block_number, accounts)
                    .await
                {
                    Ok(account_infos) => {
                        let mut missed_accounts = Vec::new();

                        account_infos.iter().zip(accounts.iter()).for_each(
                            |(account_info, address)| {
                                if account_info.is_none() {
                                    missed_accounts.push(*address);
                                }
                            },
                        );
                        (account_infos, missed_accounts)
                    }
                    Err(e) => {
                        tracing::error!("Error getting accounts from preflight provider: {:?}", e);
                        (Vec::new(), accounts.to_vec())
                    }
                }
            } else {
                (Vec::new(), accounts.to_vec())
            };

        tracing::info!(
            "preflight get_accounts missed_accounts: {:?}.",
            missed_accounts
        );

        // fall back to legacy RPC & process missed accounts
        let mut all_missed_accounts = Vec::with_capacity(missed_accounts.len());

        let max_batch_size = 250;
        for accounts in missed_accounts.chunks(max_batch_size) {
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

            all_missed_accounts.append(&mut accounts);
        }

        if account_info_opts.is_empty() {
            Ok(all_missed_accounts)
        } else {
            // insert the missed accounts back with the correct order
            let mut all_accounts = Vec::with_capacity(accounts.len());
            account_info_opts
                .iter()
                .for_each(|account_info_opt| match account_info_opt {
                    Some(account_info) => all_accounts.push(account_info.clone()),
                    None => {
                        let account_info = all_missed_accounts.pop().unwrap();
                        all_accounts.push(account_info);
                    }
                });
            assert!(all_missed_accounts.is_empty());
            Ok(all_accounts)
        }
    }

    async fn get_storage_values(
        &self,
        block_number: u64,
        accounts: &[(Address, U256)],
    ) -> RaikoResult<Vec<U256>> {
        info!("get_storage_values block_number: {}", block_number);

        assert!(
            self.block_numbers.contains(&block_number),
            "Block number {} not found in {:?}",
            block_number,
            self.block_numbers
        );
        let (preflight_storage_values, missed_accounts) =
            if let Some(preflight_provider) = &self.boost_provider {
                match preflight_provider
                    .try_get_storage_values(block_number, accounts)
                    .await
                {
                    Ok(storage_values) => {
                        let mut missed_accounts = Vec::new();
                        storage_values.iter().zip(accounts.iter()).for_each(
                            |(storage_value_opt, account)| {
                                if storage_value_opt.is_none() {
                                    missed_accounts.push(account.clone());
                                }
                            },
                        );

                        (storage_values, missed_accounts)
                    }
                    Err(e) => {
                        tracing::error!(
                            "Error getting storage values from preflight provider: {:?}",
                            e
                        );
                        (Vec::new(), accounts.to_vec())
                    }
                }
            } else {
                (Vec::new(), accounts.to_vec())
            };

        if !preflight_storage_values.is_empty() {
            assert_eq!(preflight_storage_values.len(), accounts.len());
        }

        tracing::info!(
            "preflight get_storage_values missed_accounts: {:?}.",
            missed_accounts
        );

        // fall back to legacy RPC & process missed accounts
        let mut all_missed_values = Vec::with_capacity(missed_accounts.len());

        let max_batch_size = 1000;
        for accounts in missed_accounts.chunks(max_batch_size) {
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

            all_missed_values.append(&mut values);
        }

        if preflight_storage_values.is_empty() {
            return Ok(all_missed_values);
        }

        // insert back the missed values with the correct order
        let mut storage_values = Vec::with_capacity(accounts.len());
        preflight_storage_values
            .iter()
            .for_each(|storage_value_opt| match storage_value_opt {
                Some(storage_value) => storage_values.push(storage_value.clone()),
                None => {
                    let storage_value = all_missed_values.pop().unwrap();
                    storage_values.push(storage_value);
                }
            });
        assert!(all_missed_values.is_empty());

        Ok(storage_values)
    }

    async fn get_merkle_proofs(
        &self,
        block_number: u64,
        accounts: HashMap<Address, Vec<U256>>,
        offset: usize,
        num_storage_proofs: usize,
    ) -> RaikoResult<MerkleProof> {
        info!("get_merkle_proofs block_number: {}", block_number);

        assert!(
            self.block_numbers.contains(&block_number),
            "Block number {} not found in {:?}",
            block_number,
            self.block_numbers
        );
        let (account_key_proofs, missed_accounts) = if let Some(preflight_provider) =
            &self.boost_provider
        {
            match preflight_provider
                .try_get_merkle_proofs(block_number, accounts.clone(), offset, num_storage_proofs)
                .await
            {
                Ok((account_key_proofs, missed_accounts)) => {
                    (account_key_proofs, missed_accounts)
                    // (MerkleProof::new(), accounts)
                }
                Err(e) => {
                    tracing::error!(
                        "Error getting merkle proofs from preflight provider: {:?}",
                        e
                    );
                    (MerkleProof::new(), accounts.clone())
                }
            }
        } else {
            (MerkleProof::new(), accounts.clone())
        };

        tracing::trace!("get_merkle_proofs accounts: {:?}", &accounts.keys());
        tracing::info!(
            "preflight get_merkle_proofs missed_accounts: {:?}",
            missed_accounts.keys()
        );

        let mut storage_proofs: MerkleProof = HashMap::new();
        let mut idx = offset;

        let mut accounts = missed_accounts.clone();

        let batch_limit = 1000;
        while !accounts.is_empty() {
            // #[cfg(debug_assertions)]
            tracing::info!("fetching storage proof {idx}/{num_storage_proofs}...");
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

        if account_key_proofs.is_empty() {
            return Ok(storage_proofs);
        }

        // Insert the account key proofs back with the correct order
        let mut all_account_key_proofs = MerkleProof::new();
        all_account_key_proofs.extend(account_key_proofs.into_iter());
        while !storage_proofs.is_empty() {
            let address = storage_proofs.keys().next().unwrap().clone();
            let mut missed_proof = storage_proofs.remove(&address).unwrap();
            let account_key_proof = all_account_key_proofs.get_mut(&address).unwrap();
            assert_eq!(account_key_proof.address, address);

            account_key_proof.storage_proof = account_key_proof
                .storage_proof
                .iter()
                .map(|storage_proof| {
                    if storage_proof.proof.is_empty() {
                        let storage_proof = missed_proof.storage_proof.pop().unwrap();
                        storage_proof
                    } else {
                        storage_proof.clone()
                    }
                })
                .collect::<Vec<_>>();
            account_key_proof.balance = missed_proof.balance;
            account_key_proof.nonce = missed_proof.nonce;
            account_key_proof.code_hash = missed_proof.code_hash;
            account_key_proof.storage_hash = missed_proof.storage_hash;
            account_key_proof.account_proof = missed_proof.account_proof;
            assert!(
                missed_proof.storage_proof.is_empty(),
                "missing proofs still exist for address {:?}",
                address
            );
        }

        Ok(all_account_key_proofs)
    }

    async fn get_prestate(&self, block_number: u64) -> RaikoResult<super::PrestateImage> {
        info!("get_prestate block_number: {}", block_number);

        assert!(
            self.block_numbers.contains(&block_number),
            "Block number {} not found in {:?}",
            block_number,
            self.block_numbers
        );

        if let Some(preflight_provider) = &self.boost_provider {
            match preflight_provider.get_prestate(block_number).await {
                Ok(prestate) => Ok(prestate),
                Err(e) => {
                    tracing::error!("Error getting prestate from preflight provider: {:?}", e);
                    Err(RaikoError::RPC("No prestate provider".to_owned()))
                }
            }
        } else {
            Err(RaikoError::RPC("No prestate provider".to_owned()))
        }
    }
}
