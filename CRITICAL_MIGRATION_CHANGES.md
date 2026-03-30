# Critical Migration Changes: Shasta -> RealTime

This document lists every Shasta validation check that was removed or modified
for the RealTime proving path, with rationale for each change.

Source files:
- `lib/src/utils/shasta_rules.rs` — Shasta: `validate_normal_proposal_manifest`, RealTime: `validate_realtime_proposal_manifest`
- `lib/src/utils/realtime.rs` — calls `validate_realtime_proposal_manifest` instead of `validate_normal_proposal_manifest`

---

## Validations REMOVED for RealTime

### 1. Anchor Block Range Check (`valid_anchor_in_normal_proposal`)

**Shasta behavior:**
Validates that every `anchor_block_number` in the manifest falls within
`[proposal_block_number - 1 - 128, proposal_block_number - 1]`.
This ensures the anchor's blockhash is accessible on-chain via `BLOCKHASH` opcode
(only the last 256 blocks are available).

Four sub-checks:
1. No anchor regresses below `last_anchor_block_number`
2. At least one anchor is greater than `last_anchor_block_number` (must grow)
3. Anchors are non-decreasing across blocks
4. Every anchor is within `[l1_origin - ANCHOR_MAX_OFFSET, l1_origin]` where `l1_origin = proposal_block_number - 1`

**Why removed for RealTime:**
There is no on-chain proposal yet at proving time. `proposal_block_number()` returns
a meaningless value (0 or `maxAnchorBlockNumber`), making the range calculation invalid.
The 256-block constraint is enforced by Catalyst when it eventually posts the proposal
on-chain — it is a posting-time constraint, not a proving-time constraint.

**Risk if re-enabled incorrectly:**
Valid proofs would be rejected because the anchor falls outside a range derived from
a non-existent L1 inclusion block.

---

### 2. Block Timestamp Upper Bound (`validate_shasta_manifest_block_timesatmp`)

**Shasta behavior:**
Validates `block.timestamp <= proposal.timestamp` for every block in the manifest.
The proposal timestamp is the L1 block timestamp at which the proposal was included.

**Why removed for RealTime:**
`proposal_timestamp()` returns `0` for RealTime (no on-chain proposal exists).
Every block with a non-zero timestamp would fail. The proposer sets timestamps
before the proposal is posted, so this constraint cannot be checked by the prover.

**Risk if re-enabled incorrectly:**
Every RealTime proof request would be rejected.

---

### 3. Block Timestamp Lower Bound (`validate_shasta_manifest_block_timesatmp`)

**Shasta behavior:**
Validates `block.timestamp >= max(parent.timestamp + 1, proposal.timestamp - TIMESTAMP_MAX_OFFSET, fork_timestamp)`.
Prevents blocks from using timestamps too far in the past relative to the proposal.

**Why removed for RealTime:**
Same as above — `proposal_timestamp()` is `0`, making the lower bound calculation
produce incorrect results. Additionally, `TIMESTAMP_MAX_OFFSET` subtracted from `0`
saturates to `0`, collapsing the lower bound to `max(parent.timestamp + 1, 0, fork_timestamp)`,
which may or may not be correct depending on fork activation.

**Risk if re-enabled incorrectly:**
Depending on the fork activation config, this could silently pass or reject valid proofs.

---

## Validations KEPT for RealTime

### 1. Block Count Limit

```
manifest.blocks.len() <= PROPOSAL_MAX_BLOCKS (384)
```

Still applies — prevents oversized manifests regardless of fork.

### 2. Gas Limit Match (`validate_shasta_block_gas_limit`)

```
manifest_block.gas_limit matches input_block.header.gas_limit
(adjusted for anchor tx gas)
```

Still applies — gas limit is a physical block property, not dependent on L1 inclusion.

### 3. Input Block Parameter Match (`validate_input_block_param`)

```
manifest_block.timestamp  == input_block.header.timestamp
manifest_block.coinbase   == input_block.header.beneficiary
manifest_block.gas_limit  == input_block.header.gas_limit
```

Still applies — this is the final assertion that ensures the manifest matches the
actual L2 block headers. This is the last line of defense and causes a panic if it fails.
Located at `realtime.rs:171`.

### 4. Base Fee Validation (`validate_shasta_block_base_fee`)

Still applies — checked after manifest validation passes. If it fails, the code falls
back to the default manifest (which will then fail `validate_input_block_param`).

---

## Summary Table

| Check | Shasta | RealTime | Reason |
|-------|--------|----------|--------|
| Anchor range `[l1_origin - 128, l1_origin]` | Yes | **No** | No L1 inclusion block exists yet |
| Anchor non-regression | Yes | **No** | Part of anchor range check |
| Anchor growth | Yes | **No** | Part of anchor range check |
| Anchor ordering | Yes | **No** | Part of anchor range check |
| Timestamp upper bound (`<= proposal_ts`) | Yes | **No** | `proposal_timestamp()` is 0 |
| Timestamp lower bound (`>= lower_bound`) | Yes | **No** | `proposal_timestamp()` is 0 |
| Block count limit | Yes | Yes | |
| Gas limit match | Yes | Yes | |
| Manifest vs block header match | Yes | Yes | |
| Base fee validation | Yes | Yes | |
