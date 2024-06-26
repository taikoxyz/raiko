use crate::{
    interfaces::{RaikoError, RaikoResult},
    provider::{db::ProviderDb, rpc::RpcBlockDataProvider, BlockDataProvider},
};
use alloy_consensus::TxEnvelope;
pub use alloy_primitives::*;
use alloy_provider::{Provider, ReqwestProvider};
use alloy_rpc_types::{Block, BlockTransactions, Filter, Transaction as AlloyRpcTransaction};
use alloy_sol_types::{SolCall, SolEvent};
use anyhow::{anyhow, bail, Result};
use kzg::eip_4844::Blob;
use raiko_lib::{
    builder::{OptimisticDatabase, RethBlockBuilder},
    clear_line,
    consts::ChainSpec,
    inplace_print,
    input::{
        decode_anchor, proposeBlockCall, BlockProposed, GuestInput, TaikoGuestInput,
        TaikoProverData,
    }, 
    primitives::{
        eip4844::{self, set_commitment_proof, MAINNET_KZG_TRUSTED_SETUP}, 
        mpt::proofs_to_tries
    }, 
    utils::{generate_transactions, zlib_compress_data}, 
    easurement
};
use reth_primitives::Block as RethBlock;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tracing::{debug, info, warn};

pub async fn preflight<BDP: BlockDataProvider>(
    provider: BDP,
    block_number: u64,
    l1_chain_spec: ChainSpec,
    taiko_chain_spec: ChainSpec,
    prover_data: TaikoProverData,
) -> RaikoResult<GuestInput> {
    let measurement = Measurement::start("Fetching block data...", false);

    // Get the block and the parent block
    let blocks = provider
        .get_blocks(&[(block_number, true), (block_number - 1, false)])
        .await?;
    let (block, parent_block) = (
        blocks.first().ok_or_else(|| {
            RaikoError::Preflight("No block data for the requested block".to_owned())
        })?,
        &blocks.get(1).ok_or_else(|| {
            RaikoError::Preflight("No parent block data for the requested block".to_owned())
        })?,
    );

    info!(
        "Processing block {:?} with hash: {:?}",
        block.header.number,
        block.header.hash.unwrap(),
    );
    debug!("block.parent_hash: {:?}", block.header.parent_hash);
    debug!("block gas used: {:?}", block.header.gas_used);
    debug!("block transactions: {:?}", block.transactions.len());

    let taiko_guest_input = if taiko_chain_spec.is_taiko() {
        prepare_taiko_chain_input(
            &l1_chain_spec,
            &taiko_chain_spec,
            block_number,
            block,
            prover_data,
        )
        .await?
    } else {
        // For Ethereum blocks we just convert the block transactions in a tx_list
        // so that we don't have to supports separate paths.
        TaikoGuestInput {
            tx_data: zlib_compress_data(&alloy_rlp::encode(&get_transactions_from_block(block)?))?,
            ..Default::default()
        }
    };
    measurement.stop();

    let reth_block = RethBlock::try_from(block.clone()).expect("block convert failed");

    let input = GuestInput {
        block: reth_block.clone(),
        chain_spec: taiko_chain_spec.clone(),
        block_number,
        beneficiary: block.header.miner,
        gas_limit: block.header.gas_limit.try_into().map_err(|_| {
            RaikoError::Conversion("Failed converting gas limit to u64".to_string())
        })?,
        timestamp: block.header.timestamp,
        extra_data: block.header.extra_data.clone(),
        mix_hash: if let Some(mix_hash) = block.header.mix_hash {
            mix_hash
        } else {
            return Err(RaikoError::Preflight(
                "No mix hash for the requested block".to_owned(),
            ));
        },
        withdrawals: block.withdrawals.clone().unwrap_or_default(),
        parent_state_trie: Default::default(),
        parent_storage: Default::default(),
        contracts: Default::default(),
        parent_header: parent_block.header.clone().try_into().unwrap(),
        ancestor_headers: Default::default(),
        base_fee_per_gas: block.header.base_fee_per_gas.map_or_else(
            || {
                Err(RaikoError::Preflight(
                    "No base fee per gas for the requested block".to_owned(),
                ))
            },
            |base_fee_per_gas| {
                base_fee_per_gas.try_into().map_err(|_| {
                    RaikoError::Conversion("Failed converting base fee per gas to u64".to_owned())
                })
            },
        )?,
        blob_gas_used: block.header.blob_gas_used.map_or_else(
            || Ok(None),
            |b: u128| -> RaikoResult<Option<u64>> {
                b.try_into().map(Some).map_err(|_| {
                    RaikoError::Conversion("Failed converting blob gas used to u64".to_owned())
                })
            },
        )?,
        excess_blob_gas: block.header.excess_blob_gas.map_or_else(
            || Ok(None),
            |b: u128| -> RaikoResult<Option<u64>> {
                b.try_into().map(Some).map_err(|_| {
                    RaikoError::Conversion("Failed converting excess blob gas to u64".to_owned())
                })
            },
        )?,
        parent_beacon_block_root: block.header.parent_beacon_block_root,
        taiko: taiko_guest_input,
    };

    // Create the block builder, run the transactions and extract the DB
    let provider_db = ProviderDb::new(
        provider,
        taiko_chain_spec,
        if let Some(parent_block_number) = parent_block.header.number {
            parent_block_number
        } else {
            return Err(RaikoError::Preflight(
                "No parent block number for the requested block".to_owned(),
            ));
        },
    )
    .await?;

    let mut builder = RethBlockBuilder::new(&input, provider_db);
    builder.prepare_header()?;

    // Optimize data gathering by executing the transactions multiple times so data can be requested in batches
    let is_local = false;
    let max_iterations = if is_local { 1 } else { 100 };
    let mut done = false;
    let mut num_iterations = 0;
    while !done {
        inplace_print(&format!("Execution iteration {num_iterations}..."));

        let optimistic = num_iterations + 1 < max_iterations;
        builder.db.as_mut().unwrap().optimistic = optimistic;

        builder.execute_transactions(optimistic).expect("execute");
        if builder.db.as_mut().unwrap().fetch_data().await {
            done = true;
        }
        num_iterations += 1;
    }
    clear_line();
    println!("State data fetched in {num_iterations} iterations");

    let provider_db = builder.db.as_mut().unwrap();

    // Gather inclusion proofs for the initial and final state
    let measurement = Measurement::start("Fetching storage proofs...", true);
    let (parent_proofs, proofs, num_storage_proofs) = provider_db.get_proofs().await?;
    measurement.stop_with_count(&format!(
        "[{} Account/{num_storage_proofs} Storage]",
        parent_proofs.len() + proofs.len(),
    ));

    // Construct the state trie and storage from the storage proofs.
    let measurement = Measurement::start("Constructing MPT...", true);
    let (state_trie, storage) =
        proofs_to_tries(input.parent_header.state_root, parent_proofs, proofs)?;
    measurement.stop();

    // Gather proofs for block history
    let measurement = Measurement::start("Fetching historical block headers...", true);
    let ancestor_headers = provider_db.get_ancestor_headers().await?;
    measurement.stop();

    // Get the contracts from the initial db.
    let measurement = Measurement::start("Fetching contract code...", true);
    let mut contracts = HashSet::new();
    let initial_db = &provider_db.initial_db;
    for account in initial_db.accounts.values() {
        let code = &account.info.code;
        if let Some(code) = code {
            contracts.insert(code.bytecode().0.clone());
        }
    }
    measurement.stop();

    let input = GuestInput {
        parent_state_trie: state_trie,
        parent_storage: storage,
        contracts: contracts.into_iter().map(Bytes).collect(),
        ancestor_headers,
        ..input
    };

    // Add the collected data to the input
    Ok(input)
}

