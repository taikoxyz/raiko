use alloy_primitives::{hex, Log as LogStruct, B256};
use alloy_provider::{Provider, ReqwestProvider};
use alloy_rpc_types::{
    BlockId, BlockTransactionsKind, Filter, Header, Log, Transaction as AlloyRpcTransaction,
};
use alloy_sol_types::SolEvent;
use anyhow::{anyhow, bail, ensure, Result};
use kzg::kzg_types::ZFr;
use kzg_traits::{
    eip_4844::{blob_to_kzg_commitment_rust, Blob},
    Fr, G1,
};
use raiko_lib::{
    builder::{OptimisticDatabase, RethBlockBuilder},
    clear_line,
    consts::ChainSpec,
    inplace_print,
    input::{
        shasta::{Proposed as ShastaProposed, ShastaEventData},
        BlobProofType, BlockProposedFork, InputDataSource, TaikoGuestBatchInput, TaikoProverData,
    },
    primitives::eip4844::{self, commitment_to_version_hash, KZG_SETTINGS},
    utils::shasta_rules::anchor_max_offset_for_chain,
};
use reth_evm_ethereum::taiko::decode_anchor_shasta;
use reth_primitives::{Block as RethBlock, TransactionSigned};
use reth_revm::primitives::SpecId;
use serde::{Deserialize, Serialize};
use std::iter;
use tracing::{debug, info, warn};

use crate::{
    interfaces::{RaikoError, RaikoResult},
    provider::{db::ProviderDb, rpc::RpcBlockDataProvider, BlockDataProvider},
};

/// Optimize data gathering by executing the transactions multiple times so data can be requested in batches
pub async fn execute_txs<BDP>(
    builder: &mut RethBlockBuilder<ProviderDb<'_, BDP>>,
    pool_txs: Vec<reth_primitives::TransactionSigned>,
) -> RaikoResult<()>
where
    BDP: BlockDataProvider,
{
    let max_iterations = 100;
    for num_iterations in 0.. {
        inplace_print(&format!("Executing iteration {num_iterations}..."));

        let Some(db) = builder.db.as_mut() else {
            return Err(RaikoError::Preflight("No db in builder".to_owned()));
        };
        db.optimistic = num_iterations + 1 < max_iterations;

        builder
            .execute_transactions(pool_txs.clone(), num_iterations + 1 < max_iterations)
            .map_err(|e| {
                RaikoError::Preflight(format!("Executing transactions in builder failed: {e}"))
            })?;

        let Some(db) = builder.db.as_mut() else {
            return Err(RaikoError::Preflight("No db in builder".to_owned()));
        };
        if db.fetch_data().await {
            clear_line();
            debug!("State data fetched in {num_iterations} iterations");
            break;
        }
    }

    Ok(())
}

// get fork corresponding anchor block height and state root
fn get_anchor_tx_info_by_fork(
    fork: SpecId,
    anchor_tx: &TransactionSigned,
) -> RaikoResult<(u64, B256)> {
    match fork {
        SpecId::SHASTA => {
            let anchor_call = decode_anchor_shasta(anchor_tx.input())?;
            Ok((
                anchor_call._checkpoint.blockNumber,
                anchor_call._checkpoint.stateRoot,
            ))
        }
        _ => Err(RaikoError::Preflight(format!(
            "unsupported taiko fork in shasta-only mode: {fork:?}"
        ))),
    }
}

/// Shasta proposal requests use a dedicated entrypoint.
/// Returns (block_numbers, event_data) for caching and reuse
pub async fn parse_l1_batch_proposal_tx_for_shasta_fork(
    l1_chain_spec: &ChainSpec,
    taiko_chain_spec: &ChainSpec,
    l1_inclusion_block_number: u64,
    proposal_id: u64,
) -> RaikoResult<(Vec<u64>, BlockProposedFork)> {
    let provider_l1 = RpcBlockDataProvider::new(&l1_chain_spec.rpc, 0).await?;
    let (l1_inclusion_height, _tx, proposal_fork) = get_block_proposed_event_by_height(
        provider_l1.provider(),
        taiko_chain_spec.clone(),
        l1_inclusion_block_number,
        proposal_id,
        SpecId::SHASTA,
    )
    .await?;

    assert_eq!(
        l1_inclusion_block_number, l1_inclusion_height,
        "proposal tx inclusive block != proof_request block"
    );

    // _proposal_fork is shasta proposal tx, so we can get the lastFinalizedProposalId from it
    match &proposal_fork {
        BlockProposedFork::Shasta(_) => {
            // todo: no way to get l2 block numbers from shasta proposal tx
            Ok((vec![], proposal_fork))
        }
        _ => Err(RaikoError::Preflight(
            "BlockProposedFork is not Shasta".to_owned(),
        )),
    }
}

