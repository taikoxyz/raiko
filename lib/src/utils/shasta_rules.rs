use core::cmp::{max, min};
use reth_evm_ethereum::taiko::ANCHOR_V4_GAS_LIMIT;
use reth_primitives::revm_primitives::SpecId;
use reth_primitives::{Block, Header};
use std::cmp::max as std_max;
use tracing::warn;

use crate::consts::ForkCondition;
use crate::input::{GuestBatchInput, GuestInput};
use crate::manifest::{DerivationSourceManifest, ProtocolBlockManifest, PROPOSAL_MAX_BLOCKS};
#[cfg(not(feature = "std"))]
use crate::no_std::*;

pub const BOND_PROCESSING_DELAY: usize = 6;

pub const ANCHOR_MAX_OFFSET: usize = 128;

pub(crate) fn valid_anchor_in_normal_proposal(
    blocks: &[ProtocolBlockManifest],
    last_anchor_block_number: u64,
    l1_origin_block_number: u64,
) -> bool {
    // Check if anchor is within valid range [l1_header_number - ANCHOR_MAX_OFFSET, l1_header_number]
    // Use saturating_sub to avoid underflow when l1_header_number is small
    let min_anchor = l1_origin_block_number.saturating_sub(ANCHOR_MAX_OFFSET as u64);
    let max_anchor = l1_origin_block_number;

    // Perform all checks in a single loop:
    // 1. No anchor should regress below last_anchor_block_number
    // 2. At least one anchor should be greater than last_anchor_block_number
    // 3. Anchors should be in order (non-decreasing)
    // 4. All anchors should be within valid range
    let mut has_anchor_grow = false;
    let mut prev_anchor = None;

    for block in blocks.iter() {
        let anchor = block.anchor_block_number;

        // Check 1: no anchor should regress below last_anchor_block_number
        if anchor < last_anchor_block_number {
            warn!(
                "anchor {} is below last_anchor_block_number {}",
                anchor, last_anchor_block_number
            );
            return false;
        }

        // Check 2: at least one anchor should > last_anchor_block_number
        if anchor > last_anchor_block_number {
            has_anchor_grow = true;
        }

        // Check 3: anchors should be in order (non-decreasing)
        if let Some(prev) = prev_anchor {
            if anchor < prev {
                warn!("anchor is not in order, blocks: {:?}", blocks);
                return false;
            }
        }
        prev_anchor = Some(anchor);

        // Check 4: anchor should be within valid range
        if anchor < min_anchor || anchor > max_anchor {
            warn!(
                "anchor {} is not in range, [{}, {}]",
                anchor, min_anchor, max_anchor
            );
            return false;
        }
    }

    if !has_anchor_grow {
        warn!(
            "anchor is not growing, last_anchor_block_number: {}",
            last_anchor_block_number,
        );
    }
    has_anchor_grow
}

pub(crate) fn validate_normal_proposal_manifest(
    input: &GuestBatchInput,
    manifest: &DerivationSourceManifest,
    last_anchor_block_number: u64,
) -> bool {
    let manifest_block_number = manifest.blocks.len();
    if manifest_block_number > PROPOSAL_MAX_BLOCKS {
        warn!(
            "manifest_block_number {} > PROPOSAL_MAX_BLOCKS {}",
            manifest_block_number, PROPOSAL_MAX_BLOCKS
        );
        return false;
    }

    if !valid_anchor_in_normal_proposal(
        &manifest.blocks,
        last_anchor_block_number,
        input.taiko.batch_proposed.proposal_block_number() - 1,
    ) {
        warn!(
            "valid_anchor_in_proposal failed, last_anchor_block_number: {}",
            last_anchor_block_number
        );
        return false;
    }

    if !validate_shasta_block_gas_limit(&manifest.blocks, &input.inputs) {
        warn!("validate_shasta_block_gas_limit failed");
        return false;
    }

    if !validate_shasta_manifest_block_timesatmp(&manifest.blocks, &input) {
        warn!("validate_shasta_block_timesatmp failed");
        return false;
    }
    true
}

pub(crate) fn validate_force_inc_proposal_manifest(manifest: &DerivationSourceManifest) -> bool {
    if manifest.blocks.len() != 1 {
        warn!(
            "validate_force_inc_proposal_manifest failed, manifest: {:?}",
            manifest
        );
        return false;
    }
    true
}