/// Prepare the input for a Taiko chain
async fn prepare_taiko_chain_input(
    l1_chain_spec: &ChainSpec,
    taiko_chain_spec: &ChainSpec,
    block_number: u64,
    block: &Block,
    prover_data: TaikoProverData,
) -> RaikoResult<TaikoGuestInput> {
    let provider_l1 = RpcBlockDataProvider::new(&l1_chain_spec.rpc, block_number)?;

    // Decode the anchor tx to find out which L1 blocks we need to fetch
    let anchor_tx = match &block.transactions {
        BlockTransactions::Full(txs) => txs[0].clone(),
        _ => unreachable!(),
    };
    let anchor_call = decode_anchor(anchor_tx.input.as_ref())?;
    // The L1 blocks we need
    let l1_state_block_number = anchor_call.l1BlockId;
    let l1_inclusion_block_number = l1_state_block_number + 1;

    debug!(
        "anchor L1 block id: {:?}\nanchor L1 state root: {:?}",
        anchor_call.l1BlockId, anchor_call.l1StateRoot
    );

    // Get the L1 block in which the L2 block was included so we can fetch the DA data.
    // Also get the L1 state block header so that we can prove the L1 state root.
    let l1_blocks = provider_l1
        .get_blocks(&[
            (l1_inclusion_block_number, false),
            (l1_state_block_number, false),
        ])
        .await?;
    let (l1_inclusion_block, l1_state_block) = (&l1_blocks[0], &l1_blocks[1]);

    let l1_state_block_hash = l1_state_block.header.hash.ok_or_else(|| {
        RaikoError::Preflight("No L1 state block hash for the requested block".to_owned())
    })?;

    debug!("l1_state_root_block hash: {l1_state_block_hash:?}");

    let l1_inclusion_block_hash = l1_inclusion_block.header.hash.ok_or_else(|| {
        RaikoError::Preflight("No L1 inclusion block hash for the requested block".to_owned())
    })?;

    // Get the block proposal data
    let (proposal_tx, proposal_event) = get_block_proposed_event(
        provider_l1.provider(),
        taiko_chain_spec.clone(),
        l1_inclusion_block_hash,
        block_number,
    )
    .await?;

    // Fetch the tx data from either calldata or blobdata
    let (tx_data, blob_commitment, kzg_settings) = if proposal_event.meta.blobUsed {
        debug!("blob active");
        // Get the blob hashes attached to the propose tx
        let blob_hashes = proposal_tx.blob_versioned_hashes.unwrap_or_default();
        assert!(!blob_hashes.is_empty());
        // Currently the protocol enforces the first blob hash to be used
        let blob_hash = blob_hashes[0];
        // Get the blob data for this block
        let slot_id = block_time_to_block_slot(
            l1_inclusion_block.header.timestamp,
            l1_chain_spec.genesis_time,
            l1_chain_spec.seconds_per_slot,
        )?;
        let beacon_rpc_url: String = l1_chain_spec.beacon_rpc.clone().ok_or_else(|| {
            RaikoError::Preflight("Beacon RPC URL is required for Taiko chains".to_owned())
        })?;
        let blob = get_blob_data(&beacon_rpc_url, slot_id, blob_hash).await?;

        let kzg_settings = eip4844::MAINNET_KZG_TRUSTED_SETUP.as_ref().clone();
        let (proof, commitment) =
            eip4844::get_kzg_proof_commitment(&blob, &kzg_settings).map_err(|e| anyhow!(e))?;
        set_commitment_proof(&proof, &commitment).map_err(|e| anyhow!(e))?;

        (blob, Some(commitment), Some(kzg_settings))
    } else {
        // Get the tx list data directly from the propose transaction data
        let proposal_call = proposeBlockCall::abi_decode(&proposal_tx.input, false)
            .map_err(|_| RaikoError::Preflight("Could not decode proposeBlockCall".to_owned()))?;
        (proposal_call.txList.as_ref().to_owned(), None, None)
    };

    // Create the transactions from the proposed tx list
    let transactions = generate_transactions(
        taiko_chain_spec,
        proposal_event.meta.blobUsed,
        &tx_data,
        Some(anchor_tx.clone()),
    );
    // Do a sanity check using the transactions returned by the node
    assert!(
        transactions.len() >= block.transactions.len(),
        "unexpected number of transactions"
    );

    // Create the input struct without the block data set
    Ok(TaikoGuestInput {
        l1_header: l1_state_block.header.clone().try_into().unwrap(),
        tx_data,
        anchor_tx: serde_json::to_string(&anchor_tx).map_err(RaikoError::Serde)?,
        blob_commitment,
        kzg_settings,
        block_proposed: proposal_event,
        prover_data,
        skip_verify_blob: false,
    })
}