/// Prepare Shasta batch input
#[allow(clippy::too_many_arguments)]
async fn prepare_shasta_batch_input(
    shasta_event_data: raiko_lib::input::shasta::ShastaEventData,
    batch_id: u64,
    _l1_inclusion_block_number: u64,
    _anchor_block_height: u64,
    l1_inclusion_header: Header,
    l1_state_header: Header,
    l1_ancestor_headers: Vec<Header>,
    l1_chain_spec: &ChainSpec,
    taiko_chain_spec: &ChainSpec,
    prover_data: TaikoProverData,
    blob_proof_type: &BlobProofType,
    _provider_l1: &RpcBlockDataProvider,
    grandparent_header: Option<reth_primitives::Header>,
) -> RaikoResult<TaikoGuestBatchInput> {
    let mut data_sources = Vec::new();
    for derivation_source in shasta_event_data.proposal.sources.clone() {
        let blob_hashes = derivation_source.blobSlice.blobHashes;
        let is_forced_inclusion = derivation_source.isForcedInclusion;
        let l1_blob_timestamp = if is_forced_inclusion {
            // force inclusion block
            info!("process force inclusion from data_source blob hashes: {blob_hashes:?}");
            derivation_source.blobSlice.timestamp
        } else {
            l1_inclusion_header.timestamp
        };

        // according to protocol, calldata is mutex with blob
        let (tx_data_from_calldata, blob_tx_buffers_with_proofs) = if blob_hashes.is_empty() {
            unimplemented!("calldata txlist is not supported in shasta.");
        } else {
            let blob_tx_buffers = get_batch_tx_data_with_proofs(
                blob_hashes,
                l1_blob_timestamp,
                l1_chain_spec,
                blob_proof_type,
            )
            .await?;
            (Vec::new(), blob_tx_buffers)
        };
        data_sources.push(InputDataSource {
            tx_data_from_calldata,
            tx_data_from_blob: blob_tx_buffers_with_proofs
                .iter()
                .map(|(blob_tx_data, _, _)| blob_tx_data.clone())
                .collect(),
            blob_commitments: blob_tx_buffers_with_proofs
                .iter()
                .map(|(_, commit, _)| commit.clone())
                .collect(),
            blob_proofs: blob_tx_buffers_with_proofs
                .iter()
                .map(|(_, _, proof)| proof.clone())
                .collect(),
            blob_proof_type: blob_proof_type.clone(),
            is_forced_inclusion,
        });
    }

    Ok(TaikoGuestBatchInput {
        batch_id,
        batch_proposed: BlockProposedFork::Shasta(shasta_event_data),
        l1_header: l1_state_header.try_into().unwrap(),
        l1_ancestor_headers: l1_ancestor_headers
            .iter()
            .map(|header| header.clone().try_into().unwrap())
            .collect(),
        chain_spec: taiko_chain_spec.clone(),
        prover_data,
        l2_grandparent_header: grandparent_header,
        data_sources,
    })
}

