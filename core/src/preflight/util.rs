use alloy_primitives::{hex, Log, B256};
use alloy_provider::{Provider, ReqwestProvider};
use alloy_rpc_types::{Filter, Header, Transaction as AlloyRpcTransaction};
use alloy_sol_types::{SolCall, SolEvent};
use anyhow::{anyhow, bail, ensure, Result};
use kzg_traits::{
    eip_4844::{blob_to_kzg_commitment_rust, Blob},
    G1,
};
use raiko_lib::{
    builder::{OptimisticDatabase, RethBlockBuilder},
    clear_line,
    consts::ChainSpec,
    inplace_print,
    input::{
        ontake::{BlockProposedV2, CalldataTxList},
        proposeBlockCall, BlobProofType, BlockProposed, BlockProposedFork, TaikoGuestInput,
        TaikoProverData,
    },
    primitives::eip4844::{self, commitment_to_version_hash, KZG_SETTINGS},
};
use reth_evm_ethereum::taiko::decode_anchor;
use reth_primitives::Block;
use reth_revm::primitives::SpecId;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};

use crate::{
    interfaces::{RaikoError, RaikoResult},
    provider::{db::ProviderDb, rpc::RpcBlockDataProvider, BlockDataProvider},
    require,
};

/// Optimize data gathering by executing the transactions multiple times so data can be requested in batches
pub async fn execute_txs<BDP>(builder: &mut RethBlockBuilder<ProviderDb<BDP>>) -> RaikoResult<()>
where
    BDP: BlockDataProvider,
{
    let max_iterations = 100;
    for num_iterations in 0.. {
        inplace_print(&format!("Execution iteration {num_iterations}..."));

        let Some(db) = builder.db.as_mut() else {
            return Err(RaikoError::Preflight("No db in builder".to_owned()));
        };
        db.optimistic = num_iterations + 1 < max_iterations;

        builder
            .execute_transactions(num_iterations + 1 < max_iterations)
            .map_err(|e| {
                RaikoError::Preflight(format!("Executing transactions in builder failed: {e}"))
            })?;

        let Some(db) = builder.db.as_mut() else {
            return Err(RaikoError::Preflight("No db in builder".to_owned()));
        };
        if db.fetch_data().await {
            clear_line();
            info!("State data fetched in {num_iterations} iterations");
            break;
        }
    }

    Ok(())
}

/// Prepare the input for a Taiko chain
pub async fn prepare_taiko_chain_input(
    l1_chain_spec: &ChainSpec,
    taiko_chain_spec: &ChainSpec,
    block_number: u64,
    l1_inclusion_block_number: Option<u64>,
    block: &Block,
    prover_data: TaikoProverData,
    blob_proof_type: BlobProofType,
) -> RaikoResult<TaikoGuestInput> {
    // Decode the anchor tx to find out which L1 blocks we need to fetch
    let anchor_tx = block
        .body
        .first()
        .ok_or_else(|| RaikoError::Preflight("No anchor tx in the block".to_owned()))?;
    let anchor_call = decode_anchor(anchor_tx.input())?;
    // The L1 blocks we need
    let l1_state_block_number = anchor_call.l1BlockId;
    let fork = taiko_chain_spec.active_fork(block.number, block.timestamp)?;
    let l1_inclusion_block_number = l1_inclusion_block_number.unwrap_or(l1_state_block_number + 1);

    debug!(
        "anchor L1 block id: {:?}\nanchor L1 state root: {:?}",
        anchor_call.l1BlockId, anchor_call.l1StateRoot
    );

    // Get the L1 block in which the L2 block was included so we can fetch the DA data.
    // Also get the L1 state block header so that we can prove the L1 state root.
    let provider_l1 = RpcBlockDataProvider::new(&l1_chain_spec.rpc, block_number)?;

    let (l1_inclusion_header, l1_state_header) = get_headers(
        &provider_l1,
        (l1_inclusion_block_number, l1_state_block_number),
    )
    .await?;

    let l1_state_block_hash = l1_state_header.hash.ok_or_else(|| {
        RaikoError::Preflight("No L1 state block hash for the requested block".to_owned())
    })?;

    debug!("l1_state_root_block hash: {l1_state_block_hash:?}");

    let l1_inclusion_block_hash = l1_inclusion_header.hash.ok_or_else(|| {
        RaikoError::Preflight("No L1 inclusion block hash for the requested block".to_owned())
    })?;

    // Get the block proposal data
    let (proposal_tx, block_proposed) = get_block_proposed_event(
        provider_l1.provider(),
        taiko_chain_spec.clone(),
        l1_inclusion_block_hash,
        block_number,
        fork,
    )
    .await?;

    // Fetch the tx data from either calldata or blobdata
    let (tx_data, blob_commitment) = if block_proposed.blob_used() {
        get_tx_data(
            proposal_tx.blob_versioned_hashes,
            l1_inclusion_header.timestamp,
            l1_chain_spec,
        )
        .await?
    } else {
        match fork {
            SpecId::ONTAKE => {
                // Get the tx list data directly from the propose block CalldataTxList event
                let calldata_txlist = get_calldata_txlist_event(
                    provider_l1.provider(),
                    taiko_chain_spec.clone(),
                    l1_inclusion_block_hash,
                    block_number,
                )
                .await?;
                (calldata_txlist.1.txList.as_ref().to_owned(), None)
            }
            _ => {
                // Get the tx list data directly from the propose transaction data
                let proposal_call = proposeBlockCall::abi_decode(&proposal_tx.input, false)
                    .map_err(|_| {
                        RaikoError::Preflight("Could not decode proposeBlockCall".to_owned())
                    })?;
                (proposal_call.txList.as_ref().to_owned(), None)
            }
        }
    };

    // Create the input struct without the block data set
    Ok(TaikoGuestInput {
        l1_header: l1_state_header.try_into().unwrap(),
        tx_data,
        anchor_tx: Some(anchor_tx.clone()),
        blob_commitment,
        block_proposed,
        prover_data,
        blob_proof_type,
    })
}

