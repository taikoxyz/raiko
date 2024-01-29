//! Prepare Input for guest
use std::fmt::Debug;
use thiserror::Error as ThisError;
use zeth_lib::{
    block_builder::NetworkStrategyBundle, consts::{get_taiko_chain_spec, ETH_MAINNET_CHAIN_SPEC},EthereumTxEssence
};

use super::{
    context::Context, error::Result, request::{ProofRequest, PseZkRequest, SgxRequest}, taiko_extra::{get_taiko_initial_data, TaikoExtra}
    
};

use util::{provider::{file_provider::cache_file_path, new_provider, BlockQuery, ProofQuery, Provider}, provider_db};
use util::Init;

/// prepare input data for guests
pub async fn prepare_input<N: NetworkStrategyBundle<TxEssence = EthereumTxEssence>>(
    ctx: &Context,
    req: &ProofRequest,
) -> Result<(Init<N::TxEssence>, TaikoExtra)>
where
    <N::Database as revm::primitives::db::Database>::Error: Debug,
{
    match req {
        ProofRequest::Sgx(SgxRequest {
            block,
            l1_rpc,
            l2_rpc,
            prover,
            graffiti,
        }) => {
            let l2_block = *block;
            let l2_cache_path = cache_file_path(&ctx.cache_path, l2_block, false);

            let l2_spec = get_taiko_chain_spec(&ctx.l2_chain);
            let l2_rpc = l2_rpc.to_owned();

            let l1_spec = ETH_MAINNET_CHAIN_SPEC.clone();
            let l1_cache_path = cache_file_path(&ctx.cache_path, l2_block, true);
            let l1_rpc = l1_rpc.to_owned();
            let prover = prover.to_owned();
            let graffiti = *graffiti;
            // run sync task in blocking mode
            tokio::task::spawn_blocking(move || {
                get_taiko_initial_data::<N>(
                    Some(l1_cache_path.into_os_string().into_string().unwrap()),
                    l1_spec,
                    Some(l1_rpc),
                    prover,
                    Some(l2_cache_path.into_os_string().into_string().unwrap()),
                    l2_spec,
                    Some(l2_rpc),
                    l2_block,
                    graffiti,
                )
            })
            .await?
            .map_err(Into::into)
        }
        ProofRequest::PseZk(PseZkRequest { .. }) => todo!(),
    }
}




use ethers_core::types::{Block, Transaction as EthersTransaction, H160, H256};
use tracing::info;
use zeth_primitives::{
    ethers::{from_ethers_h160, from_ethers_h256, from_ethers_u256},
    Address, B256,
};

use zeth_lib::{
    block_builder::BlockBuilder,
    consts::ChainSpec,
    input::Input,
    taiko::Layer,
};

#[allow(clippy::type_complexity)]
pub fn fetch_data(
    annotation: &str,
    cache_path: Option<String>,
    rpc_url: Option<String>,
    block_no: u64,
    signal_service: Address,
    layer: Layer,
) -> Result<(
    Box<dyn Provider>,
    Block<H256>,
    Block<EthersTransaction>,
    B256,
    Input<EthereumTxEssence>,
)> {
    let mut provider = new_provider(cache_path, rpc_url)?;

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
        base_fee_per_gas: from_ethers_u256(fini_block.base_fee_per_gas.unwrap_or_default()),
    };

    Ok((provider, init_block, fini_block, signal_root, input))
}

pub fn execute_data<N: NetworkStrategyBundle<TxEssence = EthereumTxEssence>>(
    provider: Box<dyn Provider>,
    chain_spec: ChainSpec,
    init_block: Block<H256>,
    input: Input<EthereumTxEssence>,
    fini_block: Block<EthersTransaction>,
) -> Result<Init<EthereumTxEssence>> {
    // Create the provider DB
    let provider_db = provider_db::ProviderDb::new(provider, init_block.number.unwrap().as_u64());
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




