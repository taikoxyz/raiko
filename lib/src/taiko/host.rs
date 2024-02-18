use alloy_sol_types::abi::encode;
use anyhow::Result;
use ethers_core::types::{Block, Transaction as EthersTransaction, H160, H256, U256};
use reqwest;
use serde::Deserialize;
use sha2::{Sha256, Digest};
use tracing::info;
use zeth_primitives::{
    ethers::{from_ethers_h160, from_ethers_h256, from_ethers_u256},
    taiko::*,
    transactions::ethereum::EthereumTxEssence,
    withdrawal::Withdrawal,
    Address, B256,
};

use crate::{
    block_builder::{BlockBuilder, NetworkStrategyBundle},
    consts::ChainSpec,
    host::{
        provider::{new_provider, BlockQuery, ProposeQuery, Provider},
        Init,
    },
    input::Input,
    taiko::{precheck::rebuild_and_precheck_block, Layer},
};

#[derive(Debug)]
pub struct TaikoExtra {
    pub l1_hash: B256,
    pub l1_height: u64,
    pub l2_tx_list: Vec<u8>,
    pub prover: Address,
    pub graffiti: B256,
    pub l2_withdrawals: Vec<Withdrawal>,
    pub block_proposed: BlockProposed,
    pub l1_next_block: Block<EthersTransaction>,
    pub l2_fini_block: Block<EthersTransaction>,
}

#[allow(clippy::type_complexity)]
fn fetch_data(
    annotation: &str,
    cache_path: Option<String>,
    rpc_url: Option<String>,
    block_no: u64,
    layer: Layer,
) -> Result<(
    Box<dyn Provider>,
    Block<H256>,
    Block<EthersTransaction>,
    Input<EthereumTxEssence>,
)> {
    let mut provider = new_provider(cache_path, rpc_url)?;

    let fini_query = BlockQuery { block_no };
    match layer {
        Layer::L1 => {}
        Layer::L2 => {
            provider.batch_get_partial_blocks(&fini_query)?;
        }
    }
    // Fetch the initial block
    let init_block = provider.get_partial_block(&BlockQuery {
        block_no: block_no - 1,
    })?;

    info!(
        "Initial {} block: {:?} ({:?})",
        annotation,
        init_block.number.unwrap(),
        init_block.hash.unwrap()
    );

    // Fetch the finished block
    let fini_block = provider.get_full_block(&fini_query)?;

    info!(
        "Final {} block number: {:?} ({:?})",
        annotation,
        fini_block.number.unwrap(),
        fini_block.hash.unwrap()
    );
    info!("Transaction count: {:?}", fini_block.transactions.len());

    // Create input
    let input = Input {
        beneficiary: fini_block.author.map(from_ethers_h160).unwrap_or_default(),
        gas_limit: from_ethers_u256(fini_block.gas_limit),
        timestamp: from_ethers_u256(fini_block.timestamp),
        extra_data: fini_block.extra_data.0.clone().into(),
        mix_hash: from_ethers_h256(fini_block.mix_hash.unwrap()),
        transactions: fini_block
            .transactions
            .clone()
            .into_iter()
            .map(|tx| tx.try_into().unwrap())
            .collect(),
        withdrawals: fini_block
            .withdrawals
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(|w| w.try_into().unwrap())
            .collect(),
        parent_state_trie: Default::default(),
        parent_storage: Default::default(),
        contracts: vec![],
        parent_header: init_block.clone().try_into()?,
        ancestor_headers: vec![],
        base_fee_per_gas: from_ethers_u256(fini_block.base_fee_per_gas.unwrap_or_default()),
    };

    Ok((provider, init_block, fini_block, input))
}

fn execute_data<N: NetworkStrategyBundle<TxEssence = EthereumTxEssence>>(
    provider: Box<dyn Provider>,
    chain_spec: ChainSpec,
    init_block: Block<H256>,
    input: Input<EthereumTxEssence>,
    fini_block: Block<EthersTransaction>,
) -> Result<Init<EthereumTxEssence>> {
    // Create the provider DB
    let provider_db =
        crate::host::provider_db::ProviderDb::new(provider, init_block.number.unwrap().as_u64());
    // Create the block builder, run the transactions and extract the DB
    let mut builder = BlockBuilder::new(&chain_spec, input)
        .with_db(provider_db)
        .prepare_header::<N::HeaderPrepStrategy>()?
        .execute_transactions::<N::TxExecStrategy>()?;
    let provider_db = builder.mut_db().unwrap();

    info!("Gathering inclusion proofs ...");

    // Gather inclusion proofs for the initial and final state
    let init_proofs = provider_db.get_initial_proofs()?;
    let fini_proofs = provider_db.get_latest_proofs()?;

    // Gather proofs for block history
    let history_headers = provider_db.provider.batch_get_partial_blocks(&BlockQuery {
        block_no: fini_block.number.unwrap().as_u64(),
    })?;
    // ancestors == history - current - parent
    let ancestor_headers = if history_headers.len() > 2 {
        history_headers
            .into_iter()
            .rev()
            .skip(2)
            .map(|header| {
                header
                    .try_into()
                    .expect("Failed to convert ancestor headers")
            })
            .collect()
    } else {
        vec![]
    };

    info!("Saving provider cache ...");

    // Save the provider cache
    provider_db.get_provider().save()?;
    info!("Provider-backed execution is Done!");
    // assemble init
    let transactions = fini_block
        .transactions
        .clone()
        .into_iter()
        .map(|tx| tx.try_into().unwrap())
        .collect();
    let withdrawals = fini_block
        .withdrawals
        .clone()
        .unwrap_or_default()
        .into_iter()
        .map(|w| w.try_into().unwrap())
        .collect();

    let init = Init {
        db: provider_db.get_initial_db().clone(),
        init_block: init_block.try_into()?,
        init_proofs,
        fini_block: fini_block.try_into()?,
        fini_transactions: transactions,
        fini_withdrawals: withdrawals,
        fini_proofs,
        ancestor_headers,
    };
    Ok(init)
}

