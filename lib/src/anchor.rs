//! Taiko's anchor related functionality and checks.

use alethia_reth_consensus::transaction::TaikoTxEnvelope;
use alloy_primitives::{uint, Address, U256};
use anyhow::{anyhow, bail, Result};
use once_cell::sync::Lazy;
use reth_primitives::Header;
use std::str::FromStr;

#[derive(Clone, Debug, Default)]
/// Base fee configuration
pub struct ProtocolBaseFeeConfig {
    /// BaseFeeConfig::adjustmentQuotient
    pub adjustment_quotient: u8,
    /// BaseFeeConfig::sharingPctg
    pub sharing_pctg: u8,
    /// BaseFeeConfig::gasIssuancePerSecond
    pub gas_issuance_per_second: u32,
    /// BaseFeeConfig::minGasExcess
    pub min_gas_excess: u64,
    /// BaseFeeConfig::maxGasIssuancePerBlock
    pub max_gas_issuance_per_block: u32,
}

/// Data required to validate a Taiko Block
#[derive(Clone, Debug, Default)]
pub struct TaikoData {
    /// header
    pub l1_header: Header,
    /// parent L1 header
    pub parent_header: Header,
    /// L2 contract
    pub l2_contract: Address,
    /// base fee sharing ratio
    pub base_fee_config: ProtocolBaseFeeConfig,
    /// gas limit to invalidate some extra txs
    /// to align with the client's mining rule
    pub gas_limit: u64,
}

/// Anchor tx gas limit
pub const ANCHOR_GAS_LIMIT: u64 = 250_000;
/// AnchorV3 tx gas limit
pub const ANCHOR_V3_GAS_LIMIT: u64 = 1_000_000;

/// The address calling the anchor transaction
pub static GOLDEN_TOUCH_ACCOUNT: Lazy<Address> = Lazy::new(|| {
    Address::from_str("0x0000777735367b36bC9B61C50022d9D0700dB4Ec")
        .expect("invalid golden touch account")
});
static GX1: U256 = uint!(0x79BE667EF9DCBBAC55A06295CE870B07029BFCDB2DCE28D959F2815B16F81798_U256);
static N: U256 = uint!(0xfffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141_U256);
static GX1_MUL_PRIVATEKEY: U256 =
    uint!(0x4341adf5a780b4a87939938fd7a032f6e6664c7da553c121d3b4947429639122_U256);
static GX2: U256 = uint!(0xc6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5_U256);

/// check the anchor signature with fixed K value
pub fn check_anchor_signature(anchor: &TaikoTxEnvelope) -> Result<()> {
    let sign = anchor
        .signature()
        .ok_or_else(|| anyhow!("anchor transaction has no signature"))?;
    if sign.r() == GX1 {
        return Ok(());
    }
    let msg_hash = anchor.signature_hash();
    let msg_hash: U256 = msg_hash.into();
    if sign.r() == GX2 {
        // when r == GX2 require s == 0 if k == 1
        // alias: when r == GX2 require N == msg_hash + *GX1_MUL_PRIVATEKEY
        if N != msg_hash + GX1_MUL_PRIVATEKEY {
            bail!(
                "r == GX2, but N != msg_hash + *GX1_MUL_PRIVATEKEY, N: {}, msg_hash: {msg_hash}, *GX1_MUL_PRIVATEKEY: {}",
                N, GX1_MUL_PRIVATEKEY
            );
        }
        return Ok(());
    }
    Err(anyhow!(
        "r != *GX1 && r != GX2, r: {}, *GX1: {}, GX2: {}",
        sign.r(),
        GX1,
        GX2
    ))
}

use alloy_sol_types::{sol, SolCall};

