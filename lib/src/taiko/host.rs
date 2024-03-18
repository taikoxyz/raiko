use std::sync::Arc;

use anyhow::Result;
use c_kzg::{Blob, KzgCommitment};
use ethers_core::types::{Block, Transaction as EthersTransaction, H160, H256, U256};
use reth_primitives::{
    constants::eip4844::MAINNET_KZG_TRUSTED_SETUP, eip4844::kzg_to_versioned_hash,
};
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
    pub chain_id: u64,
    pub sgx_verifier_address: Address,
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

const BLOB_FIELD_ELEMENT_NUM: usize = 4096;
const BLOB_FIELD_ELEMENT_BYTES: usize = 32;
const BLOB_DATA_LEN: usize = BLOB_FIELD_ELEMENT_NUM * BLOB_FIELD_ELEMENT_BYTES;

fn decode_blob_data(blob: &str) -> Vec<u8> {
    let origin_blob = hex::decode(blob.to_lowercase().trim_start_matches("0x")).unwrap();
    let header: U256 = U256::from_big_endian(&origin_blob[0..BLOB_FIELD_ELEMENT_BYTES]); // first element is the length
    let expected_len = header.as_usize();

    assert!(origin_blob.len() == BLOB_DATA_LEN);
    // the first 32 bytes is the length of the blob
    // every first 1 byte is reserved.
    assert!(expected_len <= (BLOB_FIELD_ELEMENT_NUM - 1) * (BLOB_FIELD_ELEMENT_BYTES - 1));
    let mut chunk: Vec<Vec<u8>> = Vec::new();
    let mut decoded_len = 0;
    let mut i = 1;
    while decoded_len < expected_len && i < BLOB_FIELD_ELEMENT_NUM {
        let segment_len = if expected_len - decoded_len >= 31 {
            31
        } else {
            expected_len - decoded_len
        };
        let segment = &origin_blob
            [i * BLOB_FIELD_ELEMENT_BYTES + 1..i * BLOB_FIELD_ELEMENT_BYTES + 1 + segment_len];
        i += 1;
        decoded_len += segment_len;
        chunk.push(segment.to_vec());
    }
    let blob_vec: Vec<u8> = chunk.iter().flatten().cloned().collect();
    assert!(decoded_len == expected_len && blob_vec.len() == expected_len);
    blob_vec
}

