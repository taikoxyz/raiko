use alloy_rlp::Decodable;
use anyhow::Result;
use core::cmp::{max, min};
use libflate::zlib::{Decoder as zlibDecoder, Encoder as zlibEncoder};
use reth_primitives::{Address, Block, TransactionSigned};
use std::cmp::max as std_max;
use std::io::{Read, Write};
use tracing::{debug, error, warn};

use crate::consts::{ChainSpec, Network};
use crate::input::{BlockProposedFork, GuestBatchInput, GuestInput};
use crate::manifest::{DerivationSourceManifest, ProtocolBlockManifest};
#[cfg(not(feature = "std"))]
use crate::no_std::*;

pub fn decode_transactions(tx_list: &[u8]) -> Vec<TransactionSigned> {
    #[allow(clippy::useless_asref)]
    Vec::<TransactionSigned>::decode(&mut tx_list.as_ref()).unwrap_or_else(|e| {
        // If decoding fails we need to make an empty block
        warn!("decode_transactions not successful: {e:?}, use empty tx_list");
        vec![]
    })
}

// leave a simply fn in case of more checks in future
fn validate_calldata_tx_list(tx_list: &[u8]) -> bool {
    tx_list.len() <= CALL_DATA_CAPACITY
}

fn unzip_tx_list_from_data_buf(
    chain_spec: &ChainSpec,
    is_blob_data: bool,
    blob_slice_param: Option<(usize, usize)>,
    tx_list_data_buf: &[u8],
) -> Vec<u8> {
    debug!(
        "unzip_tx_list_from_data_buf(is_blob_data: {is_blob_data}, tx_list_data_buf.len: {:?}, blob_slice_param: {blob_slice_param:?})",
        tx_list_data_buf.len()
    );
    #[allow(clippy::collapsible_else_if)]
    if chain_spec.is_taiko() {
        // taiko has some limitations to be aligned with taiko-client
        if is_blob_data {
            let compressed_tx_list = decode_blob_data(tx_list_data_buf);
            assert!(compressed_tx_list.len() <= MAX_BLOB_DATA_SIZE);
            let slice_compressed_tx_list = if let Some((offset, length)) = blob_slice_param {
                if offset + length > compressed_tx_list.len() {
                    error!("blob_slice_param ({offset},{length}) out of range, use empty tx_list");
                    vec![]
                } else {
                    compressed_tx_list[offset..offset + length].to_vec()
                }
            } else {
                compressed_tx_list.to_vec()
            };
            zlib_decompress_data(&slice_compressed_tx_list).unwrap_or_default()
        } else {
            if Network::TaikoA7.to_string() == chain_spec.network() {
                let tx_list = zlib_decompress_data(tx_list_data_buf).unwrap_or_default();
                if validate_calldata_tx_list(&tx_list) {
                    tx_list
                } else {
                    warn!("validate_calldata_tx_list failed, use empty tx_list");
                    vec![]
                }
            } else {
                if validate_calldata_tx_list(tx_list_data_buf) {
                    zlib_decompress_data(tx_list_data_buf).unwrap_or_default()
                } else {
                    warn!("validate_calldata_tx_list failed, use empty tx_list");
                    vec![]
                }
            }
        }
    } else {
        // no limitation on non-taiko chains
        zlib_decompress_data(tx_list_data_buf).unwrap_or_default()
    }
}

pub fn generate_transactions(
    chain_spec: &ChainSpec,
    block_proposal: &BlockProposedFork,
    tx_list_data_buf: &[u8],
    anchor_tx: &Option<TransactionSigned>,
) -> Vec<TransactionSigned> {
    let is_blob_data = block_proposal.blob_used();
    let blob_slice_param = block_proposal.blob_tx_slice_param(tx_list_data_buf);
    // Decode the tx list from the raw data posted onchain
    let unzip_tx_list_buf =
        unzip_tx_list_from_data_buf(chain_spec, is_blob_data, blob_slice_param, tx_list_data_buf);
    // Decode the transactions from the tx list
    let mut transactions = decode_transactions(&unzip_tx_list_buf);
    // Add the anchor tx at the start of the list
    if let Some(anchor_tx) = anchor_tx {
        transactions.insert(0, anchor_tx.clone());
    }
    transactions
}