pub(crate) fn validate_input_block_param(
    manifest_block: &ProtocolBlockManifest,
    input_block: &Block,
) -> bool {
    if manifest_block.timestamp != input_block.header.timestamp {
        warn!(
            "manifest_block.timestamp != input_block.header.timestamp, manifest_block.timestamp: {}, input_block.header.timestamp: {}",
            manifest_block.timestamp, input_block.header.timestamp
        );
        return false;
    }
    if manifest_block.coinbase != input_block.header.beneficiary {
        warn!(
            "manifest_block.coinbase != input_block.header.coinbase, manifest_block.coinbase: {}, input_block.header.coinbase: {}",
            manifest_block.coinbase, input_block.header.beneficiary
        );
        return false;
    }
    if manifest_block.gas_limit + ANCHOR_V4_GAS_LIMIT != input_block.header.gas_limit {
        warn!(
            "manifest_block.gas_limit != input_block.header.gas_limit, manifest_block.gas_limit: {}, input_block.header.gas_limit: {}",
            manifest_block.gas_limit, input_block.header.gas_limit
        );
        return false;
    }
    true
}

const BLOCK_GAS_LIMIT_MAX_CHANGE: u64 = 200;
const GAS_LIMIT_DENOMINATOR: u64 = 1_000_000;
const MAX_BLOCK_GAS_LIMIT: u64 = 45_000_000;
const MIN_BLOCK_GAS_LIMIT: u64 = 10_000_000;

/// validate gas limit for each block
pub fn validate_shasta_block_gas_limit(
    manifest_blocks: &[ProtocolBlockManifest],
    block_guest_inputs: &[GuestInput],
) -> bool {
    let mut parent_gas_limit = if block_guest_inputs[0].parent_header.number == 0 {
        block_guest_inputs[0].parent_header.gas_limit
    } else {
        block_guest_inputs[0].parent_header.gas_limit - ANCHOR_V4_GAS_LIMIT
    };
    for manifest_block in manifest_blocks.iter() {
        let block_gas_limit: u64 = manifest_block.gas_limit;
        let upper_limit: u64 = min(
            MAX_BLOCK_GAS_LIMIT,
            parent_gas_limit * (GAS_LIMIT_DENOMINATOR + BLOCK_GAS_LIMIT_MAX_CHANGE)
                / GAS_LIMIT_DENOMINATOR,
        );
        let lower_limit = min(
            max(
                MIN_BLOCK_GAS_LIMIT,
                parent_gas_limit * (GAS_LIMIT_DENOMINATOR - BLOCK_GAS_LIMIT_MAX_CHANGE)
                    / GAS_LIMIT_DENOMINATOR,
            ),
            upper_limit,
        );
        tracing::info!("validate_shasta_block_gas_limit, parent_gas_limit: {}, block_gas_limit: {}, upper_limit: {}, lower_limit: {}", parent_gas_limit, block_gas_limit, upper_limit, lower_limit);

        if block_gas_limit < lower_limit || block_gas_limit > upper_limit {
            warn!("block gas limit is out of bounds, block_gas_limit: {}, lower_limit: {}, upper_limit: {}", block_gas_limit, lower_limit, upper_limit);
            return false;
        }
        parent_gas_limit = block_gas_limit;
    }
    true
}

// Offset constant for lower bound, placeholder, adjust as needed for protocol.
const TIMESTAMP_MAX_OFFSET: u64 = 12 * 128;

/// validate timestamp for each block
// #### `timestamp` Validation
// Validates that block timestamps conform to the protocol rules. The 3rd party should set correct values
// according to these rules before calling this function:
// 1. **Upper bound validation**: `block.timestamp <= proposal.timestamp` must hold
// 2. **Lower bound calculation**: `lowerBound = max(parent.timestamp + 1, proposal.timestamp - TIMESTAMP_MAX_OFFSET)`
// 3. **Lower bound validation**: `block.timestamp >= lowerBound` must hold
pub fn validate_shasta_manifest_block_timesatmp(
    blocks: &[ProtocolBlockManifest],
    batch_guest_inputs: &GuestBatchInput,
) -> bool {
    let block_guest_inputs = &batch_guest_inputs.inputs;
    let proposal_timestamp = batch_guest_inputs.taiko.batch_proposed.proposal_timestamp();
    let shasta_fork_timestamp = match batch_guest_inputs
        .taiko
        .chain_spec
        .hard_forks
        .get(&SpecId::SHASTA)
    {
        Some(ForkCondition::Timestamp(timestamp)) => *timestamp,
        _ => 0,
    };
    let mut parent_timestamp = block_guest_inputs[0].parent_header.timestamp;
    for manifest_block in blocks.iter() {
        let block_timestamp = manifest_block.timestamp;
        // Upper bound validation: block.timestamp <= proposal.timestamp
        if block_timestamp > proposal_timestamp {
            warn!(
                "Block timestamp {} exceeds proposal timestamp {}",
                block_timestamp, proposal_timestamp
            );
            return false;
        }

        // Lower bound validation:
        // Calculate lowerBound = max(parent.timestamp + 1, proposal.timestamp - TIMESTAMP_MAX_OFFSET)
        // Then validate: block.timestamp >= lowerBound
        let lower_bound = std_max(
            std_max(
                parent_timestamp + 1,
                proposal_timestamp.saturating_sub(TIMESTAMP_MAX_OFFSET),
            ),
            shasta_fork_timestamp,
        );
        if block_timestamp < lower_bound {
            warn!(
                "Block timestamp {} is less than calculated lower bound {}",
                block_timestamp, lower_bound
            );
            return false;
        }
        parent_timestamp = block_timestamp;
    }
    true
}