// block_time_to_block_slot returns the slots of the given timestamp.
fn block_time_to_block_slot(
    block_time: u64,
    genesis_time: u64,
    block_per_slot: u64,
) -> RaikoResult<u64> {
    if genesis_time == 0u64 {
        Err(RaikoError::Anyhow(anyhow!(
            "genesis time is 0, please check chain spec"
        )))
    } else if block_time < genesis_time {
        Err(RaikoError::Anyhow(anyhow!(
            "provided block_time precedes genesis time",
        )))
    } else {
        Ok((block_time - genesis_time) / block_per_slot)
    }
}

fn blob_to_bytes(blob_str: &str) -> Vec<u8> {
    match hex::decode(blob_str.to_lowercase().trim_start_matches("0x")) {
        Ok(b) => b,
        Err(_) => Vec::new(),
    }
}

fn preflight_blob_versioned_hash(blob_str: &str) -> [u8; 32] {
    use kzg::G1;
    let blob_bytes: Vec<u8> = hex::decode(blob_str.to_lowercase().trim_start_matches("0x"))
        .expect("Could not decode blob");
    let kzg_settings = MAINNET_KZG_TRUSTED_SETUP.as_ref();
    let blob = Blob::from_bytes(&blob_bytes).expect("Could not create blob");
    let commitment = eip4844::blob_to_kzg_commitment_rust(
        &eip4844::deserialize_blob_rust(&blob).expect("Could not deserialize blob"),
        kzg_settings,
    )
    .expect("Could not create kzg commitment from blob");
    let version_hash: [u8; 32] = commitment_to_version_hash(&commitment.to_bytes()).0;
    version_hash
}