// type Sidecar struct {
// Index                    string                   `json:"index"`
// Blob                     string                   `json:"blob"`
// SignedBeaconBlockHeader  *SignedBeaconBlockHeader `json:"signed_block_header"`
// KzgCommitment            string                   `json:"kzg_commitment"`
// KzgProof                 string                   `json:"kzg_proof"`
// CommitmentInclusionProof []string
// `json:"kzg_commitment_inclusion_proof"` }

#[warn(dead_code)]
#[derive(Clone, Debug, Deserialize)]
struct GetBlobData {
    pub index: String,
    pub blob: String,
    // pub signed_block_header: String,
    pub kzg_commitment: String,
    pub kzg_proof: String,
    pub kzg_commitment_inclusion_proof: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct GetBlobsResponse {
    pub data: Vec<GetBlobData>,
}

// TODO: move to rpc provider
fn fetch_blob_data(
    l1_beacon_rpc_url: String,
    block_id: u64,
) -> Result<GetBlobsResponse, anyhow::Error> {
    // /eth/v1/beacon/blob_sidecars/{block_id}
    let url = format!(
        "{}/eth/v1/beacon/blob_sidecars/{}",
        l1_beacon_rpc_url, block_id
    );
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let response = reqwest::get(url.clone()).await?;

        if response.status().is_success() {
            println!("url: {:?}, response: {:?}", url, response);
            let blob_response: GetBlobsResponse = response.json().await?;
            Ok(blob_response)
        } else {
            Err(anyhow::anyhow!(
                "Request failed with status code: {}",
                response.status()
            ))
        }
    })
}

fn decode_blob_data(blob: &str) -> Vec<u8> {
    let origin_blob = hex::decode(blob).unwrap();
    assert!(origin_blob.len() == 4096 * 32);
    let mut chunk: Vec<Vec<u8>> = Vec::new();
    let mut lastSegFound = false;
    for i in (0..4096).rev() {
        let segment = &origin_blob[i * 32..(i + 1) * 32];
        if segment.iter().any(|&x| x != 0) || lastSegFound {
            chunk.push(segment.to_vec());
            lastSegFound = true;
        }
    }
    chunk.reverse();
    chunk.iter().flatten().cloned().collect()
}

// TODO: use reth::primitives::eip4844::kzg_to_versioned_hash
fn kzg_to_versioned_hash(commitment: &String) -> [u8; 32] {
    let commit_bytes = hex::decode(commitment.to_lowercase().trim_start_matches("0x")).unwrap();
    // let mut commit_hash = keccak(commit_bytes);
    let mut hasher = Sha256::new();
    hasher.update(commit_bytes);
    let mut commit_hash: [u8; 32] = hasher.finalize().to_vec().try_into().unwrap();
    println!("commit_hash: {:?}", hex::encode(commit_hash));
    commit_hash[0] = 0x01;
    commit_hash
}

