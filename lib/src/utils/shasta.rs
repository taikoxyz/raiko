use alloy_rlp::Decodable;
use log::warn;
use reth_evm_ethereum::taiko::ANCHOR_V4_GAS_LIMIT;
use reth_primitives::revm_primitives::SpecId;
use reth_primitives::TransactionSigned;

use crate::consts::ForkCondition;
use crate::input::GuestBatchInput;
use crate::manifest::DerivationSourceManifest;
#[cfg(not(feature = "std"))]
use crate::no_std::*;
use crate::utils::blobs::{decode_blob_data, zlib_decompress_data};
use crate::utils::shasta_rules::*;

fn make_default_manifest(
    guest_batch_input: &GuestBatchInput,
    last_parent_block_timestamp: u64,
    last_parent_block_gas_limit: u64,
    last_anchor_block_number: u64,
) -> DerivationSourceManifest {
    let taiko_guest_batch_input = &guest_batch_input.taiko;
    let proposal_timestamp = taiko_guest_batch_input.batch_proposed.proposal_timestamp();
    let shasta_fork_timestamp = match guest_batch_input
        .taiko
        .chain_spec
        .hard_forks
        .get(&SpecId::SHASTA)
    {
        Some(ForkCondition::Timestamp(timestamp)) => *timestamp,
        _ => unreachable!("shasta fork should be a timestamp fork"),
    };
    let timestamp = clamp_timestamp_lower_bound(
        last_parent_block_timestamp,
        proposal_timestamp,
        shasta_fork_timestamp,
    );
    let coinbase = taiko_guest_batch_input.batch_proposed.proposer();
    let anchor_block_number = last_anchor_block_number;
    let gas_limit = if guest_batch_input
        .inputs
        .first()
        .unwrap()
        .parent_header
        .number
        == 0
    {
        last_parent_block_gas_limit
    } else {
        last_parent_block_gas_limit - ANCHOR_V4_GAS_LIMIT
    };
    let transactions = Vec::new();
    DerivationSourceManifest::default_block_manifest(
        timestamp,
        coinbase,
        anchor_block_number,
        gas_limit,
        transactions,
    )
}

/// concat blob & decode a whole txlist, then
/// each block will get a portion of the txlist by its tx_nums
pub fn generate_transactions_for_shasta_blocks(
    guest_batch_input: &GuestBatchInput,
) -> Vec<(Vec<TransactionSigned>, bool)> {
    let taiko_guest_batch_input = &guest_batch_input.taiko;
    let batch_proposal = &taiko_guest_batch_input.batch_proposed;
    let data_sources = &taiko_guest_batch_input.data_sources;
    let mut tx_list_bufs = Vec::new();

    let last_anchor_block_number = guest_batch_input
        .taiko
        .prover_data
        .last_anchor_block_number
        .unwrap();
    let last_parent_block_header = &guest_batch_input.inputs[0].parent_header;
    let mut last_parent_block_timestamp = last_parent_block_header.timestamp;
    let mut last_parent_block_gas_limit = last_parent_block_header.gas_limit;

    for (idx, data_source) in data_sources.iter().enumerate() {
        let use_blob = batch_proposal.blob_used();
        let compressed_tx_list_buf = if use_blob {
            let blob_data_bufs = data_source.tx_data_from_blob.clone();
            let decoded_blob_data_concat = blob_data_bufs
                .iter()
                .map(|blob_data_buf| decode_blob_data(blob_data_buf))
                .collect::<Vec<Vec<u8>>>()
                .concat();
            let sliced = batch_proposal
                .blob_tx_slice_param_for_source(idx, &decoded_blob_data_concat)
                .and_then(|(blob_offset, blob_size)| {
                    tracing::info!("blob_offset: {blob_offset}, blob_size: {blob_size}");
                    decoded_blob_data_concat
                        .get(blob_offset..blob_offset + blob_size)
                        .map(|s| s.to_vec())
                })
                .unwrap_or_default();
            sliced
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
                            &guest_batch_input,
                            &manifest,
                            last_anchor_block_number,
                        ) =>
                    {
                        // parent is pacaya means this is the first shasta block
                        let is_first_shasta_proposal = guest_batch_input
                            .taiko
                            .chain_spec
                            .active_fork(
                                guest_batch_input.inputs[0].parent_header.number,
                                guest_batch_input.inputs[0].parent_header.timestamp,
                            )
                            .unwrap()
                            == SpecId::PACAYA
                            || guest_batch_input.inputs[0].parent_header.number == 0;

                        //TODO: move to validate_normal_proposal_manifest
                        if !validate_shasta_block_base_fee(
                            &guest_batch_input.inputs,
                            is_first_shasta_proposal,
                            guest_batch_input.taiko.l2_grandparent_header.as_ref(),
                        ) {
                            warn!("shasta block base fee is invalid, need double check");
                            make_default_manifest(
                                guest_batch_input,
                                last_parent_block_timestamp,
                                last_parent_block_gas_limit,
                                last_anchor_block_number,
                            )
                        } else {
                            manifest
                        }
                    }
                    _ => {
                        let manifest = make_default_manifest(
                            guest_batch_input,
                            last_parent_block_timestamp,
                            last_parent_block_gas_limit,
                            last_anchor_block_number,
                        );
                        warn!(
                            "shasta block manifest is invalid, use default manifest: {:?}",
                            &manifest
                        );
                        manifest
                    }
                };

            protocol_manifest
                .blocks
                .iter()
                .enumerate()
                .for_each(|(offset, block_manifest)| {
                    assert!(
                        validate_input_block_param(
                            block_manifest,
                            &guest_batch_input.inputs[idx + offset].block
                        ),
                        "input block manifest is invalid"
                    );
                    tx_list_bufs.push((block_manifest.transactions.clone(), false))
                });
        } else {
            assert!(
                data_source.is_forced_inclusion,
                "begin sources are force inclusion source"
            );

            let force_inc_source_bytes =
                zlib_decompress_data(&compressed_tx_list_buf).unwrap_or_default();
            let force_inc_source =
                match DerivationSourceManifest::decode(&mut force_inc_source_bytes.as_ref()) {
                    Ok(manifest) if validate_force_inc_proposal_manifest(&manifest) => {
                        // overwrite force inc manifest
                        let mut force_inc_manifest = make_default_manifest(
                            guest_batch_input,
                            last_parent_block_timestamp,
                            last_parent_block_gas_limit,
                            last_anchor_block_number,
                        );
                        force_inc_manifest.blocks[0].transactions =
                            manifest.blocks[0].transactions.clone();
                        force_inc_manifest
                    }
                    _ => {
                        let manifest = make_default_manifest(
                            guest_batch_input,
                            last_parent_block_timestamp,
                            last_parent_block_gas_limit,
                            last_anchor_block_number,
                        );
                        warn!(
                            "force inclusion block manifest is invalid, use default manifest: {:?}",
                            &manifest
                        );
                        manifest
                    }
                };

            // force inc has only 1 block
            let force_inc_block_manifest = &force_inc_source.blocks[0];
            // update last parent block timestamp for next iteration
            last_parent_block_timestamp = force_inc_block_manifest.timestamp;
            last_parent_block_gas_limit = force_inc_block_manifest.gas_limit;
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