/// distribute txs to each block by its tx_nums
/// e.g. txs = [tx1, tx2, tx3, tx4, tx5, tx6, tx7, tx8, tx9, tx10]
///     tx_num_sizes = [2, 3, 5]
///    then the result will be [[tx1, tx2], [tx3, tx4, tx5], [tx6, tx7, tx8, tx9, tx10]]
/// special case: if txs.len() < tx_num_sizes.sum(), the rest blocks either empty or with the rest txs
///               if txs.len() > tx_num_sizes.sum(), the rest txs will be ignored
fn distribute_txs<T: Clone>(data: &[T], batch_proposal: &BlockProposedFork) -> Vec<Vec<T>> {
    let tx_num_sizes = batch_proposal
        .batch_info()
        .unwrap()
        .blocks
        .iter()
        .map(|b| b.numTransactions as usize)
        .collect::<Vec<_>>();

    let proposal_txs_count: usize = tx_num_sizes.iter().sum();
    if data.len() != proposal_txs_count {
        warn!(
            "txs.len() != tx_num_sizes.sum(), txs.len(): {}, tx_num_sizes.sum(): {}",
            data.len(),
            proposal_txs_count
        );
    }

    let mut txs_list = Vec::new();
    let total_tx_count = data.len();
    tx_num_sizes.iter().fold(0, |acc, size| {
        if acc + size <= total_tx_count {
            txs_list.push(data[acc..acc + size].to_vec());
        } else if acc < total_tx_count {
            txs_list.push(data[acc..].to_vec());
        } else {
            txs_list.push(Vec::new());
        }
        acc + size
    });
    txs_list
}

/// concat blob & decode a whole txlist, then
/// each block will get a portion of the txlist by its tx_nums
pub fn generate_transactions_for_batch_blocks(
    guest_batch_input: &GuestBatchInput,
) -> Vec<(Vec<TransactionSigned>, bool)> {
    let taiko_guest_batch_input = &guest_batch_input.taiko;
    assert!(
        taiko_guest_batch_input.data_sources.len() > 0,
        "data_source is empty"
    );

    let batch_proposal = &taiko_guest_batch_input.batch_proposed;
    match batch_proposal {
        BlockProposedFork::Pacaya(_) => generate_transactions_for_pacaya_blocks(guest_batch_input),
        BlockProposedFork::Shasta(_) => generate_transactions_for_shasta_blocks(guest_batch_input),
        _ => {
            unreachable!(
                "only pacaya and shasta batch supported, but got {:?}",
                batch_proposal
            );
        }
    }
}

/// concat blob & decode a whole txlist, then
/// each block will get a portion of the txlist by its tx_nums
pub fn generate_transactions_for_pacaya_blocks(
    taiko_guest_batch_input: &GuestBatchInput,
) -> Vec<(Vec<TransactionSigned>, bool)> {
    let taiko_guest_batch_input = &taiko_guest_batch_input.taiko;
    let batch_proposal = &taiko_guest_batch_input.batch_proposed;
    let data_source = &taiko_guest_batch_input.data_sources[0];
    assert!(
        data_source.tx_data_from_calldata.is_empty() || data_source.tx_data_from_blob.is_empty(),
        "Txlist comes from either calldata or blob, but not both"
    );
    let use_blob = batch_proposal.blob_used();
    let compressed_tx_list_buf = if use_blob {
        let blob_data_bufs = data_source.tx_data_from_blob.clone();
        let compressed_tx_list_buf = blob_data_bufs
            .iter()
            .map(|blob_data_buf| decode_blob_data(blob_data_buf))
            .collect::<Vec<Vec<u8>>>()
            .concat();
        let (blob_offset, blob_size) = batch_proposal
            .blob_tx_slice_param(&compressed_tx_list_buf)
            .unwrap_or_else(|| {
                warn!("blob_tx_slice_param not found, use full buffer to decode tx_list");
                (0, compressed_tx_list_buf.len())
            });
        tracing::info!("blob_offset: {blob_offset}, blob_size: {blob_size}");
        compressed_tx_list_buf[blob_offset..blob_offset + blob_size].to_vec()
    } else {
        data_source.tx_data_from_calldata.clone()
    };

    let tx_list_buf = zlib_decompress_data(&compressed_tx_list_buf).unwrap_or_default();
    let txs = decode_transactions(&tx_list_buf);
    // todo: deal with invalid proposal, to name a few:
    // - txs.len() != tx_num_sizes.sum()
    // - random blob tx bytes
    distribute_txs(&txs, batch_proposal)
        .into_iter()
        .map(|txs| (txs, false))
        .collect()
}

