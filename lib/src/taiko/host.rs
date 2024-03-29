use std::{io::Read, sync::Arc};

use anyhow::Result;
use c_kzg::{Blob, KzgCommitment};
use ethers_core::types::{Block, Transaction as EthersTransaction, H160, H256, U256};
use libflate::zlib::Decoder as zlibDecoder;
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
const BLOB_DATA_CAPACITY: usize = BLOB_FIELD_ELEMENT_NUM * BLOB_FIELD_ELEMENT_BYTES;
const BLOB_VERSION_OFFSET: usize = 1;
const BLOB_ENCODING_VERSION: u8 = 0;
const MAX_BLOB_DATA_SIZE: usize = (4 * 31 + 3) * 1024 - 4;

// decoding https://github.com/ethereum-optimism/optimism/blob/develop/op-service/eth/blob.go
fn decode_blob_data(blob_str: &str) -> Vec<u8> {
    let blob_buf: Vec<u8> = match hex::decode(blob_str.to_lowercase().trim_start_matches("0x")) {
        Ok(b) => b,
        Err(_) => return Vec::new(),
    };

    // check the version
    if blob_buf[BLOB_VERSION_OFFSET] != BLOB_ENCODING_VERSION {
        return Vec::new();
    }

    // decode the 3-byte big-endian length value into a 4-byte integer
    let output_len =
        ((blob_buf[2] as u32) << 16 | (blob_buf[3] as u32) << 8 | (blob_buf[4] as u32)) as usize;
    if output_len > MAX_BLOB_DATA_SIZE {
        return Vec::new();
    }

    // round 0 is special cased to copy only the remaining 27 bytes of the first field element
    // into the output due to version/length encoding already occupying its first 5 bytes.
    let mut output = [0; MAX_BLOB_DATA_SIZE];
    output[0..27].copy_from_slice(&blob_buf[5..32]);

    // now process remaining 3 field elements to complete round 0
    let mut opos: usize = 28; // current position into output buffer
    let mut ipos: usize = 32; // current position into the input blob
    let mut encoded_byte: [u8; 4] = [0; 4]; // buffer for the 4 6-bit chunks
    encoded_byte[0] = blob_buf[0];
    for encoded_byte_i in encoded_byte.iter_mut().skip(1) {
        (*encoded_byte_i, opos, ipos) =
            match decode_field_element(&blob_buf, opos, ipos, &mut output) {
                Ok(res) => res,
                Err(_) => return Vec::new(),
            }
    }
    opos = reassemble_bytes(opos, &encoded_byte, &mut output);

    // in each remaining round we decode 4 field elements (128 bytes) of the input into 127
    // bytes of output
    for _ in 1..1024 {
        if opos < output_len {
            for encoded_byte_j in &mut encoded_byte {
                // save the first byte of each field element for later re-assembly
                (*encoded_byte_j, opos, ipos) =
                    match decode_field_element(&blob_buf, opos, ipos, &mut output) {
                        Ok(res) => res,
                        Err(_) => return Vec::new(),
                    }
            }
            opos = reassemble_bytes(opos, &encoded_byte, &mut output)
        }
    }
    for otailing in output.iter().skip(output_len) {
        if *otailing != 0 {
            return Vec::new();
        }
    }
    for itailing in blob_buf.iter().take(BLOB_DATA_CAPACITY).skip(ipos) {
        if *itailing != 0 {
            return Vec::new();
        }
    }
    output[0..output_len].to_vec()
}

fn decode_field_element(
    b: &[u8],
    opos: usize,
    ipos: usize,
    output: &mut [u8],
) -> Result<(u8, usize, usize)> {
    // two highest order bits of the first byte of each field element should always be 0
    if b[ipos] & 0b1100_0000 != 0 {
        return Err(anyhow::anyhow!(
            "ErrBlobInvalidFieldElement: field element: {}",
            ipos
        ));
    }
    // copy(output[opos:], b[ipos+1:ipos+32])
    output[opos..opos + 31].copy_from_slice(&b[ipos + 1..ipos + 32]);
    Ok((b[ipos], opos + 32, ipos + 32))
}

