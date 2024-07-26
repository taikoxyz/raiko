use core::{fmt::Debug, str::FromStr};

use anyhow::{anyhow, Error, Result};
use ontake::BlockProposedV2;
use reth_primitives::{
    revm_primitives::{Address, Bytes, HashMap, B256, U256},
    TransactionSigned,
};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;

use reth_primitives::{Block, Header};

#[cfg(not(feature = "std"))]
use crate::no_std::*;
use crate::{consts::ChainSpec, primitives::mpt::MptNode, utils::zlib_compress_data};

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

impl From<(Block, Header, ChainSpec, TaikoGuestInput)> for GuestInput {
    fn from(
        (block, parent_header, chain_spec, taiko): (Block, Header, ChainSpec, TaikoGuestInput),
    ) -> Self {
        Self {
            block,
            chain_spec,
            taiko,
            parent_header,
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]

pub enum BlockProposedFork {
    #[default]
    Nothing,
    Hekla(BlockProposed),
    Ontake(BlockProposedV2),
}

impl BlockProposedFork {
    pub fn blob_used(&self) -> bool {
        match self {
            BlockProposedFork::Hekla(block) => block.meta.blobUsed,
            BlockProposedFork::Ontake(block) => block.meta.blobUsed,
            _ => false,
        }
    }

    pub fn block_number(&self) -> u64 {
        match self {
            BlockProposedFork::Hekla(block) => block.meta.id,
            BlockProposedFork::Ontake(block) => block.meta.id,
            _ => 0,
        }
    }

    pub fn block_timestamp(&self) -> u64 {
        match self {
            BlockProposedFork::Hekla(block) => block.meta.timestamp,
            BlockProposedFork::Ontake(block) => block.meta.timestamp,
            _ => 0,
        }
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
    pub blob_proof_type: BlobProofType,
}

pub struct ZlibCompressError(pub String);

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

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub enum BlobProofType {
    /// Guest runs through the entire computation from blob to Kzg commitment
    /// then to version hash
    #[default]
    ProofOfCommitment,
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
            "ProofOfEquivalence" => Ok(BlobProofType::ProofOfEquivalence),
            "ProofOfCommitment" => Ok(BlobProofType::ProofOfCommitment),
            _ => Err(anyhow!("invalid blob proof type")),
        }
    }
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct TaikoProverData {
    pub prover: Address,
    pub graffiti: B256,
}

#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuestOutput {
    pub header: Header,
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

pub use hekla::*;