#[allow(clippy::too_many_arguments)]
async fn prepare_taiko_chain_batch_input_shasta(
    l1_chain_spec: &ChainSpec,
    taiko_chain_spec: &ChainSpec,
    l1_inclusion_block_number: u64,
    batch_id: u64,
    prover_data: TaikoProverData,
    blob_proof_type: &BlobProofType,
    batch_anchor_tx_info: Vec<(u64, B256)>,
    shasta_event_data: raiko_lib::input::shasta::ShastaEventData,
    grandparent_header: Option<reth_primitives::Header>,
) -> RaikoResult<TaikoGuestBatchInput> {
    let (anchor_block_height, _) = batch_anchor_tx_info[0];
    let provider_l1 = RpcBlockDataProvider::new(&l1_chain_spec.rpc, 0).await?;

    assert!(
        l1_inclusion_block_number > 0,
        "l1_inclusion_block_number is 0"
    );
    // Shasta uses the parent block instead of the anchor block for the L1 state header.
    let (l1_inclusion_header, l1_state_header) = get_headers(
        &provider_l1,
        (l1_inclusion_block_number, l1_inclusion_block_number - 1),
    )
    .await?;
    assert_eq!(
        l1_inclusion_header.parent_hash,
        l1_state_header.hash.unwrap()
    );

    let anchor_max_offset = anchor_max_offset_for_chain(taiko_chain_spec.chain_id());
    let l1_ancestor_headers = get_max_anchor_headers(
        &provider_l1,
        batch_anchor_tx_info,
        l1_inclusion_block_number - 1,
        prover_data.last_anchor_block_number,
        anchor_max_offset,
    )
    .await?;

    prepare_shasta_batch_input(
        shasta_event_data,
        batch_id,
        l1_inclusion_block_number,
        anchor_block_height,
        l1_inclusion_header,
        l1_state_header,
        l1_ancestor_headers,
        l1_chain_spec,
        taiko_chain_spec,
        prover_data,
        blob_proof_type,
        &provider_l1,
        grandparent_header,
    )
    .await
}

/// Prepare the input for a Taiko chain
#[allow(clippy::too_many_arguments)]
pub async fn prepare_taiko_chain_batch_input(
    l1_chain_spec: &ChainSpec,
    taiko_chain_spec: &ChainSpec,
    l1_inclusion_block_number: u64,
    batch_id: u64,
    batch_blocks: &[RethBlock],
    prover_data: TaikoProverData,
    blob_proof_type: &BlobProofType,
    cached_event_data: Option<BlockProposedFork>,
    grandparent_header: Option<reth_primitives::Header>,
) -> RaikoResult<TaikoGuestBatchInput> {
    // Get the L1 block in which the L2 block was included so we can fetch the DA data.
    // Also get the L1 state block header so that we can prove the L1 state root.
    // Decode the anchor tx to find out which L1 blocks we need to fetch
    let fork = taiko_chain_spec.active_fork(batch_blocks[0].number, batch_blocks[0].timestamp)?;
    let batch_anchor_tx_info = batch_blocks.iter().try_fold(
        Vec::new(),
        |mut acc, block| -> RaikoResult<Vec<(u64, B256)>> {
            let anchor_tx = block
                .body
                .first()
                .ok_or_else(|| RaikoError::Preflight("No anchor tx in the block".to_owned()))?;
            let anchor_info = get_anchor_tx_info_by_fork(fork, anchor_tx)?;
            acc.push(anchor_info);
            Ok(acc)
        },
    )?;

    // Use cached event data if available, otherwise fetch from RPC
    let batch_proposed_fork = if let Some(cached_data) = cached_event_data {
        debug!("Using cached block proposed event data, skipping RPC call");
        cached_data
    } else {
        debug!("No cached event data, fetching from RPC");
        let provider_l1 = RpcBlockDataProvider::new(&l1_chain_spec.rpc, 0).await?;
        let (l1_inclusion_height, _, event_data) = get_block_proposed_event_by_height(
            provider_l1.provider(),
            taiko_chain_spec.clone(),
            l1_inclusion_block_number,
            batch_id,
            fork,
        )
        .await?;
        assert_eq!(l1_inclusion_block_number, l1_inclusion_height);
        event_data
    };

    match (fork, batch_proposed_fork) {
        (SpecId::SHASTA, BlockProposedFork::Shasta(shasta_event_data)) => {
            assert!(
                batch_anchor_tx_info
                    .windows(2)
                    .all(|w| if w[0].0 == w[1].0 {
                        w[0].1 == w[1].1
                    } else {
                        // if anchor changes, block hash is not zero
                        w[0].0 < w[1].0 && w[0].1 != B256::ZERO && w[1].1 != B256::ZERO
                    }),
                "batch anchor tx info mismatch, {batch_anchor_tx_info:?}"
            );
            prepare_taiko_chain_batch_input_shasta(
                l1_chain_spec,
                taiko_chain_spec,
                l1_inclusion_block_number,
                batch_id,
                prover_data,
                blob_proof_type,
                batch_anchor_tx_info,
                shasta_event_data,
                grandparent_header,
            )
            .await
        }
        _ => Err(RaikoError::Preflight(
            "Unsupported BatchProposedFork type".to_owned(),
        )),
    }
}

