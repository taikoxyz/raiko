use core::{fmt::Debug, str::FromStr};

use anyhow::{anyhow, Error, Result};
use ontake::BlockProposedV2;
use pacaya::{BatchInfo, BatchProposed};
use reth_evm_ethereum::taiko::{ProtocolBaseFeeConfig, ANCHOR_GAS_LIMIT, ANCHOR_V3_GAS_LIMIT};
use reth_primitives::{
    revm_primitives::{Address, Bytes, HashMap, SpecId, B256, U256},
    Block, Header, TransactionSigned,
};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use shasta::ShastaEventData;
use tracing::error;

#[cfg(not(feature = "std"))]
use crate::no_std::*;
use crate::{
    consts::ChainSpec,
    input::shasta::Checkpoint,
    libhash::hash_proposal,
    primitives::mpt::MptNode,
    prover::{Proof, ProofCarryData},
    utils::blobs::zlib_compress_data,
};

/// Represents the state of an account's storage.
/// The storage trie together with the used storage slots allow us to reconstruct all the
/// required values.
pub type StorageEntry = (MptNode, Vec<U256>);

/// External block input.
#[serde_as]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct GuestInput {
    /// Reth block
    pub block: Block,
    /// The network to generate the proof for
    pub chain_spec: ChainSpec,
    /// Previous block header
    pub parent_header: Header,
    /// State trie of the parent block.
    pub parent_state_trie: MptNode,
    /// Maps each address with its storage trie and the used storage slots.
    pub parent_storage: HashMap<Address, StorageEntry>,
    /// The code of all unique contracts.
    pub contracts: Vec<Bytes>,
    /// List of at most 256 previous block headers
    pub ancestor_headers: Vec<Header>,
    /// Taiko specific data
    pub taiko: TaikoGuestInput,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct InputDataSource {
    pub tx_data_from_calldata: Vec<u8>,
    pub tx_data_from_blob: Vec<Vec<u8>>,
    pub blob_commitments: Option<Vec<Vec<u8>>>,
    pub blob_proofs: Option<Vec<Vec<u8>>>,
    pub blob_proof_type: BlobProofType,
    pub is_forced_inclusion: bool,
}

/// External block input.
#[serde_as]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TaikoGuestBatchInput {
    pub batch_id: u64,
    pub l1_header: Header,
    /// List of at most MAX ANCHOR OFFSET previous block headers
    pub l1_ancestor_headers: Vec<Header>,
    pub batch_proposed: BlockProposedFork,
    pub chain_spec: ChainSpec,
    pub prover_data: TaikoProverData,
    pub data_sources: Vec<InputDataSource>,
    /// L2 grandparent header for the first block in the batch (used for EIP-4396 base fee calculation)
    pub l2_grandparent_header: Option<Header>,
}

/// External block input.
#[serde_as]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct GuestBatchInput {
    pub inputs: Vec<GuestInput>,
    pub taiko: TaikoGuestBatchInput,
}

/// External aggregation input.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AggregationGuestInput {
    /// All block proofs to prove
    pub proofs: Vec<Proof>,
}

/// The raw proof data necessary to verify a proof
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RawProof {
    /// The actual proof
    pub proof: Vec<u8>,
    /// The resulting hash
    pub input: B256,
}

/// External aggregation input.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RawAggregationGuestInput {
    /// All block proofs to prove
    pub proofs: Vec<RawProof>,
}

