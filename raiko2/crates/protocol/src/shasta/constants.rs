//! Shasta protocol constants.

/// Version number for Shasta payloads.
pub const SHASTA_PAYLOAD_VERSION: u8 = 0x1;

/// The maximum number of bytes in a blob.
pub const BLOB_BYTES: usize = 4096 * 31; // 126,976 bytes

/// The maximum number of blocks allowed in a proposal.
pub const PROPOSAL_MAX_BLOCKS: usize = 384;

/// The maximum timestamp offset from the proposal origin timestamp.
pub const TIMESTAMP_MAX_OFFSET: u64 = 12 * 32;

/// The minimum anchor block number offset from the proposal origin block number.
pub const ANCHOR_MIN_OFFSET: u64 = 2;

/// The maximum anchor block number offset from the proposal origin block number.
pub const ANCHOR_MAX_OFFSET: u64 = 128;

/// Maximum valid value for BondType.
pub const MAX_BOND_TYPE: u8 = 2;