pub async fn filter_tx_blob_beacon_with_proof(
    blob_hash: B256,
    blobs: Vec<String>,
    blob_proof_type: &BlobProofType,
) -> RaikoResult<(Vec<u8>, Option<Vec<u8>>, Option<Vec<u8>>)> {
    info!("get tx from hash blob: {blob_hash:?}");
    // Get the blob data for this block
    let blob = filter_blob_data_beacon(blobs, blob_hash).await?;
    let commitment = eip4844::calc_kzg_proof_commitment(&blob).map_err(|e| anyhow!(e))?;
    let blob_proof = match blob_proof_type {
        BlobProofType::KzgVersionedHash => None,
        BlobProofType::ProofOfEquivalence => {
            let (x, y) =
                eip4844::proof_of_equivalence(&blob, &commitment_to_version_hash(&commitment))
                    .map_err(|e| anyhow!(e))?;

            debug!("x {x:?} y {y:?}");
            let point = eip4844::calc_kzg_proof_with_point(&blob, ZFr::from_bytes(&x).unwrap());
            debug!("calc_kzg_proof_with_point {point:?}");

            Some(
                point
                    .map(|g1| g1.to_bytes().to_vec())
                    .map_err(|e| anyhow!(e))?,
            )
        }
    };

    Ok((blob, Some(commitment.to_vec()), blob_proof))
}

/// get tx data(blob data) vec from blob hashes
/// and get proofs for each blobs
pub async fn get_batch_tx_data_with_proofs(
    blob_hashes: Vec<B256>,
    timestamp: u64,
    chain_spec: &ChainSpec,
    blob_proof_type: &BlobProofType,
) -> RaikoResult<Vec<(Vec<u8>, Option<Vec<u8>>, Option<Vec<u8>>)>> {
    let mut tx_data = Vec::new();
    let beacon_rpc_url: String = chain_spec.beacon_rpc.clone().ok_or_else(|| {
        RaikoError::Preflight("Beacon RPC URL is required for Taiko chains".to_owned())
    })?;
    let slot_id = block_time_to_block_slot(
        timestamp,
        chain_spec.genesis_time,
        chain_spec.seconds_per_slot,
    )?;
    // get blob data once
    let blob_data = get_blob_data(&beacon_rpc_url, slot_id).await?;
    let blobs: Vec<String> = blob_data.data.iter().map(|b| b.blob.clone()).collect();
    for hash in blob_hashes {
        let data = filter_tx_blob_beacon_with_proof(hash, blobs.clone(), blob_proof_type).await?;
        tx_data.push(data);
    }
    Ok(tx_data)
}

pub async fn filter_blockchain_event(
    provider: &ReqwestProvider,
    gen_block_event_filter: impl Fn() -> Filter,
) -> Result<Vec<Log>> {
    // Setup the filter to get the relevant events
    let filter = gen_block_event_filter();
    // Now fetch the events
    Ok(provider.get_logs(&filter).await?)
}

pub enum EventFilterCondition {
    #[allow(dead_code)]
    Hash(B256),
    Height(u64),
}