/// External aggregation input.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AggregationGuestOutput {
    /// The resulting hash
    pub hash: B256,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZkAggregationGuestInput {
    pub image_id: [u32; 8],
    pub block_inputs: Vec<B256>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ShastaAggregationGuestInput {
    /// All block proofs to prove
    pub proofs: Vec<Proof>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ShastaRawAggregationGuestInput {
    /// All block proofs to prove
    pub proofs: Vec<RawProof>,
    pub proof_carry_data_vec: Vec<ProofCarryData>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ShastaRisc0AggregationGuestInput {
    /// Underlying RISC0 image id for the proofs being re-verified
    pub image_id: [u32; 8],
    pub block_inputs: Vec<B256>,
    pub proof_carry_data_vec: Vec<ProofCarryData>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ShastaSp1AggregationGuestInput {
    /// Verifier image id for the SP1 proofs being aggregated
    pub image_id: [u32; 8],
    /// Public inputs associated with each underlying proof
    pub block_inputs: Vec<B256>,
    pub proof_carry_data_vec: Vec<ProofCarryData>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]

pub enum BlockProposedFork {
    #[default]
    Nothing,
    Hekla(BlockProposed),
    Ontake(BlockProposedV2),
    Pacaya(BatchProposed),
    Shasta(ShastaEventData),
}

impl BlockProposedFork {
    pub fn blob_used(&self) -> bool {
        match self {
            BlockProposedFork::Hekla(block) => block.meta.blobUsed,
            BlockProposedFork::Ontake(block) => block.meta.blobUsed,
            BlockProposedFork::Pacaya(batch) => batch.info.blobHashes.len() > 0,
            BlockProposedFork::Shasta(event_data) => event_data
                .proposal
                .sources
                .iter()
                .all(|source| source.blobSlice.blobHashes.len() > 0),
            _ => false,
        }
    }

    pub fn block_number(&self) -> u64 {
        match self {
            BlockProposedFork::Hekla(block) => block.meta.id,
            BlockProposedFork::Ontake(block) => block.meta.id,
            BlockProposedFork::Pacaya(_batch) => {
                _batch.info.lastBlockId - (_batch.info.blocks.len() as u64) + 1
            }
            BlockProposedFork::Shasta(_event_data) => {
                unimplemented!("can not get block number from shasta proposal")
            }
            _ => 0,
        }
    }

    pub fn proposal_timestamp(&self) -> u64 {
        match self {
            BlockProposedFork::Shasta(event_data) => event_data.proposal.timestamp,
            _ => unimplemented!("can not get proposal timestamp from non-shasta proposal"),
        }
    }

    pub fn proposal_block_number(&self) -> u64 {
        match self {
            BlockProposedFork::Shasta(event_data) => event_data.proposal.originBlockNumber + 1,
            _ => unimplemented!("can not get proposal block number from non-shasta proposal"),
        }
    }

    pub fn base_fee_config(&self) -> ProtocolBaseFeeConfig {
        match self {
            BlockProposedFork::Ontake(block) => ProtocolBaseFeeConfig {
                adjustment_quotient: block.meta.baseFeeConfig.adjustmentQuotient,
                sharing_pctg: block.meta.baseFeeConfig.sharingPctg,
                gas_issuance_per_second: block.meta.baseFeeConfig.gasIssuancePerSecond,
                min_gas_excess: block.meta.baseFeeConfig.minGasExcess,
                max_gas_issuance_per_block: block.meta.baseFeeConfig.maxGasIssuancePerBlock,
            },
            BlockProposedFork::Pacaya(batch) => ProtocolBaseFeeConfig {
                adjustment_quotient: batch.info.baseFeeConfig.adjustmentQuotient,
                sharing_pctg: batch.info.baseFeeConfig.sharingPctg,
                gas_issuance_per_second: batch.info.baseFeeConfig.gasIssuancePerSecond,
                min_gas_excess: batch.info.baseFeeConfig.minGasExcess,
                max_gas_issuance_per_block: batch.info.baseFeeConfig.maxGasIssuancePerBlock,
            },
            BlockProposedFork::Shasta(event_data) => ProtocolBaseFeeConfig {
                adjustment_quotient: 0,
                sharing_pctg: event_data.proposal.basefeeSharingPctg,
                gas_issuance_per_second: 0,
                min_gas_excess: 0,
                max_gas_issuance_per_block: 0,
            },
            _ => ProtocolBaseFeeConfig::default(),
        }
    }

    pub fn blob_tx_slice_param(&self) -> Option<(usize, usize)> {
        match self {
            BlockProposedFork::Ontake(block) => Some((
                block.meta.blobTxListOffset as usize,
                block.meta.blobTxListLength as usize,
            )),
            BlockProposedFork::Pacaya(batch) => Some((
                batch.info.blobByteOffset as usize,
                batch.info.blobByteSize as usize,
            )),
            BlockProposedFork::Shasta(_) => {
                error!("blob_tx_slice_param not supported for shasta proposal");
                None
            }
            _ => None,
        }
    }

    pub fn blob_hash(&self) -> B256 {
        match self {
            BlockProposedFork::Hekla(block) => block.meta.blobHash,
            BlockProposedFork::Ontake(block) => block.meta.blobHash,
            // meaningless for pacaya and shasta
            _ => B256::default(),
        }
    }

    pub fn batch_info(&self) -> Option<&BatchInfo> {
        match self {
            BlockProposedFork::Pacaya(batch) => Some(&batch.info),
            BlockProposedFork::Shasta(_) => unimplemented!("Shasta batch_info implementation"),
            _ => None,
        }
    }

    pub fn gas_limit_with_anchor(&self) -> u64 {
        match self {
            BlockProposedFork::Hekla(block) => block.meta.gasLimit as u64 + ANCHOR_GAS_LIMIT,
            BlockProposedFork::Ontake(block) => block.meta.gasLimit as u64 + ANCHOR_GAS_LIMIT,
            BlockProposedFork::Pacaya(batch) => batch.info.gasLimit as u64 + ANCHOR_V3_GAS_LIMIT,
            _ => 0,
        }
    }

    pub fn proposer(&self) -> Address {
        match self {
            BlockProposedFork::Shasta(event_data) => event_data.proposal.proposer,
            _ => unimplemented!("proposer is not supported for non-shasta fork"),
        }
    }

    pub fn fork_spec(&self) -> SpecId {
        match self {
            BlockProposedFork::Shasta(_) => SpecId::SHASTA,
            BlockProposedFork::Pacaya(_) => SpecId::PACAYA,
            BlockProposedFork::Hekla(_) => SpecId::HEKLA,
            BlockProposedFork::Ontake(_) => SpecId::ONTAKE,
            _ => unimplemented!("unsupported fork spec"),
        }
    }

    pub fn is_shasta(&self) -> bool {
        match self {
            BlockProposedFork::Shasta(_) => true,
            _ => false,
        }
    }

    pub fn proposal_id(&self) -> u64 {
        match self {
            BlockProposedFork::Shasta(event_data) => event_data.proposal.id,
            BlockProposedFork::Pacaya(batch) => batch.meta.batchId,
            _ => 0,
        }
    }

    pub fn proposal_hash(&self) -> B256 {
        match self {
            BlockProposedFork::Shasta(event_data) => hash_proposal(&event_data.proposal),
            _ => B256::ZERO,
        }
    }

    pub fn parent_proposal_hash(&self) -> B256 {
        match self {
            BlockProposedFork::Shasta(event_data) => event_data.proposal.parentProposalHash,
            _ => B256::ZERO,
        }
    }

    pub fn all_source_blob_hashes(&self) -> Vec<Vec<B256>> {
        match self {
            BlockProposedFork::Shasta(event_data) => event_data
                .proposal
                .sources
                .iter()
                .map(|s| s.blobSlice.blobHashes.clone())
                .collect(),
            BlockProposedFork::Pacaya(batch_proposed) => {
                vec![batch_proposed.info.blobHashes.clone()]
            }
            _ => Vec::new(),
        }
    }

    /// Returns the (offset, size) slice for the compressed manifest payload for a given Shasta
    /// derivation source, using the decoded blob data concatenation for that source.
    ///
    /// Shasta blob layout (per `DerivationSource.blobSlice`):
    /// - `[offset, offset+32)`: version (`bytes32`, must equal `...01`)
    /// - `[offset+32, offset+64)`: size (`bytes32`, `uint64` stored in last 8 bytes)
    /// - `[offset+64, offset+64+size)`: compressed payload
    pub fn blob_tx_slice_param_for_source(
        &self,
        source_idx: usize,
        decoded_blob_data_concat: &[u8],
    ) -> Option<(usize, usize)> {
        const SHASTA_BLOB_DATA_PREFIX_SIZE: usize = 64;
        use crate::utils::blobs::BLOB_DATA_CAPACITY as BLOB_BYTES;

        let BlockProposedFork::Shasta(event_data) = self else {
            return None;
        };

        let source = event_data.proposal.sources.get(source_idx)?;
        if source.blobSlice.blobHashes.is_empty() {
            return None;
        }

        let offset = source.blobSlice.offset as usize;
        if offset > BLOB_BYTES.saturating_sub(SHASTA_BLOB_DATA_PREFIX_SIZE) {
            return None;
        }
        if offset + SHASTA_BLOB_DATA_PREFIX_SIZE > decoded_blob_data_concat.len() {
            return None;
        }

        let version = B256::from_slice(&decoded_blob_data_concat[offset..offset + 32]);
        if version != B256::with_last_byte(1) {
            return None;
        }

        let size_b256 = B256::from_slice(&decoded_blob_data_concat[offset + 32..offset + 64]);
        let size_bytes: [u8; 8] = size_b256.as_slice()[24..32].try_into().ok()?;
        let blob_data_size_u64 = u64::from_be_bytes(size_bytes);
        let blob_data_size: usize = usize::try_from(blob_data_size_u64).ok()?;

        let start = offset + SHASTA_BLOB_DATA_PREFIX_SIZE;
        let end = start.checked_add(blob_data_size)?;
        if end > decoded_blob_data_concat.len() {
            return None;
        }

        Some((start, blob_data_size))
    }
}

#[serde_as]
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TaikoGuestInput {
    /// header
    pub l1_header: Header,
    pub tx_data: Vec<u8>,
    pub anchor_tx: Option<TransactionSigned>,
    pub block_proposed: BlockProposedFork,
    pub prover_data: TaikoProverData,
    pub blob_commitment: Option<Vec<u8>>,
    pub blob_proof: Option<Vec<u8>>,
    pub blob_proof_type: BlobProofType,
    // extra data: is force inclusion flag
    pub extra_data: Option<bool>,
}

pub struct ZlibCompressError(pub String);

// for non-taiko chain use only. As we need to decompress txs buffer in raiko, if txs comes from non-taiko chain,
// we simply compress before sending to raiko, then, decompress will give the same txs inside raiko.
impl TryFrom<Vec<TransactionSigned>> for TaikoGuestInput {
    type Error = ZlibCompressError;

    fn try_from(value: Vec<TransactionSigned>) -> Result<Self, Self::Error> {
        let tx_data = zlib_compress_data(&alloy_rlp::encode(&value))
            .map_err(|e| ZlibCompressError(e.to_string()))?;
        Ok(Self {
            tx_data,
            ..Self::default()
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[serde(rename_all = "snake_case")]
pub enum BlobProofType {
    /// Guest runs through the entire computation from blob to Kzg commitment
    /// then to version hash
    #[default]
    KzgVersionedHash,
    /// Simplified Proof of Equivalence with fiat input in non-aligned field
    /// Referencing https://notes.ethereum.org/@dankrad/kzg_commitments_in_proofs
    /// with impl details in https://github.com/taikoxyz/raiko/issues/292
    /// Guest proves the KZG evaluation of the a fiat-shamir input x and output result y
    ///      x = sha256(sha256(blob), kzg_commit(blob))
    ///      y = f(x)
    /// where f is the KZG polynomial
    ProofOfEquivalence,
}

impl FromStr for BlobProofType {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            "proof_of_equivalence" => Ok(BlobProofType::ProofOfEquivalence),
            "kzg_versioned_hash" => Ok(BlobProofType::KzgVersionedHash),
            _ => Err(anyhow!("invalid blob proof type")),
        }
    }
}
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct TaikoProverData {
    pub actual_prover: Address,
    pub graffiti: B256,
    pub checkpoint: Option<Checkpoint>,
    pub last_anchor_block_number: Option<u64>,
}

#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuestOutput {
    pub header: Header,
    pub hash: B256,
}

#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuestBatchOutput {
    pub blocks: Vec<Block>,
    pub hash: B256,
}

#[cfg(feature = "std")]
use std::path::Path;
#[cfg(feature = "std")]
use std::path::PathBuf;

#[cfg(feature = "std")]
pub fn get_input_path(dir: &Path, block_number: u64, network: &str) -> PathBuf {
    dir.join(format!("input-{network}-{block_number}.bin"))
}

mod hekla;
pub mod ontake;
pub mod pacaya;
pub mod shasta;

pub use hekla::*;

#[cfg(test)]
mod test {
    use super::*;
    use crate::input::shasta::{BlobSlice, DerivationSource, Proposal, ShastaEventData};
    use alloy_primitives::B256;

    #[test]
    fn test_guest_input_se_de() {
        let input = GuestInput {
            block: Block::default(),
            chain_spec: ChainSpec::default(),
            parent_header: Header::default(),
            parent_state_trie: MptNode::default(),
            parent_storage: HashMap::default(),
            contracts: vec![],
            ancestor_headers: vec![],
            taiko: TaikoGuestInput::default(),
        };
        let input_ser = serde_json::to_string(&input).unwrap();
        let input_de: GuestInput = serde_json::from_str(&input_ser).unwrap();
        print!("{:?}", input_de);
    }

    #[test]
    fn test_guest_input_value_sede() {
        let input = GuestInput {
            block: Block::default(),
            chain_spec: ChainSpec::default(),
            parent_header: Header::default(),
            parent_state_trie: MptNode::default(),
            parent_storage: HashMap::default(),
            contracts: vec![],
            ancestor_headers: vec![],
            taiko: TaikoGuestInput::default(),
        };
        let input_ser = serde_json::to_value(&input).unwrap();
        let input_de: GuestInput = serde_json::from_value(input_ser).unwrap();
        print!("{:?}", input_de);
    }

    #[test]
    fn test_shasta_blob_slice_offset_bounds() {
        const BLOB_BYTES: usize = 4096 * 32;
        const SHASTA_BLOB_DATA_PREFIX_SIZE: usize = 64;
        let offset = (BLOB_BYTES - SHASTA_BLOB_DATA_PREFIX_SIZE + 1) as u32;

        let proposal = Proposal {
            sources: vec![DerivationSource {
                isForcedInclusion: false,
                blobSlice: BlobSlice {
                    blobHashes: vec![B256::ZERO],
                    offset,
                    timestamp: 0,
                },
            }],
            ..Default::default()
        };
        let event_data = ShastaEventData { proposal };
        let fork = BlockProposedFork::Shasta(event_data);

        let mut decoded = vec![0u8; (offset as usize) + SHASTA_BLOB_DATA_PREFIX_SIZE];
        let version = B256::with_last_byte(1);
        decoded[(offset as usize)..(offset as usize + 32)].copy_from_slice(version.as_slice());

        assert!(
            fork.blob_tx_slice_param_for_source(0, &decoded).is_none(),
            "offset beyond blob prefix bound should be rejected"
        );
    }
}