const PROPOSAL_MAX_BLOCKS: usize = 384usize;
/// concat blob & decode a whole txlist, then
/// each block will get a portion of the txlist by its tx_nums
pub fn generate_transactions_for_shasta_blocks(
    guest_batch_input: &GuestBatchInput,
) -> Vec<(Vec<TransactionSigned>, bool)> {
    let taiko_guest_batch_input = &guest_batch_input.taiko;
    let batch_proposal = &taiko_guest_batch_input.batch_proposed;
    let data_sources = &taiko_guest_batch_input.data_sources;
    let mut tx_list_bufs = Vec::new();

    // TODO: for invalid path, align the default calculation with node
    let last_parent_block_header = &guest_batch_input.inputs[0].parent_header;
    let last_anchor_block_number = guest_batch_input
        .taiko
        .prover_data
        .last_anchor_block_number
        .unwrap();
    for (idx, data_source) in data_sources.iter().enumerate() {
        let use_blob = batch_proposal.blob_used();
        let compressed_tx_list_buf = if use_blob {
            let blob_data_bufs = data_source.tx_data_from_blob.clone();
            let compressed_tx_list_buf = blob_data_bufs
                .iter()
                .map(|blob_data_buf| decode_blob_data(blob_data_buf))
                .collect::<Vec<Vec<u8>>>()
                .concat();
            let (blob_offset, blob_size) = batch_proposal
                .blob_tx_slice_param(&compressed_tx_list_buf)
                .unwrap_or_else(|| (0, 0));
            tracing::info!("blob_offset: {blob_offset}, blob_size: {blob_size}");
            compressed_tx_list_buf[blob_offset..blob_offset + blob_size].to_vec()
        } else {
            unreachable!("shasta does not use calldata");
        };

        // - Decode manifest from blob data
        // - Extract transactions from manifest blocks
        // - Distribute transactions to blocks
        if idx == data_sources.len() - 1 {
            assert!(
                !data_source.is_forced_inclusion,
                "last source should be normal source"
            );
            let protocol_manifest_bytes =
                zlib_decompress_data(&compressed_tx_list_buf).unwrap_or_default();
            let protocol_manifest =
                match DerivationSourceManifest::decode(&mut protocol_manifest_bytes.as_ref()) {
                    Ok(manifest)
                        if validate_normal_proposal_manifest(
                            &manifest,
                            last_anchor_block_number,
                        ) =>
                    {
                        manifest
                    }
                    _ => {
                        let timestamp = taiko_guest_batch_input.l1_header.timestamp;
                        let coinbase = taiko_guest_batch_input.batch_proposed.proposer();
                        let anchor_block_number = last_anchor_block_number;
                        let gas_limit = last_parent_block_header.gas_limit;
                        let transactions = Vec::new();
                        DerivationSourceManifest::default_block_manifest(
                            timestamp,
                            coinbase,
                            anchor_block_number,
                            gas_limit,
                            transactions,
                        )
                    }
                };

            protocol_manifest
                .blocks
                .iter()
                .enumerate()
                .for_each(|(offset, block)| {
                    assert!(
                        validate_input_block_param(
                            block,
                            &guest_batch_input.inputs[idx + offset].block
                        ),
                        "input block manifest is invalid"
                    );
                    tx_list_bufs.push((block.transactions.clone(), false))
                });
        } else {
            assert!(
                data_source.is_forced_inclusion,
                "begin sources are force inclusion source"
            );

            let timestamp = taiko_guest_batch_input.l1_header.timestamp;
            let coinbase = taiko_guest_batch_input.batch_proposed.proposer();
            let anchor_block_number = last_anchor_block_number;
            let gas_limit = last_parent_block_header.gas_limit;
            let transactions = Vec::new();
            let force_inc_source_bytes =
                zlib_decompress_data(&compressed_tx_list_buf).unwrap_or_default();
            let force_inc_source =
                match DerivationSourceManifest::decode(&mut force_inc_source_bytes.as_ref()) {
                    Ok(manifest) if validate_force_inc_proposal_manifest(&manifest) => manifest,
                    _ => DerivationSourceManifest::default_block_manifest(
                        timestamp,
                        coinbase,
                        anchor_block_number,
                        gas_limit,
                        transactions,
                    ),
                };
            // force inc has only 1 block
            let force_inc_block_manifest = &force_inc_source.blocks[0];
            assert!(
                validate_input_block_param(
                    force_inc_block_manifest,
                    &guest_batch_input.inputs[idx].block
                ),
                "force inclusion source is invalid"
            );
            tx_list_bufs.push((force_inc_block_manifest.transactions.clone(), true));
        }
    }
    tx_list_bufs
}