pub async fn filter_block_proposed_event(
    provider: &ReqwestProvider,
    chain_spec: ChainSpec,
    filter_condition: EventFilterCondition,
    block_num_or_batch_id: u64,
    fork: SpecId,
) -> Result<(u64, AlloyRpcTransaction, BlockProposedFork)> {
    // Get the address that emitted the event
    let l1_address = chain_spec
        .l1_contract
        .get(&fork)
        .copied()
        .ok_or_else(|| anyhow!("L1 contract address not found for fork {fork:?}"))?;

    // Get the event signature (value can differ between chains)
    let event_signature = match fork {
        SpecId::SHASTA => ShastaProposed::SIGNATURE_HASH,
        _ => bail!("unsupported block proposed event filter for fork {fork:?}"),
    };
    // Setup the filter to get the relevant events
    let logs = filter_blockchain_event(provider, || match filter_condition {
        EventFilterCondition::Hash(block_hash) => Filter::new()
            .address(l1_address)
            .at_block_hash(block_hash)
            .event_signature(event_signature),
        EventFilterCondition::Height(block_number) => Filter::new()
            .address(l1_address)
            .from_block(block_number)
            .to_block(block_number)
            .event_signature(event_signature),
    })
    .await?;

    // Run over the logs returned to find the matching event for the specified L2 block number
    // (there can be multiple blocks proposed in the same block and even same tx)
    for log in logs {
        let Some(log_struct) = LogStruct::new(
            log.address(),
            log.topics().to_vec(),
            log.data().data.clone(),
        ) else {
            bail!("Could not create log")
        };
        let (block_or_batch_id, block_propose_event) = match fork {
            SpecId::SHASTA => {
                let event = ShastaProposed::decode_log(&log_struct, false)
                    .map_err(|_| RaikoError::Anyhow(anyhow!("Could not decode log")))?;

                let mut event_data =
                    ShastaEventData::from_proposal_event(&event.data).map_err(|_| {
                        RaikoError::Anyhow(anyhow!("Could not decode Shasta event data"))
                    })?;

                let current_block_number = log.block_number.unwrap();
                let current_block = provider
                    .get_block(
                        BlockId::number(current_block_number),
                        BlockTransactionsKind::Hashes,
                    )
                    .await?;
                let Some(current_block) = current_block else {
                    bail!("No current block found for the proposal")
                };
                let timestamp = current_block.header.timestamp;

                let origin_block_number = current_block_number - 1;
                let origin_block = provider
                    .get_block_by_number(
                        alloy_rpc_types::BlockNumberOrTag::Number(origin_block_number),
                        true,
                    )
                    .await?;
                let Some(origin_block) = origin_block else {
                    bail!("No origin block found for the proposal")
                };
                let origin_block_hash = origin_block.header.hash.unwrap();
                event_data.proposal.originBlockNumber = origin_block_number;
                event_data.proposal.originBlockHash = origin_block_hash;
                event_data.proposal.timestamp = timestamp;
                (
                    raiko_lib::primitives::U256::from(event_data.proposal.id),
                    BlockProposedFork::Shasta(event_data),
                )
            }
            _ => bail!("unsupported block proposed event decode for fork {fork:?}"),
        };

        if block_or_batch_id == raiko_lib::primitives::U256::from(block_num_or_batch_id) {
            let Some(log_tx_hash) = log.transaction_hash else {
                bail!("No transaction hash in the log")
            };
            let tx = provider
                .get_transaction_by_hash(log_tx_hash)
                .await
                .expect("couldn't query the propose tx")
                .expect("Could not find the propose tx");

            return Ok((log.block_number.unwrap(), tx, block_propose_event));
        } else {
            info!("block_or_batch_id: {block_or_batch_id} != block_num_or_batch_id: {block_num_or_batch_id}");
            continue;
        }
    }

    Err(anyhow!(
        "No BlockProposed event found for proposal/batch id {block_num_or_batch_id}."
    ))
}

// pub async fn _get_block_proposed_event_by_hash(
//     provider: &ReqwestProvider,
//     chain_spec: ChainSpec,
//     l1_inclusion_block_hash: B256,
//     l2_block_number: u64,
//     fork: SpecId,
// ) -> Result<(u64, AlloyRpcTransaction, BlockProposedFork)> {
//     filter_block_proposed_event(
//         provider,
//         chain_spec,
//         EventFilterConditioin::Hash(l1_inclusion_block_hash),
//         l2_block_number,
//         fork,
//     )
//     .await
// }

pub async fn get_block_proposed_event_by_height(
    provider: &ReqwestProvider,
    chain_spec: ChainSpec,
    l1_inclusion_block_number: u64,
    block_num_or_batch_id: u64,
    fork: SpecId,
) -> Result<(u64, AlloyRpcTransaction, BlockProposedFork)> {
    filter_block_proposed_event(
        provider,
        chain_spec,
        EventFilterCondition::Height(l1_inclusion_block_number),
        block_num_or_batch_id,
        fork,
    )
    .await
}