fn calc_blob_versioned_hash(blob_str: &str) -> [u8; 32] {
    let blob_bytes = hex::decode(blob_str.to_lowercase().trim_start_matches("0x")).unwrap();
    let kzg_settings = Arc::clone(&*MAINNET_KZG_TRUSTED_SETUP);
    let blob = Blob::from_bytes(&blob_bytes).unwrap();
    let kzg_commit = KzgCommitment::blob_to_kzg_commitment(&blob, &kzg_settings).unwrap();
    let version_hash: [u8; 32] = kzg_to_versioned_hash(kzg_commit).0;
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
        txList: l2_tx_list, ..
    } = decode_propose_block_call_args(&propose_tx.input)?;

    // blobUsed == (txList.length == 0) according to TaikoL1
    let blob_used = l2_tx_list.is_empty();
    let (l2_tx_list_blob, tx_blob_hash) = if blob_used {
        let blob_hashs = propose_tx.blob_versioned_hashes.unwrap();
        assert!(blob_hashs.len() == 1);
        let blob_hash = blob_hashs[0];

        let blobs = l1_provider.get_blob_data(l1_block_no + 1)?;
        let tx_blobs: Vec<GetBlobData> = blobs
            .data
            .iter()
            .filter(|blob: &&GetBlobData| {
                // calculate from plain blob
                blob_hash.as_fixed_bytes() == &calc_blob_versioned_hash(&blob.blob)
            })
            .cloned()
            .collect::<Vec<GetBlobData>>();
        let blob_data = decode_blob_data(&tx_blobs[0].blob);
        (blob_data, Some(from_ethers_h256(blob_hash)))
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
        tx_blob_hash,
        prover,
        graffiti,
        l2_withdrawals: l2_input.withdrawals.clone(),
        block_proposed: block_metadata,
        l1_next_block,
        l2_fini_block: l2_fini_block.clone(),
        chain_id: l2_chain_spec.chain_id(),
        sgx_verifier_address: *SGX_VERIFIER_ADDRESS,
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
    use std::sync::Arc;

    use c_kzg::{Blob, KzgCommitment};
    use ethers_core::types::Transaction;
    use reth_primitives::{
        constants::eip4844::MAINNET_KZG_TRUSTED_SETUP,
        eip4844::kzg_to_versioned_hash,
        revm_primitives::kzg::{parse_kzg_trusted_setup, KzgSettings},
    };

    use super::*;
    use crate::{consts::get_taiko_chain_spec, taiko::utils::rlp_decode_list};

    fn calc_commit_versioned_hash(commitment: &str) -> [u8; 32] {
        let commit_bytes = hex::decode(commitment.to_lowercase().trim_start_matches("0x")).unwrap();
        let kzg_commit = c_kzg::KzgCommitment::from_bytes(&commit_bytes).unwrap();
        let version_hash: [u8; 32] = kzg_to_versioned_hash(kzg_commit).0;
        version_hash
    }

    #[test]
    fn test_parse_kzg_trusted_setup() {
        // check if file exists
        let b_file_exists = std::path::Path::new("../kzg_parsed_trust_setup").exists();
        assert!(b_file_exists);
        // open file as lines of strings
        let kzg_trust_setup_str = std::fs::read_to_string("../kzg_parsed_trust_setup").unwrap();
        let (g1, g2) = parse_kzg_trusted_setup(&kzg_trust_setup_str)
            .map_err(|e| {
                println!("error: {:?}", e);
                e
            })
            .unwrap();
        println!("g1: {:?}", g1.0.len());
        println!("g2: {:?}", g2.0.len());
    }

    #[test]
    fn test_blob_to_kzg_commitment() {
        // check if file exists
        let b_file_exists = std::path::Path::new("../kzg_parsed_trust_setup").exists();
        assert!(b_file_exists);
        // open file as lines of strings
        let kzg_trust_setup_str = std::fs::read_to_string("../kzg_parsed_trust_setup").unwrap();
        let (g1, g2) = parse_kzg_trusted_setup(&kzg_trust_setup_str)
            .map_err(|e| {
                println!("error: {:?}", e);
                e
            })
            .unwrap();
        let kzg_settings = KzgSettings::load_trusted_setup(&g1.0, &g2.0).unwrap();
        let blob = [0u8; 131072].into();
        let kzg_commit = KzgCommitment::blob_to_kzg_commitment(&blob, &kzg_settings).unwrap();
        assert_eq!(
            kzg_to_versioned_hash(kzg_commit).to_string(),
            "0x010657f37554c781402a22917dee2f75def7ab966d7b770905398eba3c444014"
        );
    }

    #[test]
    fn test_new_blob_decode() {
        let valid_blob_str = "\
            00000000000000000000000000000000000000000000000000000000000000e2\
            00f8b9b8b702f8b483028c59821cca8459682f008459682f028286b394016700\
            00100000000000000000000000000001009980b844a9059cbb00000000000000\
            0000000000000167001000000000000000000000000000010099000000000000\
            000000000000000000000000000000000000000000000000000001c080a02d55\
            004e149d15575030f271403a3b359cd9d5df8acb47ae7df5845aadc54b1ee2a0\
            0039b7ce8e803c443d8fd33679948fbd0a485d88b6a55812a53d9a03a9221421\
            0000000000000000000000000000000000000000000000000000000000000000\
            00000000000000000000";
        // println!("valid blob: {:?}", valid_blob_str);
        let expected_dec_blob = "\
              f8b9b8b702f8b483028c59821cca8459682f008459682f028286b394016700\
              100000000000000000000000000001009980b844a9059cbb00000000000000\
              00000000000167001000000000000000000000000000010099000000000000\
              0000000000000000000000000000000000000000000000000001c080a02d55\
              4e149d15575030f271403a3b359cd9d5df8acb47ae7df5845aadc54b1ee2a0\
              39b7ce8e803c443d8fd33679948fbd0a485d88b6a55812a53d9a03a9221421\
              00000000000000000000000000000000000000000000000000000000000000\
              000000000000000000";

        let blob_str = format!("{:0<262144}", valid_blob_str);
        let dec_blob = decode_blob_data(&blob_str);
        println!("dec blob tx len: {:?}", dec_blob.len());
        println!("dec blob tx: {:?}", dec_blob);
        assert_eq!(hex::encode(dec_blob), expected_dec_blob);
    }

    #[test]
    fn test_c_kzg_lib_commitment() {
        // check c-kzg mainnet trusted setup is ok
        let kzg_settings = Arc::clone(&*MAINNET_KZG_TRUSTED_SETUP);
        let blob = [0u8; 131072].into();
        let kzg_commit = KzgCommitment::blob_to_kzg_commitment(&blob, &kzg_settings).unwrap();
        assert_eq!(
            kzg_to_versioned_hash(kzg_commit).to_string(),
            "0x010657f37554c781402a22917dee2f75def7ab966d7b770905398eba3c444014"
        );
    }

    #[ignore]
    #[tokio::test]
    async fn test_propose_block() {
        tokio::task::spawn_blocking(|| {
            let l2_chain_spec = get_taiko_chain_spec("internal_devnet_a");
            let mut l1_provider = new_provider(
                None,
                Some("https://localhost:8545".to_owned()),
                Some("https://localhost:3500/".to_owned()),
            )
            .expect("bad provider");
            let (propose_tx, block_metadata) = l1_provider
                .get_propose(&ProposeQuery {
                    l1_contract: H160::from_slice(l2_chain_spec.l1_contract.unwrap().as_slice()),
                    l1_block_no: 6093,
                    l2_block_no: 1000,
                })
                .expect("bad get_propose");
            println!("propose_tx: {:?}", propose_tx);
            println!("block_metadata: {:?}", block_metadata);
        })
        .await
        .unwrap();
    }

    #[ignore]
    #[tokio::test]
    async fn test_fetch_blob_data_and_hash() {
        tokio::task::spawn_blocking(|| {
            let mut provider = new_provider(
                None,
                Some("https://l1rpc.internal.taiko.xyz/".to_owned()),
                Some("https://l1beacon.internal.taiko.xyz/".to_owned()),
            )
            .expect("bad provider");
            // let blob_data = fetch_blob_data("http://localhost:3500".to_string(), 5).unwrap();
            let blob_data = provider.get_blob_data(17138).unwrap();
            println!("blob len: {:?}", blob_data.data[0].blob.len());
            let dec_blob = decode_blob_data(&blob_data.data[0].blob);
            println!("dec blob tx: {:?}", dec_blob.len());

            println!("blob commitment: {:?}", blob_data.data[0].kzg_commitment);
            let blob_hash = calc_commit_versioned_hash(&blob_data.data[0].kzg_commitment);
            println!("blob hash {:?}", hex::encode(blob_hash));
        })
        .await
        .unwrap();
    }

    #[ignore]
    #[tokio::test]
    async fn test_fetch_and_verify_blob_data() {
        tokio::task::spawn_blocking(|| {
            let mut provider = new_provider(
                None,
                Some("https://l1rpc.internal.taiko.xyz".to_owned()),
                Some("https://l1beacon.internal.taiko.xyz".to_owned()),
            )
            .expect("bad provider");
            let blob_data = provider.get_blob_data(168).unwrap();
            let blob_bytes: [u8; 4096 * 32] = hex::decode(
                blob_data.data[0]
                    .blob
                    .to_lowercase()
                    .trim_start_matches("0x"),
            )
            .unwrap()
            .try_into()
            .unwrap();
            let blob: Blob = blob_bytes.into();
            let kzg_settings = Arc::clone(&*MAINNET_KZG_TRUSTED_SETUP);
            let kzg_commit: KzgCommitment =
                KzgCommitment::blob_to_kzg_commitment(&blob, &kzg_settings).unwrap();
            assert_eq!(
                "0x".to_owned() + &kzg_commit.as_hex_string(),
                blob_data.data[0].kzg_commitment
            );
            println!("blob commitment: {:?}", blob_data.data[0].kzg_commitment);
            let calc_versioned_hash = calc_commit_versioned_hash(&blob_data.data[0].kzg_commitment);
            println!("blob hash {:?}", hex::encode(calc_versioned_hash));
        })
        .await
        .unwrap();
    }

    #[ignore]
    #[tokio::test]
    async fn test_fetch_and_decode_blob_tx() {
        tokio::task::spawn_blocking(|| {
            let mut provider = new_provider(
                None,
                Some("https://l1rpc.internal.taiko.xyz".to_owned()),
                Some("https://l1beacon.internal.taiko.xyz".to_owned()),
            )
            .expect("bad provider");
            let blob_data = provider.get_blob_data(168).unwrap();
            let blob_bytes = decode_blob_data(&blob_data.data[0].blob);
            // println!("blob byte len: {:?}", blob_bytes.len());
            println!("blob bytes {:?}", blob_bytes);
            // rlp decode blob tx
            let txs: Vec<Transaction> = rlp_decode_list(&blob_bytes).unwrap();
            println!("blob tx: {:?}", txs.len());
        })
        .await
        .unwrap();
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
        println!("tx: {:?}", tx);
    }
}