sol! {
    /// Anchor call
    function anchor(
        /// The L1 hash
        bytes32 l1Hash,
        /// The L1 state root
        bytes32 l1StateRoot,
        /// The L1 block number
        uint64 l1BlockId,
        /// The gas used in the parent block
        uint32 parentGasUsed
    )
        external
    {}

    /// Base fee configuration
    struct BaseFeeConfig {
        /// adjustmentQuotient for eip1559
        uint8 adjustmentQuotient;
        /// sharingPctg for fee sharing
        uint8 sharingPctg;
        /// gasIssuancePerSecond for eip1559
        uint32 gasIssuancePerSecond;
        /// minGasExcess for eip1559
        uint64 minGasExcess;
        /// maxGasIssuancePerBlock for eip1559
        uint32 maxGasIssuancePerBlock;
    }

    function anchorV2(
        /// The anchor L1 block
        uint64 _anchorBlockId,
        /// The anchor block state root
        bytes32 _anchorStateRoot,
        /// The parent gas used
        uint32 _parentGasUsed,
        /// The base fee configuration
        BaseFeeConfig calldata _baseFeeConfig
    )
        external
        nonReentrant
    {}

    function anchorV3(
        uint64 _anchorBlockId,
        bytes32 _anchorStateRoot,
        uint32 _parentGasUsed,
        BaseFeeConfig calldata _baseFeeConfig,
        bytes32[] calldata _signalSlots
    )
        external
        nonReentrant
    {}

    /// @notice Proposal-level data that applies to the entire batch of blocks.
    struct ProposalParams {
        uint48 proposalId; // Unique identifier of the proposal
        address proposer; // Address of the entity that proposed this batch
        bytes proverAuth; // Encoded ProverAuth for prover designation
    }

    /// @notice Represents a synced checkpoint
    struct Checkpoint {
        /// @notice The block number associated with the checkpoint.
        uint48 blockNumber;
        /// @notice The block hash for the end (last) L2 block in this proposal.
        bytes32 blockHash;
        /// @notice The state root for the end (last) L2 block in this proposal.
        bytes32 stateRoot;
    }

    /// @notice Processes a block within a proposal and anchors L1 data.
    /// @dev Core function that processes blocks sequentially within a proposal:
    ///      1. Anchors L1 block data for cross-chain verification
    /// @param _proposalParams Proposal-level parameters that define the overall batch.
    /// @param _checkpoint Checkpoint data for the L1 block being anchored.
    function anchorV4(
        Checkpoint calldata _checkpoint
    )
        external
        onlyValidSender
        nonReentrant
    {}

    /// @notice RealTime fork anchor — carries signal slots on the first block of each batch.
    /// @dev The first block in a RealTime batch calls this with the full signal slots array.
    ///      All subsequent blocks in the same batch call this with an empty `_signalSlots` array.
    /// @param _checkpoint Checkpoint data for the L1 block being anchored.
    /// @param _signalSlots L1 signal slots to relay (non-empty only on the first block).
    function anchorV4WithSignalSlots(
        Checkpoint calldata _checkpoint,
        bytes32[] calldata _signalSlots
    )
        external
        onlyValidSender
        nonReentrant
    {}

    // event emitted by anchorV4
    event Anchored(
        uint48 indexed proposalId,
        bool indexed isNewProposal,
        bool indexed isLowBondProposal,
        address designatedProver,
        uint48 prevAnchorBlockNumber,
        uint48 anchorBlockNumber,
        bytes32 ancestorsHash
    );
}

// todo, use compiled abi once test passes
// sol!(TaikoL2, "./res/TaikoL2.json");
// use TaikoL2::{anchor, anchorV2};

/// Decode anchor tx data
pub fn decode_anchor(bytes: &[u8]) -> Result<anchorCall> {
    anchorCall::abi_decode_validate(bytes).map_err(|e| anyhow!(e))
}

/// Decode anchor tx data for ontake fork, using anchorV2
pub fn decode_anchor_ontake(bytes: &[u8]) -> Result<anchorV2Call> {
    anchorV2Call::abi_decode_validate(bytes).map_err(|e| anyhow!(e))
}

/// Decode anchor tx data for pacaya fork, using anchorV3
pub fn decode_anchor_pacaya(bytes: &[u8]) -> Result<anchorV3Call> {
    anchorV3Call::abi_decode_validate(bytes).map_err(|e| anyhow!(e))
}

/// Decode anchor tx data for shasta fork, using anchorV4
pub fn decode_anchor_shasta(bytes: &[u8]) -> Result<anchorV4Call> {
    anchorV4Call::abi_decode_validate(bytes).map_err(|e| anyhow!(e))
}

/// Decode anchor tx data for the RealTime fork, using anchorV4WithSignalSlots.
/// Returns the decoded checkpoint and signal slots array.
/// The signal slots array is non-empty only on the first block of a batch.
pub fn decode_anchor_realtime(bytes: &[u8]) -> Result<anchorV4WithSignalSlotsCall> {
    anchorV4WithSignalSlotsCall::abi_decode_validate(bytes).map_err(|e| anyhow!(e))
}
