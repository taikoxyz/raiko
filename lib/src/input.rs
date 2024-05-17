// Copyright 2023 RISC Zero, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
use core::fmt::Debug;
#[cfg(feature = "std")]
use std::path::PathBuf;

use alloy_consensus::Header as AlloyConsensusHeader;
use alloy_rpc_types::Withdrawal as AlloyWithdrawal;
use alloy_sol_types::{sol, SolCall};
use anyhow::{anyhow, Result};
use raiko_primitives::{mpt::MptNode, Address, Bytes, B256, U256};
use revm::primitives::HashMap;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;

#[cfg(not(feature = "std"))]
use crate::no_std::*;
use crate::{consts::ChainSpec, serde_with::RlpBytes, serde_with::RlpHexBytes};

/// Represents the state of an account's storage.
/// The storage trie together with the used storage slots allow us to reconstruct all the
/// required values.
pub type StorageEntry = (MptNode, Vec<U256>);

/// External block input.
#[serde_as]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct GuestInput {
    /// The network to generate the proof for
    pub chain_spec: ChainSpec,
    /// Block number
    pub block_number: u64,
    /// Block gas used
    pub gas_used: u64,
    /// Block hash - for reference!
    pub block_hash_reference: B256,
    /// Block header - for reference!
    #[serde_as(as = "RlpBytes")]
    pub block_header_reference: AlloyConsensusHeader,
    /// Previous block header
    #[serde_as(as = "RlpBytes")]
    pub parent_header: AlloyConsensusHeader,
    /// Address to which all priority fees in this block are transferred.
    pub beneficiary: Address,
    /// Scalar equal to the current limit of gas expenditure per block.
    pub gas_limit: u64,
    /// Scalar corresponding to the seconds since Epoch at this block's inception.
    pub timestamp: u64,
    /// Arbitrary byte array containing data relevant for this block.
    pub extra_data: Bytes,
    /// Hash previously used for the PoW now containing the RANDAO value.
    pub mix_hash: B256,
    /// List of stake withdrawals for execution
    pub withdrawals: Vec<AlloyWithdrawal>,
    /// State trie of the parent block.
    pub parent_state_trie: MptNode,
    /// Maps each address with its storage trie and the used storage slots.
    pub parent_storage: HashMap<Address, StorageEntry>,
    /// The code of all unique contracts.
    pub contracts: Vec<Bytes>,
    /// List of at most 256 previous block headers
    #[serde_as(as = "Vec<RlpBytes>")]
    pub ancestor_headers: Vec<AlloyConsensusHeader>,
    /// Base fee per gas
    pub base_fee_per_gas: u64,

    pub blob_gas_used: Option<u64>,
    pub excess_blob_gas: Option<u64>,
    pub parent_beacon_block_root: Option<B256>,

    /// Taiko specific data
    pub taiko: TaikoGuestInput,
}

#[serde_as]
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TaikoGuestInput {
    /// header
    #[serde_as(as = "RlpBytes")]
    pub l1_header: AlloyConsensusHeader,
    pub tx_data: Vec<u8>,
    pub anchor_tx: String,
    pub block_proposed: BlockProposed,
    pub prover_data: TaikoProverData,
    pub tx_blob_hash: Option<B256>,
    pub skip_verify_blob: bool,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct TaikoProverData {
    pub prover: Address,
    pub graffiti: B256,
}

#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GuestOutput {
    Success {
        #[serde_as(as = "RlpHexBytes")]
        header: AlloyConsensusHeader,
        hash: B256,
    },
    Failure,
}

sol! {
    function anchor(
        bytes32 l1Hash,
        bytes32 l1StateRoot,
        uint64 l1BlockId,
        uint32 parentGasUsed
    )
        external
    {}
}

#[inline]
pub fn decode_anchor(bytes: &[u8]) -> Result<anchorCall> {
    anchorCall::abi_decode(bytes, true).map_err(|e| anyhow!(e))
    // .context("Invalid anchor call")
}

sol! {
    #[derive(Debug, Default, Deserialize, Serialize)]
    struct EthDeposit {
        address recipient;
        uint96 amount;
        uint64 id;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    struct BlockMetadata {
        bytes32 l1Hash;
        bytes32 difficulty;
        bytes32 blobHash; //or txListHash (if Blob not yet supported)
        bytes32 extraData;
        bytes32 depositsHash;
        address coinbase; // L2 coinbase
        uint64 id;
        uint32 gasLimit;
        uint64 timestamp;
        uint64 l1Height;
        uint16 minTier;
        bool blobUsed;
        bytes32 parentMetaHash;
        address sender;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    struct BlockParams {
        address assignedProver;
        address coinbase;
        bytes32 extraData;
        bytes32 parentMetaHash;
        HookCall[] hookCalls;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    struct HookCall {
        address hook;
        bytes data;
    }

    #[derive(Debug)]
    struct Transition {
        bytes32 parentHash;
        bytes32 blockHash;
        bytes32 stateRoot;
        bytes32 graffiti;
    }

    #[derive(Debug, Default, Deserialize, Serialize)]
    event BlockProposed(
        uint256 indexed blockId,
        address indexed assignedProver,
        uint96 livenessBond,
        BlockMetadata meta,
        EthDeposit[] depositsProcessed
    );

    #[derive(Debug)]
    struct TierProof {
        uint16 tier;
        bytes data;
    }

    #[derive(Debug)]
    function proposeBlock(
        bytes calldata params,
        bytes calldata txList
    )
    {}

    function proveBlock(uint64 blockId, bytes calldata input) {}
}

#[cfg(feature = "std")]
use std::path::Path;

#[cfg(feature = "std")]
pub fn get_input_path(dir: &Path, block_number: u64, network: &str) -> PathBuf {
    dir.join(format!("input-{network}-{block_number}.bin"))
}

#[cfg(test)]
mod tests {
    extern crate alloc;
    use super::*;

    #[test]
    fn input_serde_roundtrip() {
        let input = GuestInput::default();
        let _: GuestInput = bincode::deserialize(&bincode::serialize(&input).unwrap()).unwrap();
    }
}