pub(crate) fn clamp_timestamp_lower_bound(
    parent_block_ts: u64,
    proposal_ts: u64,
    shasta_fork_timestamp: u64,
) -> u64 {
    tracing::info!("clamp_timestamp_lower_bound, parent_block_ts: {}, proposal_ts: {}, shasta_fork_timestamp: {}", parent_block_ts, proposal_ts, shasta_fork_timestamp);
    let lower_bound = std_max(
        parent_block_ts + 1,
        proposal_ts.saturating_sub(TIMESTAMP_MAX_OFFSET),
    );
    if lower_bound < shasta_fork_timestamp {
        shasta_fork_timestamp
    } else {
        lower_bound
    }
}

/// Block time target for EIP-4396 base fee calculation (2 seconds)
const BLOCK_TIME_TARGET: u64 = 2;
/// Maximum gas target percentage (95%)
const MAX_GAS_TARGET_TARGET_PERCENTAGE: u64 = 95;

/// Calculates the next base fee for Shasta blocks according to EIP-4396 logic.
/// Returns the next base fee given the parent block gas/fee parameters and protocol config.
fn calc_next_shasta_base_fee(
    parent_gas_limit: u64,
    parent_gas_used: u64,
    parent_base_fee: u64,
    parent_block_time: u64,
    elasticity_multiplier: u64,
    base_fee_change_denominator: u64,
) -> u64 {
    // Calculate parentGasTarget = parent.GasLimit / elasticity_multiplier
    let parent_gas_target = parent_gas_limit / elasticity_multiplier;

    // Calculate parentAdjustedGasTarget = min(
    //     parentGasTarget * parentBlockTime / blockTimeTarget,
    //     parent.GasLimit * maxGasTargetTargetPercentage / 100
    // )
    let adjusted_target_1 = parent_gas_target
        .saturating_mul(parent_block_time)
        .checked_div(BLOCK_TIME_TARGET)
        .unwrap_or(0);
    let adjusted_target_2 = parent_gas_limit
        .saturating_mul(MAX_GAS_TARGET_TARGET_PERCENTAGE)
        .checked_div(100)
        .unwrap_or(0);
    let parent_adjusted_gas_target = min(adjusted_target_1, adjusted_target_2);

    // If the parent gasUsed is the same as the adjusted target, the baseFee remains unchanged.
    if parent_gas_used == parent_adjusted_gas_target {
        return clamp_shasta_base_fee(parent_base_fee);
    }

    if parent_gas_used > parent_adjusted_gas_target {
        // If the parent block used more gas than its target, the baseFee should increase.
        // max(1, parentBaseFee * gasUsedDelta / parentGasTarget / baseFeeChangeDenominator)
        let gas_used_delta = parent_gas_used - parent_adjusted_gas_target;
        let adjustment = parent_base_fee
            .saturating_mul(gas_used_delta)
            .checked_div(parent_gas_target)
            .unwrap_or(0)
            .checked_div(base_fee_change_denominator)
            .unwrap_or(0);

        if adjustment < 1 {
            return clamp_shasta_base_fee(parent_base_fee.saturating_add(1));
        }
        clamp_shasta_base_fee(parent_base_fee.saturating_add(adjustment))
    } else {
        // Otherwise if the parent block used less gas than its target, the baseFee should decrease.
        // max(0, parentBaseFee * gasUsedDelta / parentGasTarget / baseFeeChangeDenominator)
        let gas_used_delta = parent_adjusted_gas_target - parent_gas_used;
        let adjustment = parent_base_fee
            .saturating_mul(gas_used_delta)
            .checked_div(parent_gas_target)
            .unwrap_or(0)
            .checked_div(base_fee_change_denominator)
            .unwrap_or(0);

        let base_fee = parent_base_fee.saturating_sub(adjustment);
        // If baseFee < 0, set it to 0 (handled by saturating_sub, but we check explicitly)
        let base_fee = if base_fee > parent_base_fee {
            0
        } else {
            base_fee
        };
        clamp_shasta_base_fee(base_fee)
    }
}

