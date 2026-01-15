// utils fns for tx procerssing.
use alloy_rlp::Decodable;
use reth_primitives::TransactionSigned;
use tracing::{debug, error, warn};

use crate::consts::{ChainSpec, Network};
use crate::input::{BlockProposedFork, GuestBatchInput};

#[cfg(not(feature = "std"))]
use crate::no_std::*;
use crate::utils::blobs::{
    decode_blob_data, zlib_decompress_data, CALL_DATA_CAPACITY, MAX_BLOB_DATA_SIZE,
};
use crate::utils::pacaya::generate_transactions_for_pacaya_blocks;
use crate::utils::shasta::generate_transactions_for_shasta_blocks;

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