async fn get_blob_data(
    beacon_rpc_url: &str,
    block_id: u64,
    blob_hash: FixedBytes<32>,
) -> Result<Vec<u8>> {
    if beacon_rpc_url.contains("blobscan.com") {
        get_blob_data_blobscan(beacon_rpc_url, block_id, blob_hash).await
    } else {
        get_blob_data_beacon(beacon_rpc_url, block_id, blob_hash).await
    }
}

async fn get_blob_data_beacon(
    beacon_rpc_url: &str,
    block_id: u64,
    blob_hash: FixedBytes<32>,
) -> Result<Vec<u8>> {
    // Blob data from the beacon chain
    // type Sidecar struct {
    // Index                    string                   `json:"index"`
    // Blob                     string                   `json:"blob"`
    // SignedBeaconBlockHeader  *SignedBeaconBlockHeader `json:"signed_block_header"`
    // KzgCommitment            string                   `json:"kzg_commitment"`
    // KzgProof                 string                   `json:"kzg_proof"`
    // CommitmentInclusionProof []string
    // `json:"kzg_commitment_inclusion_proof"` }
    #[derive(Clone, Debug, Deserialize, Serialize)]
    struct GetBlobData {
        pub index: String,
        pub blob: String,
        // pub signed_block_header: SignedBeaconBlockHeader, // ignore for now
        pub kzg_commitment: String,
        pub kzg_proof: String,
        //pub kzg_commitment_inclusion_proof: Vec<String>,
    }

    #[derive(Clone, Debug, Deserialize, Serialize)]
    struct GetBlobsResponse {
        pub data: Vec<GetBlobData>,
    }

    let url = format!(
        "{}/eth/v1/beacon/blob_sidecars/{block_id}",
        beacon_rpc_url.trim_end_matches('/'),
    );
    info!("Retrieve blob from {url}.");
    let response = reqwest::get(url.clone()).await?;
    if response.status().is_success() {
        let blobs: GetBlobsResponse = response.json().await?;
        assert!(!blobs.data.is_empty(), "blob data not available anymore");
        // Get the blob data for the blob storing the tx list
        let tx_blob = blobs
            .data
            .iter()
            .find(|blob| {
                // calculate from plain blob
                blob_hash == preflight_blob_versioned_hash(&blob.blob)
            })
            .cloned();
        assert!(tx_blob.is_some());
        Ok(blob_to_bytes(&tx_blob.unwrap().blob))
    } else {
        warn!(
            "Request {url} failed with status code: {}",
            response.status()
        );
        Err(anyhow::anyhow!(
            "Request failed with status code: {}",
            response.status()
        ))
    }
}

async fn get_blob_data_blobscan(
    beacon_rpc_url: &str,
    _block_id: u64,
    blob_hash: FixedBytes<32>,
) -> Result<Vec<u8>> {
    // https://api.blobscan.com/#/
    #[derive(Clone, Debug, Deserialize, Serialize)]
    struct BlobScanData {
        pub commitment: String,
        pub data: String,
    }

    let url = format!("{}/blobs/{blob_hash}", beacon_rpc_url.trim_end_matches('/'),);
    let response = reqwest::get(url.clone()).await?;
    if response.status().is_success() {
        let blob: BlobScanData = response.json().await?;
        Ok(blob_to_bytes(&blob.data))
    } else {
        println!(
            "Request {url} failed with status code: {}",
            response.status()
        );
        Err(anyhow::anyhow!(
            "Request failed with status code: {}",
            response.status()
        ))
    }
}