/// Minimum allowed base fee for Shasta blocks (0.005 Gwei)
pub const MIN_BASE_FEE_SHASTA: u64 = 5_000_000;
/// Maximum allowed base fee for Shasta blocks (1 Gwei)
pub const MAX_BASE_FEE_SHASTA: u64 = 1_000_000_000;

/// Clamp the provided base fee to the min and max allowed for Shasta blocks.
pub fn clamp_shasta_base_fee(base_fee: u64) -> u64 {
    if base_fee < MIN_BASE_FEE_SHASTA {
        MIN_BASE_FEE_SHASTA
    } else if base_fee > MAX_BASE_FEE_SHASTA {
        MAX_BASE_FEE_SHASTA
    } else {
        base_fee
    }
}

/// Bounds the amount the base fee can change between blocks.
pub const DEFAULT_BASE_FEE_CHANGE_DENOMINATOR: u64 = 8;
/// Bounds the maximum gas limit an EIP-1559 block may have.
pub const DEFAULT_ELASTICITY_MULTIPLIER: u64 = 2;
/// Initial base fee for EIP-1559 blocks.
pub const INITIAL_BASE_FEE: u64 = 1_000_000_000;
/// CHANGE(taiko): add ShastaInitialBaseFee for Shasta fork.
pub const SHASTA_INITIAL_BASE_FEE: u64 = 25_000_000;