pub async fn get_batch_blocks_and_parent_data<BDP>(
    provider: &BDP,
    block_numbers: &[u64],
) -> RaikoResult<Vec<(RethBlock, alloy_rpc_types::Block)>>
where
    BDP: BlockDataProvider,
{
    let is_first_block = block_numbers[0] == 1;
    let target_blocks = if is_first_block {
        iter::once(block_numbers[0] - 1)
            .chain(block_numbers.iter().cloned())
            .enumerate()
            .map(|(i, block_number)| (block_number, i != 0))
            .collect::<Vec<(u64, bool)>>()
    } else {
        std::iter::once(block_numbers[0] - 2)
            .chain(std::iter::once(block_numbers[0] - 1))
            .chain(block_numbers.iter().cloned())
            .enumerate()
            .map(|(i, block_number)| (block_number, i != 0))
            .collect::<Vec<(u64, bool)>>()
    };
    // Get the block and the parent block
    let blocks = provider.get_blocks(&target_blocks).await?;
    if is_first_block {
        assert!(blocks.len() == block_numbers.len() + 1);
    } else {
        assert!(blocks.len() == block_numbers.len() + 2);
    }

    info!(
        "Processing {} blocks with (num, hash) from:({:?}, {:?}) to ({:?}, {:?})",
        block_numbers.len(),
        blocks.first().unwrap().header.number,
        blocks.first().unwrap().header.hash.unwrap(),
        blocks.last().unwrap().header.number,
        blocks.last().unwrap().header.hash.unwrap(),
    );

    let pairs = blocks
        .windows(2)
        .map(|window_blocks| {
            let parent_block = &window_blocks[0];
            let prove_block = RethBlock::try_from(window_blocks[1].clone())
                .map_err(|e| {
                    RaikoError::Conversion(format!("Failed converting to reth block: {e}"))
                })
                .unwrap();
            (prove_block, parent_block.clone())
        })
        .collect();

    Ok(pairs)
}

pub async fn get_headers<BDP>(provider: &BDP, (a, b): (u64, u64)) -> RaikoResult<(Header, Header)>
where
    BDP: BlockDataProvider,
{
    let blocks = provider.get_blocks(&[(a, true), (b, false)]).await?;
    let [block_a, block_b] = blocks.as_slice() else {
        return Err(RaikoError::Preflight(
            "No block data for the requested block".to_owned(),
        ));
    };
    Ok((block_a.header.clone(), block_b.header.clone()))
}

pub async fn get_max_anchor_headers<BDP>(
    provider: &BDP,
    anchor_tx_info_vec: Vec<(u64, B256)>,
    original_block_numbers: u64,
    last_anchor_block_number: Option<u64>,
    anchor_max_offset: usize,
) -> RaikoResult<Vec<Header>>
where
    BDP: BlockDataProvider,
{
    assert!(!anchor_tx_info_vec.is_empty(), "anchor_tx_info is empty");
    let min_anchor_height = anchor_tx_info_vec[0].0;
    let lag = original_block_numbers.saturating_sub(min_anchor_height);
    let anchor_max_offset_u64 = anchor_max_offset as u64;
    info!(
        "get max anchor L1 block headers in range: ({min_anchor_height}, {original_block_numbers}), anchor_max_offset={anchor_max_offset}"
    );
    if lag > anchor_max_offset_u64 && last_anchor_block_number == Some(min_anchor_height) {
        warn!(
            "skip l1 ancestor headers due to stalled anchor: origin={original_block_numbers}, anchor={min_anchor_height}, lag={lag}"
        );
        return Ok(Vec::new());
    }
    assert!(
        lag <= anchor_max_offset_u64,
        "original_block_numbers - min_anchor_height > anchor_max_offset"
    );

    let all_init_block_numbers = (min_anchor_height..=original_block_numbers)
        .map(|block_number| (block_number, false))
        .collect::<Vec<_>>();
    // can filter out the block numbers that are already in the initial_db
    // but need to handle the block header db as well
    let initial_history_blocks = provider.get_blocks(&all_init_block_numbers).await?;

    // assert all anchor in this chain
    for anchor_tx_info in anchor_tx_info_vec {
        let block = initial_history_blocks
            .iter()
            .find(|block| block.header.number.unwrap() == anchor_tx_info.0);
        if block.is_none() {
            return Err(RaikoError::Preflight(format!(
                "Anchor block {} not found in the chain",
                anchor_tx_info.0
            )));
        }
    }

    Ok(initial_history_blocks
        .iter()
        .map(|block| block.header.clone())
        .collect())
}