async fn get_block_proposed_event(
    provider: &ReqwestProvider,
    chain_spec: ChainSpec,
    block_hash: B256,
    l2_block_number: u64,
) -> Result<(AlloyRpcTransaction, BlockProposed)> {
    // Get the address that emitted the event
    let Some(l1_address) = chain_spec.l1_contract else {
        bail!("No L1 contract address in the chain spec");
    };

    // Get the event signature (value can differ between chains)
    let event_signature = BlockProposed::SIGNATURE_HASH;
    // Setup the filter to get the relevant events
    let filter = Filter::new()
        .address(l1_address)
        .at_block_hash(block_hash)
        .event_signature(event_signature);
    // Now fetch the events
    let logs = provider.get_logs(&filter).await?;

    // Run over the logs returned to find the matching event for the specified L2 block number
    // (there can be multiple blocks proposed in the same block and even same tx)
    for log in logs {
        let Some(log_struct) = Log::new(
            log.address(),
            log.topics().to_vec(),
            log.data().data.clone(),
        ) else {
            bail!("Could not create log")
        };
        let event = BlockProposed::decode_log(&log_struct, false)
            .map_err(|_| RaikoError::Anyhow(anyhow!("Could not decode log")))?;
        if event.blockId == raiko_lib::primitives::U256::from(l2_block_number) {
            let Some(log_tx_hash) = log.transaction_hash else {
                bail!("No transaction hash in the log")
            };
            let tx = provider
                .get_transaction_by_hash(log_tx_hash)
                .await
                .expect("couldn't query the propose tx")
                .expect("Could not find the propose tx");
            return Ok((tx, event.data));
        }
    }
    bail!("No BlockProposed event found for block {l2_block_number}");
}

fn get_transactions_from_block(block: &Block) -> RaikoResult<Vec<TxEnvelope>> {
    let mut transactions: Vec<TxEnvelope> = Vec::new();
    if !block.transactions.is_empty() {
        match &block.transactions {
            BlockTransactions::Full(txs) => {
                for tx in txs {
                    transactions.push(TxEnvelope::try_from(tx.clone()).unwrap());
                }
            },
            _ => unreachable!("Block is too old, please connect to an archive node or use a block that is at most 128 blocks old."),
        };
        assert!(
            transactions.len() == block.transactions.len(),
            "unexpected number of transactions"
        );
    }
    Ok(transactions)
}

#[cfg(test)]
mod test {
    use ethers_core::types::Transaction;
    use raiko_lib::{
        consts::{Network, SupportedChainSpecs},
        utils::decode_transactions,
    };

    use super::*;

    #[test]
    fn test_new_blob_decode() {
        let valid_blob_str = "\
            01000004b0f904adb8b502f8b283028c59188459682f008459682f028286b394\
            006700100000000000000000000000000001009980b844a9059cbb0000000000\
            0000000000000001670010000000000000000000000000000100990000000000\
            000000000000000000000000000000000000000000000000000001c080a0af40\
            093afa19e4b7256a209c71a902d33985c5655e580d5fbf36815e290b623177a0\
            19d4b4ccaa5497a47845016680c128b63e74e9d6a9756ebdeb2f78a65e0fa120\
            0001f802f901f483028c592e8459682f008459682f02832625a0941670010000\
            0b000000000000000000000000000280b90184fa233d0c000000000000000000\
            0000000000000000000000000000000000000000000000200000000000000000\
            000000000000000000000000000000000000000000007e7e0000000000000000\
            0000000014dc79964da2c08b23698b3d3cc7ca32193d99550000000000000000\
            0000000014dc79964da2c08b23698b3d3cc7ca32193d99550000000000000000\
            0000000000016700100000000000000000000000000001009900000000000000\
            0000000000000000000000000000000000000000000000000100000000000000\
            000000000000000000000000000000000000000000002625a000000000000000\
            0000000000000000000000000000000000000000000000000000000000000000\
            000000000000976ea74026e726554db657fa54763abd0c3a0aa9000000000000\
            0000000000000000000000000000000000000000000000000120000000000000\
            220000000000000000000000000000000000000000000000001243726f6e4a6f\
            102053656e64546f6b656e730000000000000000000000000000c080a0a99edd\
            2b13d5436cb0fe71b2ea4e69c2292fdc682ae54fe702cc36d6634dd0ba85a057\
            119f9297ca5ebd5402bd886405fe3aa8f8182438a9e56c1ef2a1ec0ae4a0acb9\
            00f802f901f483028c592f8459682f008459682f02832625a094167001000000\
            000000000000000000000000000280b90184fa233d0c00000000000000000000\
            0000000000000000000000000000000000000000000020000000000000000000\
            0000000000000000000000000000000000000000007e7e000000000000000000\
            00000014dc79964da2c08b23698b3d3cc7ca32193d9955000000000000000000\
            00000014dc79964da2c08b23698b3d3cc7ca32193d9955000000000000000000\
            0000000001670010000000000000000000000000000100990000000000000000\
            0000000000000000000000000000000000000000000000010000000000000000\
            0000000000000000000000000000000000000000002625a00000000000000000\
            0000000000000000000000000000000000000000000000000000000000000000\
            0000000000976ea74026e726554db657fa54763abd0c3a0aa900000000000000\
            0000000000000000000000000000000000000000000000012000000000000000\
            2000000000000000000000000000000000000000000000001243726f6e4a6f62\
            0053656e64546f6b656e730000000000000000000000000000c080a08f0a9757\
            35d78526f1339c69c2ed02df7a6d7cded10c74fb57398c11c1420526c2a0047f\
            003054d3d75d33120020872b6d5e0a4a05e47c50179bb9a8b866b7fb71b30000\
            0000000000000000000000000000000000000000000000000000000000000000\
            0000000000000000000000000000000000000000000000000000000000000000\
            0000000000000000000000000000000000000000000000000000000000000000\
            0000000000000000000000000000000000000000000000000000000000000000\
            0000000000000000000000000000000000000000000000000000000000000000\
            00000000000000000000000000000000";
        let blob_str = format!("{:0<262144}", valid_blob_str);
        let dec_blob = blob_to_bytes(&blob_str);
        println!("dec blob tx len: {:?}", dec_blob.len());
        let txs = decode_transactions(&dec_blob);
        println!("dec blob tx: {txs:?}");
    }