pub async fn get_tx_data(
    blob_versioned_hashes: Option<Vec<B256>>,
    timestamp: u64,
    chain_spec: &ChainSpec,
) -> RaikoResult<(Vec<u8>, Option<Vec<u8>>)> {
    debug!("blob active");
    // Get the blob hashes attached to the propose tx
    let blob_hashes = blob_versioned_hashes.unwrap_or_default();
    require(!blob_hashes.is_empty(), "blob hashes are empty")?;
    // Currently the protocol enforces the first blob hash to be used
    let blob_hash = blob_hashes[0];
    // Get the blob data for this block
    let slot_id = block_time_to_block_slot(
        timestamp,
        chain_spec.genesis_time,
        chain_spec.seconds_per_slot,
    )?;
    let beacon_rpc_url: String = chain_spec.beacon_rpc.clone().ok_or_else(|| {
        RaikoError::Preflight("Beacon RPC URL is required for Taiko chains".to_owned())
    })?;
    let blob = get_blob_data(&beacon_rpc_url, slot_id, blob_hash).await?;
    let commitment = eip4844::calc_kzg_proof_commitment(&blob).map_err(|e| anyhow!(e))?;

    Ok((blob, Some(commitment.to_vec())))
}

pub async fn get_calldata_txlist_event(
    provider: &ReqwestProvider,
    chain_spec: ChainSpec,
    block_hash: B256,
    l2_block_number: u64,
) -> Result<(AlloyRpcTransaction, CalldataTxList)> {
    // Get the address that emitted the event
    let Some(l1_address) = chain_spec.l1_contract else {
        bail!("No L1 contract address in the chain spec");
    };

    // Get the event signature (value can differ between chains)
    let event_signature = CalldataTxList::SIGNATURE_HASH;
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
        let event = CalldataTxList::decode_log(&log_struct, false)
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
    bail!("No BlockProposedV2 event found for block {l2_block_number}");
}

pub async fn get_block_proposed_event(
    provider: &ReqwestProvider,
    chain_spec: ChainSpec,
    block_hash: B256,
    l2_block_number: u64,
    fork: SpecId,
) -> Result<(AlloyRpcTransaction, BlockProposedFork)> {
    // Get the address that emitted the event
    let Some(l1_address) = chain_spec.l1_contract else {
        bail!("No L1 contract address in the chain spec");
    };

    // Get the event signature (value can differ between chains)
    let event_signature = match fork {
        SpecId::ONTAKE => BlockProposedV2::SIGNATURE_HASH,
        _ => BlockProposed::SIGNATURE_HASH,
    };
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
        let (block_id, data) = match fork {
            SpecId::ONTAKE => {
                let event = BlockProposedV2::decode_log(&log_struct, false)
                    .map_err(|_| RaikoError::Anyhow(anyhow!("Could not decode log")))?;
                (event.blockId, BlockProposedFork::Ontake(event.data))
            }
            _ => {
                let event = BlockProposed::decode_log(&log_struct, false)
                    .map_err(|_| RaikoError::Anyhow(anyhow!("Could not decode log")))?;
                (event.blockId, BlockProposedFork::Hekla(event.data))
            }
        };

        if block_id == raiko_lib::primitives::U256::from(l2_block_number) {
            let Some(log_tx_hash) = log.transaction_hash else {
                bail!("No transaction hash in the log")
            };
            let tx = provider
                .get_transaction_by_hash(log_tx_hash)
                .await
                .expect("couldn't query the propose tx")
                .expect("Could not find the propose tx");
            return Ok((tx, data));
        }
    }
    bail!("No BlockProposed event found for block {l2_block_number}");
}

