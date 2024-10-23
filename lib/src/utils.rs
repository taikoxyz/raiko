use std::io::{Read, Write};

use alloy_rlp::Decodable;
use anyhow::Result;
use libflate::zlib::{Decoder as zlibDecoder, Encoder as zlibEncoder};
use reth_primitives::TransactionSigned;
use tracing::{error, warn};

use crate::consts::{ChainSpec, Network};
use crate::input::BlockProposedFork;
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
    let blob_slice_param = block_proposal.blob_tx_slice_param();
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

const BLOB_FIELD_ELEMENT_NUM: usize = 4096;
const BLOB_FIELD_ELEMENT_BYTES: usize = 32;
const BLOB_DATA_CAPACITY: usize = BLOB_FIELD_ELEMENT_NUM * BLOB_FIELD_ELEMENT_BYTES;
// max call data bytes
const CALL_DATA_CAPACITY: usize = BLOB_FIELD_ELEMENT_NUM * (BLOB_FIELD_ELEMENT_BYTES - 1);
const BLOB_VERSION_OFFSET: usize = 1;
const BLOB_ENCODING_VERSION: u8 = 0;
const MAX_BLOB_DATA_SIZE: usize = (4 * 31 + 3) * 1024 - 4;

// decoding https://github.com/ethereum-optimism/optimism/blob/develop/op-service/eth/blob.go
fn decode_blob_data(blob_buf: &[u8]) -> Vec<u8> {
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