    #[ignore]
    #[test]
    fn test_slot_block_num_mapping() {
        let chain_spec = SupportedChainSpecs::default()
            .get_chain_spec(&Network::TaikoA7.to_string())
            .unwrap();
        let expected_slot = 1000u64;
        let second_per_slot = 12u64;
        let block_time = chain_spec.genesis_time + expected_slot * second_per_slot;
        let block_num =
            block_time_to_block_slot(block_time, chain_spec.genesis_time, second_per_slot)
                .expect("block time to slot failed");
        assert_eq!(block_num, expected_slot);

        assert!(block_time_to_block_slot(
            chain_spec.genesis_time - 10,
            chain_spec.genesis_time,
            second_per_slot
        )
        .is_err());
    }

    #[ignore]
    #[test]
    fn json_to_ethers_blob_tx() {
        let response = "{
            \"blockHash\":\"0xa61eea0256aa361dfd436be11b0e276470413fbbc34b3642fbbf3b5d8d72f612\",
		    \"blockNumber\":\"0x4\",
		    \"from\":\"0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266\",
		    \"gas\":\"0xf4240\",
		    \"gasPrice\":\"0x5e92e74e\",
		    \"maxFeePerGas\":\"0x8b772ea6\",
		    \"maxPriorityFeePerGas\":\"0x3b9aca00\",
		    \"maxFeePerBlobGas\":\"0x2\",
		    \"hash\":\"0xdb3b11250a2332cc4944fa8022836bd32da43c34d4f2e9e1b246cfdbc5b4c60e\",
		    \"input\":\"0x11762da2\",
		    \"nonce\":\"0x1\",
		    \"to\":\"0x5fbdb2315678afecb367f032d93f642f64180aa3\",
		    \"transactionIndex\":\"0x0\",
		    \"value\":\"0x0\",
		    \"type\":\"0x3\",
            \"accessList\":[],
		    \"chainId\":\"0x7e7e\",
            \"blobVersionedHashes\":[\"0x012d46373b7d1f53793cd6872e40e801f9af6860ecbdbaa2e28df25937618c6f\",\"0x0126d296b606f85b775b12b8b4abeb3bdb88f5a50502754d598537ae9b7fb947\"],
            \"v\":\"0x0\",
		    \"r\":\"0xaba289efba8ef610a5b3b70b72a42fe1916640f64d7112ec0b89087bbc8fff5f\",
		    \"s\":\"0x1de067d69b79d28d0a3bd179e332c85b93cedbd299d9e205398c073a59633dcf\",
		    \"yParity\":\"0x0\"
        }";
        let tx: Transaction = serde_json::from_str(response).unwrap();
        println!("tx: {tx:?}");
    }
}
