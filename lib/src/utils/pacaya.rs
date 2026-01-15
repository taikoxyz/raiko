use reth_primitives::TransactionSigned;
use tracing::warn;

use crate::input::{BlockProposedFork, GuestBatchInput};
#[cfg(not(feature = "std"))]
use crate::no_std::*;
use crate::utils::blobs::{decode_blob_data, zlib_decompress_data};
use crate::utils::txs::decode_transactions;

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
        let (blob_offset, blob_size) = batch_proposal.blob_tx_slice_param().unwrap_or_else(|| {
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
