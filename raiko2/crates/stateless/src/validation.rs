use crate::{sparse::SparseState, trie::StatelessTrieExt, witness_db::WitnessDatabase};
use alloy_consensus::{BlockHeader, Header, TrieAccount};
use alloy_primitives::{map::AddressMap, B256};
use alloy_rlp::Decodable;
use reth_chainspec::{EthChainSpec, EthereumHardforks};
use reth_consensus::{Consensus, HeaderValidator};
use reth_ethereum_consensus::{validate_block_post_execution, EthBeaconConsensus};
use reth_ethereum_primitives::{Block, EthPrimitives};
use reth_evm::{execute::Executor, ConfigureEvm};
use reth_primitives_traits::{Block as _, RecoveredBlock};
use reth_stateless::{validation::StatelessValidationError, ExecutionWitness};
use reth_trie_common::{HashedPostState, KeccakKeyHasher};
use std::{collections::BTreeMap, fmt::Debug, sync::Arc};

/// Performs stateless validation of a block using the provided witness data.
#[inline]
pub fn validate_block<C, E>(
    block: Block,
    witness: ExecutionWitness,
    callers: AddressMap<TrieAccount>,
    chain_spec: Arc<C>,
    config: E,
) -> Result<B256, StatelessValidationError>
where
    C: Send + Sync + EthChainSpec<Header = Header> + EthereumHardforks + Debug,
    E: ConfigureEvm<Primitives = EthPrimitives> + Clone + 'static,
{
    stateless_validation_with_trie::<SparseState, _, _>(block, witness, callers, chain_spec, config)
}

// Performs stateless validation of a block using a custom `StatelessTrie` implementation.
//
// This is a generic version of `stateless_validation` that allows users to provide their own
// implementation of the `StatelessTrie` for custom trie backends or optimizations.
//
// See `stateless_validation` for detailed documentation of the validation process.
fn stateless_validation_with_trie<T, ChainSpec, E>(
    current_block: Block,
    witness: ExecutionWitness,
    callers: AddressMap<TrieAccount>,
    chain_spec: Arc<ChainSpec>,
    evm_config: E,
) -> Result<B256, StatelessValidationError>
where
    T: StatelessTrieExt,
    ChainSpec: Send + Sync + EthChainSpec<Header = Header> + EthereumHardforks + Debug,
    E: ConfigureEvm<Primitives = EthPrimitives> + Clone + 'static,
{
    let current_block = current_block
        .try_into_recovered()
        .map_err(|_| StatelessValidationError::SignerRecovery)?;

    let mut ancestor_headers: Vec<Header> = witness
        .headers
        .iter()
        .map(|serialized_header| {
            let bytes = serialized_header.as_ref();
            Header::decode(&mut &bytes[..])
                .map_err(|_| StatelessValidationError::HeaderDeserializationFailed)
        })
        .collect::<Result<_, _>>()?;
    // Sort the headers by their block number to ensure that they are in
    // ascending order.
    ancestor_headers.sort_by_key(|header| header.number());

    // Validate block against pre-execution consensus rules
    validate_block_consensus(chain_spec.clone(), &current_block)?;

    // Check that the ancestor headers form a contiguous chain and are not just random headers.
    let ancestor_hashes = compute_ancestor_hashes(&current_block, &ancestor_headers)?;

    // Get the last ancestor header and retrieve its state root.
    //
    // There should be at least one ancestor header, this is because we need the parent header to
    // retrieve the previous state root.
    // The edge case here would be the genesis block, but we do not create proofs for the genesis
    // block.
    let pre_state_root = match ancestor_headers.last() {
        Some(prev_header) => prev_header.state_root,
        None => return Err(StatelessValidationError::MissingAncestorHeader),
    };

    // First verify that the pre-state reads are correct
    let (mut trie, bytecode) = T::new(&witness, pre_state_root)?;
    trie.append_callers(callers);

    // Create an in-memory database that will use the reads to validate the block
    let db = WitnessDatabase::new(&trie, bytecode, ancestor_hashes);

    // Execute the block
    let executor = evm_config.executor(db);
    let output = executor
        .execute(&current_block)
        .map_err(|e| StatelessValidationError::StatelessExecutionFailed(e.to_string()))?;

    // Post validation checks
    validate_block_post_execution(
        &current_block,
        &chain_spec,
        &output.receipts,
        &output.requests,
    )
    .map_err(StatelessValidationError::ConsensusValidationFailed)?;

    // Compute and check the post state root
    let hashed_state = HashedPostState::from_bundle_state::<KeccakKeyHasher>(&output.state.state);
    let state_root = trie.calculate_state_root(hashed_state)?;
    if state_root != current_block.state_root {
        return Err(StatelessValidationError::PostStateRootMismatch {
            got: state_root,
            expected: current_block.state_root,
        });
    }

    // Return block hash
    Ok(current_block.hash_slow())
}

fn validate_block_consensus<ChainSpec>(
    chain_spec: Arc<ChainSpec>,
    block: &RecoveredBlock<Block>,
) -> Result<(), StatelessValidationError>
where
    ChainSpec: Send + Sync + EthChainSpec<Header = Header> + EthereumHardforks + Debug,
{
    let consensus = EthBeaconConsensus::new(chain_spec);

    consensus.validate_header(block.sealed_header())?;

    consensus.validate_block_pre_execution(block)?;

    Ok(())
}

fn compute_ancestor_hashes(
    current_block: &RecoveredBlock<Block>,
    ancestor_headers: &[Header],
) -> Result<BTreeMap<u64, B256>, StatelessValidationError> {
    let mut ancestor_hashes = BTreeMap::new();

    let mut child_header = current_block.header();

    // Next verify that headers supplied are contiguous
    for parent_header in ancestor_headers.iter().rev() {
        let parent_hash = child_header.parent_hash();
        ancestor_hashes.insert(parent_header.number, parent_hash);

        if parent_hash != parent_header.hash_slow() {
            return Err(StatelessValidationError::InvalidAncestorChain); // Blocks must be contiguous
        }

        if parent_header.number + 1 != child_header.number {
            return Err(StatelessValidationError::InvalidAncestorChain); // Header number should be
                                                                        // contiguous
        }

        child_header = parent_header
    }

    Ok(ancestor_hashes)
}