fn valid_anchor_in_normal_proposal(
    blocks: &[ProtocolBlockManifest],
    last_anchor_block_number: u64,
) -> bool {
    // at least 1 anchor number in one proposal should > last_anchor_block_number
    blocks
        .iter()
        .any(|block| block.anchor_block_number > last_anchor_block_number)
}

fn validate_normal_proposal_manifest(
    manifest: &DerivationSourceManifest,
    last_anchor_block_number: u64,
) -> bool {
    let manifest_block_number = manifest.blocks.len();
    if manifest_block_number <= PROPOSAL_MAX_BLOCKS {
        error!(
            "manifest_block_number {} <= PROPOSAL_MAX_BLOCKS {}",
            manifest_block_number, PROPOSAL_MAX_BLOCKS
        );
        return false;
    }

    if !valid_anchor_in_normal_proposal(&manifest.blocks, last_anchor_block_number) {
        error!(
            "valid_anchor_in_proposal failed, last_anchor_block_number: {}",
            last_anchor_block_number
        );
        return false;
    }
    true
}

fn validate_force_inc_proposal_manifest(manifest: &DerivationSourceManifest) -> bool {
    if manifest.blocks.len() != 1
        || manifest.blocks[0].timestamp != 0
        || manifest.blocks[0].coinbase != Address::default()
        || manifest.blocks[0].anchor_block_number != 0
        || manifest.blocks[0].gas_limit != 0
    {
        error!(
            "validate_force_inc_proposal_manifest failed, manifest: {:?}",
            manifest
        );
        return false;
    }
    true
}

fn validate_input_block_param(manifest_block: &ProtocolBlockManifest, input_block: &Block) -> bool {
    if manifest_block.timestamp != input_block.header.timestamp {
        error!(
            "manifest_block.timestamp != input_block.header.timestamp, manifest_block.timestamp: {}, input_block.header.timestamp: {}",
            manifest_block.timestamp, input_block.header.timestamp
        );
        return false;
    }
    if manifest_block.coinbase != input_block.header.beneficiary {
        error!(
            "manifest_block.coinbase != input_block.header.coinbase, manifest_block.coinbase: {}, input_block.header.coinbase: {}",
            manifest_block.coinbase, input_block.header.beneficiary
        );
        return false;
    }
    if manifest_block.gas_limit != input_block.header.gas_limit {
        error!(
            "manifest_block.gas_limit != input_block.header.gas_limit, manifest_block.gas_limit: {}, input_block.header.gas_limit: {}",
            manifest_block.gas_limit, input_block.header.gas_limit
        );
        return false;
    }
    true
}

const MAX_BLOCK_GAS_LIMIT_CHANGE_PERMYRIAD: u64 = 10;
const MAX_BLOCK_GAS_LIMIT: u64 = 100_000_000;
const MIN_BLOCK_GAS_LIMIT: u64 = 10_000_000;

/// validate gas limit for each block
pub fn validate_shasta_block_gas_limit(block_guest_inputs: &[GuestInput]) -> bool {
    for block_guest_input in block_guest_inputs.iter() {
        let parent_gas_limit = block_guest_input.parent_header.gas_limit;
        let block_gas_limit: u64 = block_guest_input.block.header.gas_limit;
        let upper_limit = min(
            MAX_BLOCK_GAS_LIMIT,
            parent_gas_limit * (10000 + MAX_BLOCK_GAS_LIMIT_CHANGE_PERMYRIAD) / 10000,
        );
        let lower_limit = max(
            MIN_BLOCK_GAS_LIMIT,
            parent_gas_limit * (10000 - MAX_BLOCK_GAS_LIMIT_CHANGE_PERMYRIAD) / 10000,
        );
        assert!(
            block_gas_limit >= lower_limit && block_gas_limit <= upper_limit,
            "block gas limit is out of bounds"
        );
        if block_gas_limit < lower_limit || block_gas_limit > upper_limit {
            return false;
        }
    }
    true
}

// Offset constant for lower bound, placeholder, adjust as needed for protocol.
const TIMESTAMP_MAX_OFFSET: u64 = 12 * 32;