pub async fn get_block_and_parent_data<BDP>(
    provider: &BDP,
    block_number: u64,
) -> RaikoResult<(Block, alloy_rpc_types::Block)>
where
    BDP: BlockDataProvider,
{
    // Get the block and the parent block
    let blocks = provider
        .get_blocks(&[(block_number, true), (block_number - 1, false)])
        .await?;
    let mut blocks = blocks.iter();
    let Some(block) = blocks.next() else {
        return Err(RaikoError::Preflight(
            "No block data for the requested block".to_owned(),
        ));
    };
    let Some(parent_block) = blocks.next() else {
        return Err(RaikoError::Preflight(
            "No parent block data for the requested block".to_owned(),
        ));
    };

    info!(
        "Processing block {:?} with hash: {:?}",
        block.header.number,
        block.header.hash.unwrap(),
    );
    debug!("block.parent_hash: {:?}", block.header.parent_hash);
    debug!("block gas used: {:?}", block.header.gas_used);
    debug!("block transactions: {:?}", block.transactions.len());

    // Convert the alloy block to a reth block
    let block = Block::try_from(block.clone())
        .map_err(|e| RaikoError::Conversion(format!("Failed converting to reth block: {e}")))?;
    Ok((block, parent_block.clone()))
}

pub async fn get_headers<BDP>(provider: &BDP, (a, b): (u64, u64)) -> RaikoResult<(Header, Header)>
where
    BDP: BlockDataProvider,
{
    // Get the block and the parent block
    let blocks = provider.get_blocks(&[(a, true), (b, false)]).await?;
    let mut blocks = blocks.iter();
    let Some(a) = blocks.next() else {
        return Err(RaikoError::Preflight(
            "No block data for the requested block".to_owned(),
        ));
    };
    let Some(b) = blocks.next() else {
        return Err(RaikoError::Preflight(
            "No block data for the requested block".to_owned(),
        ));
    };

    // Convert the alloy block to a reth block
    Ok((a.header.clone(), b.header.clone()))
}

// block_time_to_block_slot returns the slots of the given timestamp.
pub fn block_time_to_block_slot(
    block_time: u64,
    genesis_time: u64,
    block_per_slot: u64,
) -> RaikoResult<u64> {
    if genesis_time == 0 {
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

pub fn blob_to_bytes(blob_str: &str) -> Vec<u8> {
    hex::decode(blob_str.to_lowercase().trim_start_matches("0x")).unwrap_or_default()
}

fn calc_blob_versioned_hash(blob_str: &str) -> [u8; 32] {
    let blob_bytes: Vec<u8> = hex::decode(blob_str.to_lowercase().trim_start_matches("0x"))
        .expect("Could not decode blob");
    let blob = Blob::from_bytes(&blob_bytes).expect("Could not create blob");
    let commitment = blob_to_kzg_commitment_rust(
        &eip4844::deserialize_blob_rust(&blob).expect("Could not deserialize blob"),
        &KZG_SETTINGS.clone(),
    )
    .expect("Could not create kzg commitment from blob");
    let version_hash: [u8; 32] = commitment_to_version_hash(&commitment.to_bytes()).0;
    version_hash
}

async fn get_blob_data(beacon_rpc_url: &str, block_id: u64, blob_hash: B256) -> Result<Vec<u8>> {
    if beacon_rpc_url.contains("blobscan.com") {
        get_blob_data_blobscan(beacon_rpc_url, block_id, blob_hash).await
    } else {
        get_blob_data_beacon(beacon_rpc_url, block_id, blob_hash).await
    }
}

async fn get_blob_data_beacon(
    beacon_rpc_url: &str,
    block_id: u64,
    blob_hash: B256,
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
        ensure!(!blobs.data.is_empty(), "blob data not available anymore");
        // Get the blob data for the blob storing the tx list
        let tx_blob = blobs
            .data
            .iter()
            .find(|blob| {
                // calculate from plain blob
                blob_hash == calc_blob_versioned_hash(&blob.blob)
            })
            .cloned();
        ensure!(tx_blob.is_some());
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
    blob_hash: B256,
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
        error!(
            "Request {url} failed with status code: {}",
            response.status()
        );
        Err(anyhow::anyhow!(
            "Request failed with status code: {}",
            response.status()
        ))
    }
}