pub fn validate_shasta_block_base_fee(
    block_guest_inputs: &[GuestInput],
    is_first_shasta_proposal: bool,
    l2_grandparent_header: Option<&Header>,
) -> bool {
    if is_first_shasta_proposal {
        if block_guest_inputs[0].block.header.base_fee_per_gas != Some(SHASTA_INITIAL_BASE_FEE) {
            return false;
        }
    } else {
        // Calculate parent_block_time = parent.timestamp - grandparent.timestamp
        // According to EIP-4396, we need the time between parent and grandparent
        let parent_block_time = if let Some(grandparent) = l2_grandparent_header {
            block_guest_inputs[0]
                .parent_header
                .timestamp
                .saturating_sub(grandparent.timestamp)
        } else {
            // Fallback: if no parent's parent (e.g., first block ever), use default block time target
            BLOCK_TIME_TARGET
        };
        let first_block_base_fee = calc_next_shasta_base_fee(
            block_guest_inputs[0].parent_header.gas_limit,
            block_guest_inputs[0].parent_header.gas_used,
            block_guest_inputs[0]
                .parent_header
                .base_fee_per_gas
                .unwrap(),
            parent_block_time,
            DEFAULT_ELASTICITY_MULTIPLIER,
            DEFAULT_BASE_FEE_CHANGE_DENOMINATOR,
        );
        if first_block_base_fee != block_guest_inputs[0].block.header.base_fee_per_gas.unwrap() {
            warn!(
                "first_block_base_fee mismatch: expected {}, found {}",
                first_block_base_fee,
                block_guest_inputs[0].block.header.base_fee_per_gas.unwrap()
            );
            return false;
        }
    }

    // Check that each block's base fee matches the calculated next base fee
    for i in 1..block_guest_inputs.len() {
        let block = &block_guest_inputs[i].block;
        let actual_base_fee = block.header.base_fee_per_gas.unwrap();

        // Determine the base fee used for the calculation
        let prev_base_fee = block_guest_inputs[i - 1]
            .block
            .header
            .base_fee_per_gas
            .unwrap();

        // Calculate parent_block_time for this block
        // parent = block[i-1], parent's parent = block[i-1].parent_header
        // parent_block_time = parent.timestamp - parent(parent).timestamp
        let parent_block_time = block_guest_inputs[i - 1]
            .block
            .header
            .timestamp
            .saturating_sub(block_guest_inputs[i - 1].parent_header.timestamp);

        // Use the canonical calculator function for base fee
        let expected_base_fee = calc_next_shasta_base_fee(
            block_guest_inputs[i - 1].block.header.gas_limit,
            block_guest_inputs[i - 1].block.header.gas_used,
            prev_base_fee,
            parent_block_time,
            DEFAULT_ELASTICITY_MULTIPLIER,
            DEFAULT_BASE_FEE_CHANGE_DENOMINATOR,
        );

        if expected_base_fee != actual_base_fee {
            warn!(
                "Block basefee mismatch at idx {}: expected {}, found {}",
                i, expected_base_fee, actual_base_fee
            );
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::{
        valid_anchor_in_normal_proposal, validate_force_inc_proposal_manifest,
        validate_shasta_manifest_block_timesatmp,
    };
    use crate::consts::{ChainSpec, ForkCondition};
    use crate::input::{
        shasta::{BlobSlice, DerivationSource, Proposal, ShastaEventData},
        BlockProposedFork, GuestBatchInput, GuestInput, TaikoGuestBatchInput,
    };
    use crate::manifest::{DerivationSourceManifest, ProtocolBlockManifest};
    use alloy_primitives::B256;
    use reth_primitives::revm_primitives::SpecId;
    use reth_primitives::Header;
    use std::collections::BTreeMap;

    use super::calc_next_shasta_base_fee;

    #[test]
    fn test_calc_next_shasta_base_fee() {
        let parent_gas_limit = 16_000_000;
        let parent_gas_used = 15_956_512;
        let parent_base_fee = 5_000_000;
        let parent_block_time = 240;
        let elasticity_multiplier = 2;
        let base_fee_change_denominator = 8;

        let result = calc_next_shasta_base_fee(
            parent_gas_limit,
            parent_gas_used,
            parent_base_fee,
            parent_block_time,
            elasticity_multiplier,
            base_fee_change_denominator,
        );

        // Verify the result is within valid bounds
        assert!(
            result >= super::MIN_BASE_FEE_SHASTA,
            "Result {} is below minimum base fee {}",
            result,
            super::MIN_BASE_FEE_SHASTA
        );
        assert!(
            result <= super::MAX_BASE_FEE_SHASTA,
            "Result {} exceeds maximum base fee {}",
            result,
            super::MAX_BASE_FEE_SHASTA
        );

        assert_eq!(result, 5_059_102);
    }

    #[test]
    fn test_anchor_range_includes_max_offset() {
        let l1_header_number = 1000u64;
        let last_anchor_block_number = 871u64; // min_anchor - 1
        let blocks = vec![ProtocolBlockManifest {
            anchor_block_number: 872u64, // 1000 - 128
            ..Default::default()
        }];
        assert!(valid_anchor_in_normal_proposal(
            &blocks,
            last_anchor_block_number,
            l1_header_number
        ));
    }

    #[test]
    fn test_anchor_regression_is_invalid() {
        let l1_header_number = 1000u64;
        let last_anchor_block_number = 900u64;
        let blocks = vec![ProtocolBlockManifest {
            anchor_block_number: 899u64,
            ..Default::default()
        }];
        assert!(!valid_anchor_in_normal_proposal(
            &blocks,
            last_anchor_block_number,
            l1_header_number
        ));
    }

    #[test]
    fn test_force_inclusion_manifest_accepts_non_zero_fields() {
        let manifest = DerivationSourceManifest {
            blocks: vec![ProtocolBlockManifest {
                timestamp: 123,
                coinbase: Default::default(),
                anchor_block_number: 456,
                gas_limit: 789,
                transactions: vec![],
            }],
        };
        assert!(validate_force_inc_proposal_manifest(&manifest));
    }

    #[test]
    fn test_timestamp_validation_enforces_shasta_fork_time() {
        let mut chain_spec = ChainSpec::new_single(
            "test".to_string(),
            1u64.into(),
            SpecId::SHASTA,
            Default::default(),
            true,
        );
        chain_spec.hard_forks = BTreeMap::from([(SpecId::SHASTA, ForkCondition::Timestamp(150))]);

        let proposal = Proposal {
            timestamp: 200,
            originBlockNumber: 100,
            sources: vec![DerivationSource {
                isForcedInclusion: false,
                blobSlice: BlobSlice {
                    blobHashes: vec![B256::ZERO],
                    offset: 0,
                    timestamp: 0,
                },
            }],
            ..Default::default()
        };
        let event_data = ShastaEventData { proposal };

        let guest_input = GuestInput {
            parent_header: Header {
                timestamp: 90,
                ..Default::default()
            },
            ..Default::default()
        };
        let batch_input = GuestBatchInput {
            inputs: vec![guest_input],
            taiko: TaikoGuestBatchInput {
                batch_proposed: BlockProposedFork::Shasta(event_data),
                chain_spec,
                ..Default::default()
            },
        };

        let blocks = vec![ProtocolBlockManifest {
            timestamp: 120, // above parent+1 but below SHASTA fork time
            ..Default::default()
        }];
        assert!(!validate_shasta_manifest_block_timesatmp(
            &blocks,
            &batch_input
        ));
    }
}