// block_time_to_block_slot returns the slots of the given timestamp.
pub fn block_time_to_block_slot(
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

pub fn blob_to_bytes(blob_str: &str) -> Vec<u8> {
    hex::decode(blob_str.to_lowercase().trim_start_matches("0x")).unwrap_or_default()
}

fn calc_blob_versioned_hash(blob_str: &str) -> [u8; 32] {
    let blob_bytes = hex::decode(blob_str.to_lowercase().trim_start_matches("0x"))
        .expect("Could not decode blob");
    let blob = Blob::from_bytes(&blob_bytes).expect("Could not create blob");
    let commitment = blob_to_kzg_commitment_rust(
        &eip4844::deserialize_blob_rust(&blob).expect("Could not deserialize blob"),
        &KZG_SETTINGS.clone(),
    )
    .expect("Could not create kzg commitment from blob");
    commitment_to_version_hash(&commitment.to_bytes()).0
}

async fn get_blob_data(beacon_rpc_url: &str, block_id: u64) -> Result<GetBlobsResponse> {
    if beacon_rpc_url.contains("blobscan.com") {
        unimplemented!("blobscan.com is not supported yet")
    } else {
        get_blob_data_beacon(beacon_rpc_url, block_id).await
    }
}

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

async fn get_blob_data_beacon(beacon_rpc_url: &str, block_id: u64) -> Result<GetBlobsResponse> {
    let url = format!(
        "{}/eth/v1/beacon/blob_sidecars/{block_id}",
        beacon_rpc_url.trim_end_matches('/'),
    );
    info!("Retrieve blob from {url}.");
    let response = reqwest::get(url.clone()).await?;

    if !response.status().is_success() {
        warn!(
            "Request {url} failed with status code: {}",
            response.status()
        );
        return Err(anyhow::anyhow!(
            "Request failed with status code: {}",
            response.status()
        ));
    }

    let blobs = response.json::<GetBlobsResponse>().await?;
    ensure!(!blobs.data.is_empty(), "blob data not available anymore");
    Ok(blobs)
}

async fn filter_blob_data_beacon(blobs: Vec<String>, blob_hash: B256) -> Result<Vec<u8>> {
    // Get the blob data for the blob storing the tx list
    let tx_blob = blobs
        .iter()
        .find(|blob| {
            // calculate from plain blob
            blob_hash == calc_blob_versioned_hash(blob)
        })
        .cloned();

    if let Some(tx_blob) = &tx_blob {
        Ok(blob_to_bytes(tx_blob))
    } else {
        Err(anyhow!("couldn't find blob data matching blob hash"))
    }
}

#[cfg(test)]
mod test {
    use alloy_rlp::Decodable;
    use raiko_lib::{
        manifest::DerivationSourceManifest,
        utils::blobs::{decode_blob_data, zlib_decompress_data},
    };

    use super::*;

    #[ignore = "not run in CI as devnet changes frequently"]
    #[tokio::test]
    async fn test_shasta_blob_decoding() -> Result<()> {
        let beacon_rpc_url = "https://l1beacon.internal.taiko.xyz";
        let slot_id = 156;
        let blob_data = get_blob_data(&beacon_rpc_url, slot_id).await.expect("ok");
        println!("blob_data: {blob_data:?}");
        let blob_data = blob_to_bytes(&blob_data.data[0].blob);
        // decompress
        let decoded_blob_data = decode_blob_data(&blob_data);
        println!("decoded_blob_data: {decoded_blob_data:?}");
        let version = B256::from_slice(&decoded_blob_data[0..32]);
        let size = B256::from_slice(&decoded_blob_data[32..64]);
        let size_u64 = usize::from_be_bytes(size.as_slice()[24..32].try_into().unwrap());
        println!("version: {version:?}, size: {size:?}, size_u64: {size_u64}");
        let decompressed_blob_data =
            zlib_decompress_data(&decoded_blob_data[64..64 + size_u64]).expect("ok");
        println!("decompressed_blob_data: {decompressed_blob_data:?}");

        let proposal_manifest =
            DerivationSourceManifest::decode(&mut decompressed_blob_data.as_ref()).unwrap();
        println!("proposal_manifest: {proposal_manifest:?}");
        Ok(())
    }
}
