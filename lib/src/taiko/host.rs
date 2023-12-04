use std::{fmt::Debug, process::Termination};

use anyhow::{Context, Result};
use ethers_core::types::{
    Block, EIP1186ProofResponse, Transaction as EthersTransaction, H160, H256, U256,
};
use hashbrown::HashMap;
use log::info;
use serde::{Deserialize, Serialize};
use zeth_primitives::{
    block::Header,
    ethers::{from_ethers_h160, from_ethers_h256, from_ethers_u256},
    taiko::*,
    transactions::{ethereum::EthereumTxEssence, Transaction, TxEssence},
    withdrawal::Withdrawal,
    Address, B256,
};

use crate::{
    block_builder::{BlockBuilder, NetworkStrategyBundle},
    consts::ChainSpec,
    host::{
        provider::{new_provider, BlockQuery, ProofQuery, ProposeQuery, Provider},
        provider_db::{self, ProviderDb},
        Init,
    },
    input::Input,
    mem_db::MemDb,
    taiko::precheck::rebuild_and_precheck_block,
};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TaikoExtra {
    pub l1_hash: B256,
    pub l1_height: u64,
    pub l2_tx_list: Vec<u8>,
    pub prover: Address,
    pub graffiti: B256,
    pub l1_signal_root: B256,
    pub l2_signal_root: B256,
    pub l2_withdrawals: Vec<Withdrawal>,
}

fn fetch_data(
    annotation: &str,
    cache_path: Option<String>,
    rpc_url: Option<String>,
    block_no: u64,
    signal_service: Address,
) -> Result<(
    Box<dyn Provider>,
    Block<H256>,
    Block<EthersTransaction>,
    B256,
    Input<EthereumTxEssence>,
)> {
    let mut provider = new_provider(cache_path, rpc_url)?;

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
    let fini_block = provider.get_full_block(&BlockQuery { block_no })?;

    info!(
        "Final {} block number: {:?} ({:?})",
        annotation,
        fini_block.number.unwrap(),
        fini_block.hash.unwrap()
    );
    info!("Transaction count: {:?}", fini_block.transactions.len());

    // Get l2 signal root by signal service
    let proof = provider.get_proof(&ProofQuery {
        block_no,
        address: H160::from_slice(signal_service.as_slice()),
        indices: Default::default(),
    })?;
    let signal_root = from_ethers_h256(proof.storage_hash);

    info!(
        "Final {} signal root: {:?} ({:?})",
        annotation,
        fini_block.number.unwrap(),
        signal_root,
    );

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
        base_fee_per_gas: Default::default(),
    };

    Ok((provider, init_block, fini_block, signal_root, input))
}

fn execute_data<N: NetworkStrategyBundle<TxEssence = EthereumTxEssence>>(
    provider: Box<dyn Provider>,
    chain_spec: ChainSpec,
    init_block: Block<H256>,
    fini_block: Block<EthersTransaction>,
) -> Result<Init<N::TxEssence>> {
    // Create the provider DB
    let provider_db =
        crate::host::provider_db::ProviderDb::new(provider, init_block.number.unwrap().as_u64());
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
        base_fee_per_gas: Default::default(),
    };
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
    let ancestor_headers = provider_db.get_ancestor_headers()?;

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

pub fn get_taiko_initial_data<N: NetworkStrategyBundle<TxEssence = EthereumTxEssence>>(
    l1_cache_path: Option<String>,
    l1_chain_spec: ChainSpec,
    l1_rpc_url: Option<String>,
    prover: Address,
    l2_cache_path: Option<String>,
    l2_chain_spec: ChainSpec,
    l2_rpc_url: Option<String>,
    l2_block_no: u64,
    graffiti: B256,
) -> Result<(Init<N::TxEssence>, TaikoExtra)> {
    let (mut l2_provider, l2_init_block, mut l2_fini_block, l2_signal_root, l2_input) = fetch_data(
        "L2",
        l2_cache_path,
        l2_rpc_url,
        l2_block_no,
        L2_SIGNAL_SERVICE.clone(),
    )?;
    // Get anchor call parameters
    let anchorCall {
        l1Hash: anchor_l1_hash,
        l1SignalRoot: anchor_l1_signal_root,
        l1Height: l1_block_no,
        parentGasUsed: l2_parent_gas_used,
    } = decode_anchor_call_args(&l2_fini_block.transactions[0].input)
        .context("failed to decode anchor arguments")?;

    let (mut l1_provider, l1_init_block, l1_fini_block, l1_signal_root, l1_input) = fetch_data(
        "L1",
        l1_cache_path,
        l1_rpc_url,
        l1_block_no,
        L1_SIGNAL_SERVICE.clone(),
    )?;

    let propose_tx = l1_provider.get_propose(&ProposeQuery {
        l1_block_no: l1_block_no + 1,
        l2_block_no: l2_block_no,
    })?;

    let propose_block_call = decode_propose_block_call_args(&propose_tx.input)
        .context("failed to get tx list from propose block tx")?;

    // 1. check l2 parent gas used
    if l2_init_block.gas_used != U256::from(l2_parent_gas_used) {
        return Err(anyhow::anyhow!("parent gas used mismatch"));
    }
    // 2. check l1 signal root
    if anchor_l1_signal_root != l1_signal_root {
        return Err(anyhow::anyhow!("l1 signal root mismatch"));
    }
    // 3. check l1 block hash
    if Some(anchor_l1_hash) != l1_fini_block.hash.map(from_ethers_h256) {
        return Err(anyhow::anyhow!("l1 block hash mismatch"));
    }

    let l2_withdrawals = l2_fini_block
        .withdrawals
        .clone()
        .unwrap_or_default()
        .into_iter()
        .map(|w| w.try_into().unwrap())
        .collect();

    let extra = TaikoExtra {
        l1_hash: anchor_l1_hash,
        l1_height: l1_block_no,
        l2_tx_list: propose_block_call.txList,
        prover,
        graffiti,
        l1_signal_root,
        l2_signal_root,
        l2_withdrawals,
    };

    // rebuild transaction list by tx_list from l1 contract
    rebuild_and_precheck_block(&mut l2_fini_block, &extra)?;

    // execute transactions and get states

    Ok((
        execute_data::<N>(l2_provider, l2_chain_spec, l2_init_block, l2_fini_block)?,
        extra,
    ))
}