#[allow(clippy::too_many_arguments)]
pub fn get_taiko_initial_data<N: NetworkStrategyBundle<TxEssence = EthereumTxEssence>>(
    l1_cache_path: Option<String>,
    _l1_chain_spec: ChainSpec,
    l1_rpc_url: Option<String>,
    l1_beacon_rpc_url: Option<String>,
    prover: Address,
    l2_cache_path: Option<String>,
    l2_chain_spec: ChainSpec,
    l2_rpc_url: Option<String>,
    l2_block_no: u64,
    graffiti: B256,
) -> Result<(Init<EthereumTxEssence>, TaikoExtra)> {
    let (l2_provider, l2_init_block, mut l2_fini_block, l2_input) =
        fetch_data("L2", l2_cache_path, l2_rpc_url, l2_block_no, Layer::L2)?;
    // Get anchor call parameters
    let anchorCall {
        l1Hash: anchor_l1_hash,
        l1StateRoot: anchor_l1_state_root,
        l1BlockId: l1_block_no,
        parentGasUsed: l2_parent_gas_used,
    } = decode_anchor_call_args(&l2_fini_block.transactions[0].input)?;

    let (mut l1_provider, _l1_init_block, l1_fini_block, _l1_input) =
        fetch_data("L1", l1_cache_path, l1_rpc_url, l1_block_no, Layer::L1)?;

    let (propose_tx, block_metadata) = l1_provider.get_blob_tx_propose(&ProposeQuery {
        l1_contract: H160::from_slice(l2_chain_spec.l1_contract.unwrap().as_slice()),
        l1_block_no: l1_block_no + 1,
        l2_block_no,
    })?;

    let l1_next_block = l1_provider.get_full_block(&BlockQuery {
        block_no: l1_block_no + 1,
    })?;

    // save l1 data
    l1_provider.save()?;

    let proposeBlockCall {
        params: propose_params,
        txList: l2_tx_list,
    } = decode_propose_block_call_args(&propose_tx.input)?;

    // blobUsed == (txList.length == 0) according to TaikoL1
    let blob_used = l2_tx_list.is_empty();
    let l2_tx_list_blob = if blob_used {
        let BlockParams {
            assignedProver: _,
            extraData: _,
            blobHash: proposed_blob_hash,
            txListByteOffset: offset,
            txListByteSize: size,
            cacheBlobForReuse: _,
            parentMetaHash: _,
            hookCalls: _,
        } = decode_propose_block_call_params(&propose_params)
            .expect("valid propose_block_call_params");

        let blob_hash = proposed_blob_hash;
        // let blob_hash = blob_hashs[0];
        // let blob_hashs = propose_tx.blob_versioned_hashes();
        // TODO: multiple blob hash support
        // assert(blob_hashs.len() == 1);
        // if proposed_blob_hash != [0; 32] {
        //     assert_eq!(proposed_blob_hash, blob_hash);
        // }

        // todo get blob from l1_beacon_rpc_url
        let blobs = fetch_blob_data(l1_beacon_rpc_url.unwrap(), l1_block_no + 1)?;
        // assume params has the right blobHash now
        let tx_blobs: Vec<GetBlobData> = blobs
            .data
            .iter()
            .filter(|blob| blob_hash == kzg_to_versioned_hash(&blob.kzg_commitment))
            .cloned()
            .collect::<Vec<GetBlobData>>();
        let blob_data = decode_blob_data(&tx_blobs[0].blob);
        blob_data.as_slice()[offset as usize..(offset + size) as usize].to_vec()
    } else {
        l2_tx_list
    };

    // 1. check l2 parent gas used
    if l2_init_block.gas_used != U256::from(l2_parent_gas_used) {
        return Err(anyhow::anyhow!(
            "parent gas used mismatch, expect: {}, got: {}",
            l2_init_block.gas_used,
            l2_parent_gas_used
        ));
    }
    // 2. check l1 state root
    if anchor_l1_state_root != from_ethers_h256(l1_fini_block.state_root) {
        return Err(anyhow::anyhow!(
            "l1 state root mismatch, expect: {}, got: {}",
            anchor_l1_state_root,
            from_ethers_h256(l1_fini_block.state_root)
        ));
    }
    // 3. check l1 block hash
    if Some(anchor_l1_hash) != l1_fini_block.hash.map(from_ethers_h256) {
        return Err(anyhow::anyhow!(
            "l1 block hash mismatch, expect: {}, got: {:?}",
            anchor_l1_hash,
            l1_fini_block.hash
        ));
    }

    let extra = TaikoExtra {
        l1_hash: anchor_l1_hash,
        l1_height: l1_block_no,
        l2_tx_list: l2_tx_list_blob.to_vec(),
        prover,
        graffiti,
        l2_withdrawals: l2_input.withdrawals.clone(),
        block_proposed: block_metadata,
        l1_next_block,
        l2_fini_block: l2_fini_block.clone(),
    };

    // rebuild transaction list by tx_list from l1 contract
    rebuild_and_precheck_block(&l2_chain_spec, &mut l2_fini_block, &extra)?;

    // execute transactions and get states
    let init = execute_data::<N>(
        l2_provider,
        l2_chain_spec,
        l2_init_block,
        l2_input,
        l2_fini_block,
    )?;
    Ok((init, extra))
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_fetch_blob_data() {
        let blob_data = fetch_blob_data("http://localhost:3500".to_string(), 5).unwrap();
        println!("blob len: {:?}", blob_data.data[0].blob.len());
        println!("blob commitment: {:?}", blob_data.data[0].kzg_commitment);
        let blob_hash = kzg_to_versioned_hash(&blob_data.data[0].kzg_commitment);
        println!("blob hash {:?}", hex::encode(blob_hash));
        // assert_eq!(blob_data.data.len(), 1);
    }

    #[test]
    fn test_decode_propose_block_call_params() {
        let mut l1_provider = new_provider(None, Some("http://localhost:3500".to_owned())).expect("valid provider");

        let (propose_tx, block_metadata) = l1_provider.get_blob_tx_propose(&ProposeQuery {
            l1_contract: H160::default(),
            l1_block_no: 15,
            l2_block_no: 0,
        }).expect("valid propose_tx");

        println!("propose_tx: {:?}", propose_tx);
    }
}
