use anyhow::Result;
use ethers_core::types::{Block, Transaction as EthersTransaction, H160, H256, U256};
use reth_primitives::eip4844::kzg_to_versioned_hash;
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
        provider::{new_provider, BlockQuery, GetBlobData, ProposeQuery, Provider},
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
    pub tx_blob_hash: Option<B256>,
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
    beacon_rpc_url: Option<String>,
    block_no: u64,
    layer: Layer,
) -> Result<(
    Box<dyn Provider>,
    Block<H256>,
    Block<EthersTransaction>,
    Input<EthereumTxEssence>,
)> {
    let mut provider = new_provider(cache_path, rpc_url, beacon_rpc_url)?;

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

// // TODO: move to rpc provider
// fn fetch_blob_data(
//     l1_beacon_rpc_url: String,
//     block_id: u64,
// ) -> Result<GetBlobsResponse, anyhow::Error> {
//     // /eth/v1/beacon/blob_sidecars/{block_id}
//     let url = format!(
//         "{}/eth/v1/beacon/blob_sidecars/{}",
//         l1_beacon_rpc_url, block_id
//     );
//     let rt = tokio::runtime::Runtime::new().unwrap();
//     rt.block_on(async {
//         let response = reqwest::get(url.clone()).await?;

//         if response.status().is_success() {
//             println!("url: {:?}, response: {:?}", url, response);
//             let blob_response: GetBlobsResponse = response.json().await?;
//             Ok(blob_response)
//         } else {
//             Err(anyhow::anyhow!(
//                 "Request failed with status code: {}",
//                 response.status()
//             ))
//         }
//     })
// }

fn decode_blob_data(blob: &str) -> Vec<u8> {
    let origin_blob = hex::decode(blob.to_lowercase().trim_start_matches("0x")).unwrap();
    assert!(origin_blob.len() == 4096 * 32);
    let mut chunk: Vec<Vec<u8>> = Vec::new();
    let mut last_seg_found = false;
    for i in (0..4096).rev() {
        let segment = &origin_blob[i * 32..(i + 1) * 32];
        if segment.iter().any(|&x| x != 0) || last_seg_found {
            chunk.push(segment.to_vec());
            last_seg_found = true;
        }
    }
    chunk.reverse();
    chunk.iter().flatten().cloned().collect()
}

fn calc_blob_hash(commitment: &String) -> [u8; 32] {
    let commit_bytes = hex::decode(commitment.to_lowercase().trim_start_matches("0x")).unwrap();
    let kzg_commit = c_kzg::KzgCommitment::from_bytes(&commit_bytes).unwrap();
    let version_hash: [u8; 32] = kzg_to_versioned_hash(kzg_commit).0.clone();
    version_hash
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
    let (l2_provider, l2_init_block, mut l2_fini_block, l2_input) = fetch_data(
        "L2",
        l2_cache_path,
        l2_rpc_url,
        None,
        l2_block_no,
        Layer::L2,
    )?;
    // Get anchor call parameters
    let anchorCall {
        l1Hash: anchor_l1_hash,
        l1StateRoot: anchor_l1_state_root,
        l1BlockId: l1_block_no,
        parentGasUsed: l2_parent_gas_used,
    } = decode_anchor_call_args(&l2_fini_block.transactions[0].input)?;

    let (mut l1_provider, _l1_init_block, l1_fini_block, _l1_input) = fetch_data(
        "L1",
        l1_cache_path,
        l1_rpc_url,
        l1_beacon_rpc_url,
        l1_block_no,
        Layer::L1,
    )?;

    let (propose_tx, block_metadata) = l1_provider.get_propose(&ProposeQuery {
        l1_contract: H160::from_slice(l2_chain_spec.l1_contract.unwrap().as_slice()),
        l1_block_no: l1_block_no + 1,
        l2_block_no,
    })?;

    let l1_next_block = l1_provider.get_full_block(&BlockQuery {
        block_no: l1_block_no + 1,
    })?;

    let proposeBlockCall {
        params: propose_params,
        txList: l2_tx_list,
    } = decode_propose_block_call_args(&propose_tx.input)?;

    // blobUsed == (txList.length == 0) according to TaikoL1
    let blob_used = l2_tx_list.is_empty();
    let (l2_tx_list_blob, tx_blob_hash) = if blob_used {
        let BlockParams {
            assignedProver: _,
            coinbase: _,
            extraData: _,
            blobHash: _proposed_blob_hash,
            txListByteOffset: offset,
            txListByteSize: size,
            cacheBlobForReuse: _,
            parentMetaHash: _,
            hookCalls: _,
        } = decode_propose_block_call_params(&propose_params)
            .expect("valid propose_block_call_params");

        let blob_hashs = propose_tx.blob_versioned_hashes.unwrap();
        // TODO: multiple blob hash support
        assert!(blob_hashs.len() == 1);
        let blob_hash = blob_hashs[0];
        // TODO: check _proposed_blob_hash with blob_hash if _proposed_blob_hash is not None

        let blobs = l1_provider.get_blob_data(l1_block_no + 1)?;
        let tx_blobs: Vec<GetBlobData> = blobs
            .data
            .iter()
            .filter(|blob| blob_hash.as_fixed_bytes() == &calc_blob_hash(&blob.kzg_commitment))
            .cloned()
            .collect::<Vec<GetBlobData>>();
        let blob_data = decode_blob_data(&tx_blobs[0].blob);
        (
            blob_data.as_slice()[offset as usize..(offset + size) as usize].to_vec(),
            Some(from_ethers_h256(blob_hash)),
        )
    } else {
        (l2_tx_list, None)
    };

    // save l1 data
    l1_provider.save()?;

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
        tx_blob_hash: tx_blob_hash,
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
    use ethers_core::types::Transaction;

    use super::*;

    #[tokio::test]
    async fn test_fetch_blob_data_and_hash() {
        tokio::task::spawn_blocking(|| {
            let mut provider = new_provider(
                None,
                Some("https://l1rpc.internal.taiko.xyz".to_owned()),
                Some("https://l1beacon.internal.taiko.xyz/".to_owned()),
            )
            .expect("bad provider");
            // let blob_data = fetch_blob_data("http://localhost:3500".to_string(), 5).unwrap();
            let blob_data = provider.get_blob_data(6093).unwrap();
            println!("blob len: {:?}", blob_data.data[0].blob.len());
            println!("blob commitment: {:?}", blob_data.data[0].kzg_commitment);
            let blob_hash = calc_blob_hash(&blob_data.data[0].kzg_commitment);
            println!("blob hash {:?}", hex::encode(blob_hash));
        }).await.unwrap();
    }

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
        println!("tx: {:?}", tx);
    }
}