/// validate timestamp for each block
// #### `timestamp` Validation
// Validates that block timestamps conform to the protocol rules. The 3rd party should set correct values
// according to these rules before calling this function:
// 1. **Upper bound validation**: `block.timestamp <= proposal.timestamp` must hold
// 2. **Lower bound calculation**: `lowerBound = max(parent.timestamp + 1, proposal.timestamp - TIMESTAMP_MAX_OFFSET)`
// 3. **Lower bound validation**: `block.timestamp >= lowerBound` must hold
pub fn validate_shasta_block_timesatmp(block_guest_inputs: &[GuestInput]) -> bool {
    for block_guest_input in block_guest_inputs.iter() {
        let block_timestamp = block_guest_input.block.header.timestamp;
        let proposal_timestamp = block_guest_input.taiko.block_proposed.proposal_timestamp();
        // Upper bound validation: block.timestamp <= proposal.timestamp
        assert!(
            block_timestamp <= proposal_timestamp,
            "Block timestamp {} exceeds proposal timestamp {}",
            block_timestamp,
            proposal_timestamp
        );

        // Lower bound validation:
        // Calculate lowerBound = max(parent.timestamp + 1, proposal.timestamp - TIMESTAMP_MAX_OFFSET)
        // Then validate: block.timestamp >= lowerBound
        let parent_timestamp = block_guest_input.parent_header.timestamp;
        let lower_bound = std_max(
            parent_timestamp + 1,
            proposal_timestamp.saturating_sub(TIMESTAMP_MAX_OFFSET),
        );
        assert!(
            block_timestamp >= lower_bound,
            "Block timestamp {} is less than calculated lower bound {}",
            block_timestamp,
            lower_bound
        );
    }
    true
}

const BLOB_FIELD_ELEMENT_NUM: usize = 4096;
const BLOB_FIELD_ELEMENT_BYTES: usize = 32;
const BLOB_DATA_CAPACITY: usize = BLOB_FIELD_ELEMENT_NUM * BLOB_FIELD_ELEMENT_BYTES;
// max call data bytes
const CALL_DATA_CAPACITY: usize = BLOB_FIELD_ELEMENT_NUM * (BLOB_FIELD_ELEMENT_BYTES - 1);
const BLOB_VERSION_OFFSET: usize = 1;
const BLOB_ENCODING_VERSION: u8 = 0;
const MAX_BLOB_DATA_SIZE: usize = (4 * 31 + 3) * 1024 - 4;

// decoding https://github.com/ethereum-optimism/optimism/blob/develop/op-service/eth/blob.go
pub fn decode_blob_data(blob_buf: &[u8]) -> Vec<u8> {
    // check the version
    if blob_buf[BLOB_VERSION_OFFSET] != BLOB_ENCODING_VERSION {
        return Vec::new();
    }

    // decode the 3-byte big-endian length value into a 4-byte integer
    let output_len = (u32::from(blob_buf[2]) << 16
        | u32::from(blob_buf[3]) << 8
        | u32::from(blob_buf[4])) as usize;

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
        let Ok(res) = decode_field_element(blob_buf, opos, ipos, &mut output) else {
            return Vec::new();
        };

        (*encoded_byte_i, opos, ipos) = res;
    }
    opos = reassemble_bytes(opos, encoded_byte, &mut output);

    // in each remaining round we decode 4 field elements (128 bytes) of the input into 127
    // bytes of output
    for _ in 1..1024 {
        if opos < output_len {
            for encoded_byte_j in &mut encoded_byte {
                // save the first byte of each field element for later re-assembly
                let Ok(res) = decode_field_element(blob_buf, opos, ipos, &mut output) else {
                    return Vec::new();
                };

                (*encoded_byte_j, opos, ipos) = res;
            }
            opos = reassemble_bytes(opos, encoded_byte, &mut output);
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
            "ErrBlobInvalidFieldElement: field element: {ipos}",
        ));
    }
    // copy(output[opos:], b[ipos+1:ipos+32])
    output[opos..opos + 31].copy_from_slice(&b[ipos + 1..ipos + 32]);
    Ok((b[ipos], opos + 32, ipos + 32))
}

fn reassemble_bytes(
    opos: usize,
    encoded_byte: [u8; 4],
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

pub fn zlib_decompress_data(data: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = zlibDecoder::new(data)?;
    let mut decoded_buf = Vec::new();
    decoder.read_to_end(&mut decoded_buf)?;
    Ok(decoded_buf)
}

pub fn zlib_compress_data(data: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = zlibEncoder::new(Vec::new())?;
    encoder.write_all(data).unwrap();
    let res = encoder.finish().into_result()?;
    Ok(res.clone())
}