fn reassemble_bytes(
    opos: usize,
    encoded_byte: &[u8; 4],
    output: &mut [u8; MAX_BLOB_DATA_SIZE],
) -> usize {
    // account for fact that we don't output a 128th byte
    let opos = opos - 1;
    let x = (encoded_byte[0] & 0b0011_1111) | ((encoded_byte[1] & 0b0011_0000) << 2);
    let y = (encoded_byte[1] & 0b0000_1111) | ((encoded_byte[3] & 0b0000_1111) << 4);
    let z = (encoded_byte[2] & 0b0011_1111) | ((encoded_byte[3] & 0b0011_0000) << 2);
    // put the re-assembled bytes in their appropriate output locations
    output[opos - 32] = z;
    output[opos - (32 * 2)] = y;
    output[opos - (32 * 3)] = x;
    opos
}

fn zlib_decompress_blob(blob: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = zlibDecoder::new(blob)?;
    let mut decoded_buf = Vec::new();
    decoder.read_to_end(&mut decoded_buf)?;
    Ok(decoded_buf)
}

fn calc_blob_versioned_hash(blob_str: &str) -> [u8; 32] {
    let blob_bytes: Vec<u8> =
        hex::decode(blob_str.to_lowercase().trim_start_matches("0x")).unwrap();
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
        let slot_id = block_time_to_block_slot(
            l2_fini_block.timestamp.as_u64(),
            l2_chain_spec.genesis_time,
            l2_chain_spec.seconds_per_slot,
        )?;
        let blobs = l1_provider.get_blob_data(slot_id)?;
        let tx_blobs: Vec<GetBlobData> = blobs
            .data
            .iter()
            .filter(|blob: &&GetBlobData| {
                // calculate from plain blob
                blob_hash.as_fixed_bytes() == &calc_blob_versioned_hash(&blob.blob)
            })
            .cloned()
            .collect::<Vec<GetBlobData>>();
        assert!(!tx_blobs.is_empty());
        let compressed_tx_list = decode_blob_data(&tx_blobs[0].blob);
        let decompressed_tx_list = zlib_decompress_blob(&compressed_tx_list).unwrap_or_default();
        (decompressed_tx_list, Some(from_ethers_h256(blob_hash)))
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
        sgx_verifier_address: l2_chain_spec.sgx_verifier_address.unwrap(),
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

// block_time_to_block_slot returns the slots of the given timestamp.
fn block_time_to_block_slot(
    block_time: u64,
    genesis_time: u64,
    block_per_slot: u64,
) -> Result<u64> {
    if block_time < genesis_time {
        Err(anyhow::Error::msg(
            "provided block_time precedes genesis time",
        ))
    } else {
        Ok((block_time - genesis_time) / block_per_slot)
    }
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

    #[ignore]
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
        // println!("valid blob: {:?}", valid_blob_str);
        let blob_str = format!("{:0<262144}", valid_blob_str);
        let dec_blob = decode_blob_data(&blob_str);
        println!("dec blob tx len: {:?}", dec_blob.len());
        let txs: Vec<Transaction> = rlp_decode_list(&dec_blob).unwrap();
        println!("dec blob tx: {:?}", txs);
        // assert_eq!(hex::encode(dec_blob), expected_dec_blob);
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

    #[test]
    fn test_slot_block_num_mapping() {
        let chain_spec = get_taiko_chain_spec("testnet");
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
    #[test]
    fn test_zlib_decoding() {
        let encoded_str = "789c13320100005a0047";
        let expect_decoded = "1234";
        let buf = zlib_decompress_blob(&hex::decode(encoded_str).unwrap()).unwrap();
        assert_eq!(hex::encode(buf), expect_decoded);
    }

    #[ignore]
    #[tokio::test]
    async fn test_fetch_and_decode_blob_tx() {
        let block_num = std::env::var("TAIKO_L2_BLOCK_NO")
            .unwrap_or("107".to_owned())
            .parse::<u64>()
            .unwrap();
        tokio::task::spawn_blocking(move || {
            let mut provider = new_provider(
                None,
                Some("http://35.202.137.144:8545".to_owned()),
                Some("http://35.202.137.144:3500".to_owned()),
            )
            .expect("bad provider");
            let blob_data = provider.get_blob_data(block_num).unwrap();
            println!("blob str len: {:?}", blob_data.data[0].blob.len());
            let blob_bytes = decode_blob_data(&blob_data.data[0].blob);
            // println!("blob byte len: {:?}", blob_bytes.len());
            println!("blob bytes {:?}", hex::encode(&blob_bytes));
            let decoded_buf = zlib_decompress_blob(&blob_bytes).unwrap();
            // rlp decode blob tx
            let txs: Vec<Transaction> = rlp_decode_list(&decoded_buf).unwrap();
            println!("blob tx: {:?}", txs);
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
