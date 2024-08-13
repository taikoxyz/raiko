use crate::{
    interfaces::{RaikoError, RaikoResult},
    provider::BlockDataProvider,
};
pub use alloy_primitives::*;
use alloy_rpc_types::Header;
use anyhow::{anyhow, ensure, Result};
use kzg_traits::{
    eip_4844::{blob_to_kzg_commitment_rust, Blob},
    G1,
};
use raiko_lib::{
    consts::ChainSpec,
    input::{BlobProofType, TaikoProverData},
    primitives::eip4844::{self, commitment_to_version_hash, KZG_SETTINGS},
};
use reth_primitives::Block;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};

pub mod hekla;
pub mod ontake;

pub struct PreflightData {
    pub block_number: u64,
    pub l1_chain_spec: ChainSpec,
    pub l1_inclusion_block_number: u64,
    pub taiko_chain_spec: ChainSpec,
    pub prover_data: TaikoProverData,
    pub blob_proof_type: BlobProofType,
}

impl PreflightData {
    pub fn new(
        block_number: u64,
        l1_inclusion_block_number: u64,
        l1_chain_spec: ChainSpec,
        taiko_chain_spec: ChainSpec,
        prover_data: TaikoProverData,
        blob_proof_type: BlobProofType,
    ) -> Self {
        Self {
            block_number,
            l1_chain_spec,
            l1_inclusion_block_number,
            taiko_chain_spec,
            prover_data,
            blob_proof_type,
        }
    }
}

pub async fn get_block_and_parent_data<BDP>(
    provider: &BDP,
    block_number: u64,
) -> RaikoResult<(Block, Block)>
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
    let parent_block = Block::try_from(parent_block.clone())
        .map_err(|e| RaikoError::Conversion(format!("Failed converting to reth block: {e}")))?;
    Ok((block, parent_block))
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
fn block_time_to_block_slot(
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

fn blob_to_bytes(blob_str: &str) -> Vec<u8> {
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
