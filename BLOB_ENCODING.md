# Blob Encoding Specification for Raiko RealTime Proving

This document specifies the exact encoding pipeline Catalyst must use when constructing blobs for RealTime proof requests to Raiko. The prover applies these steps in reverse to recover the manifest.

---

## Overview

The encoding chain (Catalyst side):

```
DerivationSourceManifest
    -> RLP encode
    -> zlib compress
    -> prepend 64-byte Shasta header
    -> OP/Kona 4844 blob encode
    -> hex encode
    -> place in proof request `blobs[]` array
```

The decoding chain (Raiko side):

```
blobs[] hex string
    -> hex decode (131072 raw bytes)
    -> OP/Kona 4844 blob decode  (decode_blob_data)
    -> slice using blobSlice.offset + Shasta header  (blob_tx_slice_param_for_source)
    -> zlib decompress
    -> RLP decode into DerivationSourceManifest
```

---

## Step 1: Build the DerivationSourceManifest

### Struct definitions

```
DerivationSourceManifest {
    blocks: Vec<ProtocolBlockManifest>
}

ProtocolBlockManifest {
    timestamp:            u64
    coinbase:             Address (20 bytes)
    anchor_block_number:  u64
    gas_limit:            u64
    transactions:         Vec<TaikoTxEnvelope>
}
```

### Field descriptions

| Field | Type | Description |
|-------|------|-------------|
| `timestamp` | `u64` | Block timestamp. **Must exactly match** the L2 block header's `timestamp`. |
| `coinbase` | `Address` | Block beneficiary. **Must exactly match** the L2 block header's `beneficiary`. |
| `anchor_block_number` | `u64` | The L1 anchor block number for this L2 block. |
| `gas_limit` | `u64` | Block gas limit. **Must exactly match** the L2 block header's `gas_limit`. |
| `transactions` | `Vec<TaikoTxEnvelope>` | The L2 block's transactions (excluding the anchor tx, which Raiko generates). |

### Transaction encoding (TaikoTxEnvelope)

`TaikoTxEnvelope` supports these EIP-2718 transaction types:

| Type byte | Variant |
|-----------|---------|
| `0x00` (legacy, no prefix) | `TxLegacy` |
| `0x01` | `TxEip2930` |
| `0x02` | `TxEip1559` |
| `0x04` | `TxEip7702` |

Each transaction is encoded as its standard EIP-2718 envelope:
- **Legacy**: RLP(nonce, gasPrice, gasLimit, to, value, data, v, r, s) with no type prefix
- **Typed (0x01, 0x02, 0x04)**: `type_byte || RLP(tx_fields..., signature)`

Inside the RLP list, `Vec<TaikoTxEnvelope>` is encoded as an RLP list of byte strings, where each byte string is the EIP-2718 encoded transaction.

**Note:** EIP-4844 (type `0x03`) transactions are NOT supported.

### Validation rules

Raiko validates the manifest against the actual L2 block headers (`validate_input_block_param`):
- `manifest_block.timestamp == input_block.header.timestamp` (fatal)
- `manifest_block.coinbase == input_block.header.beneficiary` (fatal)
- `manifest_block.gas_limit == input_block.header.gas_limit` (fatal)

If any check fails, Raiko falls back to a default manifest which will also fail validation, causing a panic. **All three fields must match exactly.**

---

## Step 2: RLP Encode

Encode the `DerivationSourceManifest` using standard RLP:

```
RLP_LIST(                                   // DerivationSourceManifest
    RLP_LIST(                               // blocks: Vec<ProtocolBlockManifest>
        RLP_LIST(                           // ProtocolBlockManifest [0]
            RLP(timestamp),                 // u64
            RLP(coinbase),                  // Address (20 bytes, prefix 0x94)
            RLP(anchor_block_number),       // u64
            RLP(gas_limit),                 // u64
            RLP_LIST(                       // transactions: Vec<TaikoTxEnvelope>
                RLP_BYTES(tx_0_envelope),   // EIP-2718 encoded tx
                RLP_BYTES(tx_1_envelope),
                ...
            )
        ),
        RLP_LIST(                           // ProtocolBlockManifest [1]
            ...
        ),
        ...
    )
)
```

Field order is critical. Fields must be encoded in exactly this order:
1. `timestamp`
2. `coinbase`
3. `anchor_block_number`
4. `gas_limit`
5. `transactions`

### RLP encoding rules for field types

- **u64**: Standard RLP integer encoding. Single byte if 0-127, otherwise `0x80+len || big-endian bytes` (no leading zeros).
- **Address (20 bytes)**: `0x94 || 20_bytes` (RLP string of length 20).
- **Vec<TaikoTxEnvelope>**: RLP list where each element is an RLP byte string containing the full EIP-2718 envelope bytes.

### Example

For a single block with timestamp=1773729080, coinbase=0x3e95...8e56, anchor=6390, gas_limit=30000000, and one EIP-1559 tx:

```
f89d                          -- outer list (DerivationSourceManifest), 157 bytes
  f89b                        -- blocks list, 155 bytes
    f899                      -- ProtocolBlockManifest, 153 bytes
      84 69b8f538             -- timestamp: 1773729080
      94 3e95dfbb...568e56    -- coinbase: 20-byte address
      18                      -- anchor_block_number: 24 (single byte)
      84 01c9c380             -- gas_limit: 30000000
      f875                    -- transactions list, 117 bytes
        b873                  -- tx byte string, 115 bytes
          02f870...           -- EIP-1559 tx envelope (type 0x02 + RLP)
```

---

## Step 3: Zlib Compress

Compress the RLP bytes using **zlib** (RFC 1950). The output includes the standard 2-byte zlib header (`78 9c` for default compression) and 4-byte Adler-32 checksum trailer.

```python
import zlib
compressed = zlib.compress(rlp_bytes)
```

```go
// Go
var buf bytes.Buffer
w := zlib.NewWriter(&buf)
w.Write(rlpBytes)
w.Close()
compressed := buf.Bytes()
```

```rust
// Rust (using libflate)
let mut encoder = libflate::zlib::Encoder::new(Vec::new())?;
encoder.write_all(&rlp_bytes)?;
let compressed = encoder.finish().into_result()?;
```

---

## Step 4: Prepend 64-byte Shasta Header

Build a 64-byte header and prepend it to the compressed data:

```
[0:32]  version:  B256 = 0x0000000000000000000000000000000000000000000000000000000000000001
[32:64] length:   B256 = big-endian U256 of compressed data byte count
[64:..]           compressed data (from Step 3)
```

### Version field (bytes 0-31)

32 bytes, all zeros except byte 31 (the last byte) which is `0x01`.

```
00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 01
```

Raiko checks: `B256::from_slice(&data[offset..offset+32]) == B256::with_last_byte(1)`

If the version byte is at any other position, the check fails silently (returns `None`) and Raiko falls back to beacon chain fetch, which fails for RealTime, causing a panic.

### Length field (bytes 32-63)

32 bytes, big-endian U256 encoding of the compressed payload size. Raiko reads only the last 8 bytes (bytes 56-63) as a big-endian `u64`:

```rust
let size_bytes: [u8; 8] = size_b256.as_slice()[24..32].try_into().ok()?;
let blob_data_size_u64 = u64::from_be_bytes(size_bytes);
```

For a compressed payload of 158 bytes (`0x9E`):
```
00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 9E
```

### Result

The "framed payload" is: `header (64 bytes) || compressed_rlp (N bytes)` = `64 + N` bytes total.

---

## Step 5: OP/Kona 4844 Blob Encode

Encode the framed payload into a 131072-byte EIP-4844 blob using the **Optimism/Kona blob encoding** scheme.

Reference: [op-service/eth/blob.go](https://github.com/ethereum-optimism/optimism/blob/develop/op-service/eth/blob.go)

### Blob structure

A blob is 4096 field elements x 32 bytes = 131,072 bytes.

The encoding uses a version byte, 3-byte length, and packs data into field elements with the top 2 bits of each field element's first byte reserved (must be zero for BLS12-381 field validity).

### Encoding algorithm

The encoder packs `ceil(data_len / 127) * 128` bytes into the blob, working in rounds of 4 field elements (128 bytes) that encode 127 bytes of data.

**Round 0 (special):**
- `blob[0]` = top 2 bits of `data[0]` (combined with bits from `data[31]`, `data[62]`, `data[93]`)
- `blob[1]` = `0x00` (encoding version)
- `blob[2..5]` = 3-byte big-endian data length
- `blob[5..32]` = lower 6 bits of `data[0..27]`
- `blob[32]` = top 2 bits of `data[31]`, `data[62]` (combined)
- `blob[33..64]` = `data[27..58]` (with top 2 bits stripped from first byte)
- `blob[64]` = top 2 bits of `data[62]`, `data[93]` (combined)
- `blob[65..96]` = `data[58..89]` (with top 2 bits stripped)
- `blob[96]` = remaining top bits
- `blob[97..128]` = `data[89..120]` (with top 2 bits stripped)
- Then 3 reassembled bytes from the collected top bits are placed at specific positions.

**Rounds 1-1023:**
Each round encodes 127 bytes of data into 4 field elements (128 bytes), using the same bit-stripping and reassembly scheme.

### Important constraints

- The first byte of each 32-byte field element MUST have its top 2 bits clear (`byte & 0xC0 == 0`). The encoder achieves this by extracting the top 2 bits from certain data bytes and packing them into the first byte of each field element.
- All trailing bytes beyond the data must be zero.
- Maximum encodable data size: `(4 * 31 + 3) * 1024 - 4 = 126,972 bytes`

### Implementation

Use the canonical Go or Rust implementation directly:

**Go** (go-ethereum / op-service):
```go
import "github.com/ethereum-optimism/optimism/op-service/eth"
var blob eth.Blob
err := blob.FromData(framedPayload)
blobBytes := blob[:]  // 131072 bytes
```

**Rust** (kona-derive or Raiko's own encoder if available):
The encoder is the inverse of `decode_blob_data` in `lib/src/utils/blobs.rs`. Use a matching implementation from the Optimism ecosystem.

---

## Step 6: Hex Encode and Place in Request

Hex-encode the 131,072 raw blob bytes with a `0x` prefix. The result is a 262,146-character hex string.

Place it in the proof request JSON:

```json
{
  "blobs": ["0x00000000de00...0000"]
}
```

---

## Proof Request Structure

The complete proof request must include:

```json
{
  "l2_block_numbers": [<block_number>],
  "proof_type": "sgx",
  "max_anchor_block_number": <u64>,
  "last_finalized_block_hash": "0x...",
  "basefee_sharing_pctg": 75,
  "signal_slots": [],
  "sources": [
    {
      "isForcedInclusion": false,
      "blobSlice": {
        "blobHashes": ["0x01<versioned_hash_hex>"],
        "offset": 0,
        "timestamp": 0
      }
    }
  ],
  "blobs": ["0x<hex_encoded_blob>"],
  "checkpoint": {
    "block_number": <u64>,
    "block_hash": "0x...",
    "state_root": "0x..."
  },
  "blob_proof_type": "proof_of_equivalence"
}
```

### Key fields

| Field | Description |
|-------|-------------|
| `sources[].blobSlice.offset` | Byte offset within the OP-decoded blob data where the 64-byte Shasta header starts. Typically `0` for single-source proposals. |
| `sources[].blobSlice.blobHashes` | EIP-4844 versioned hashes. Must be non-empty for blob path to activate. |
| `sources[].blobSlice.timestamp` | Only used for beacon chain fetch (irrelevant for RealTime, set to `0`). |
| `blobs[]` | Hex-encoded raw blob bytes (131,072 bytes each). Order corresponds to `blobHashes` order across sources. |
| `blob_proof_type` | Must be `"proof_of_equivalence"` (snake_case, not PascalCase). |

### Blob hash computation

The versioned hash in `blobHashes` must match the KZG commitment of the blob:

```
commitment = KZG_COMMIT(blob_bytes)
versioned_hash = 0x01 || SHA256(commitment)[1:]
```

Raiko computes the KZG commitment from the provided blob bytes and verifies it matches the versioned hash.

---

## Multi-block Proposals

For proposals containing multiple L2 blocks, the `DerivationSourceManifest.blocks` vector contains one `ProtocolBlockManifest` per block, in order. Each manifest entry must match its corresponding L2 block header exactly.

```json
{
  "l2_block_numbers": [10, 11, 12],
  "sources": [
    {
      "isForcedInclusion": false,
      "blobSlice": {
        "blobHashes": ["0x01..."],
        "offset": 0,
        "timestamp": 0
      }
    }
  ],
  "blobs": ["0x..."]
}
```

The manifest inside the blob:
```
DerivationSourceManifest {
    blocks: [
        { timestamp: <block_10_ts>, coinbase: ..., anchor: ..., gas_limit: ..., txs: [...] },
        { timestamp: <block_11_ts>, coinbase: ..., anchor: ..., gas_limit: ..., txs: [...] },
        { timestamp: <block_12_ts>, coinbase: ..., anchor: ..., gas_limit: ..., txs: [...] },
    ]
}
```

---

## Forced Inclusion Sources

If the proposal has forced inclusion sources, they appear as earlier entries in `sources[]` with `isForcedInclusion: true`. The **last** source is always the normal (non-forced) source containing the proposal manifest.

Each forced inclusion source also contains a `DerivationSourceManifest` encoded the same way.

```json
{
  "sources": [
    { "isForcedInclusion": true,  "blobSlice": { ... } },
    { "isForcedInclusion": false, "blobSlice": { ... } }
  ]
}
```

---

## Quick Reference: Encoding Pipeline

```
1. Build ProtocolBlockManifest(s) with exact block header values
2. Wrap in DerivationSourceManifest { blocks: [...] }
3. RLP encode (field order: timestamp, coinbase, anchor_block_number, gas_limit, transactions)
4. Zlib compress (RFC 1950, includes 78 9c header and adler32 checksum)
5. Prepend 64-byte header:
     [0:32]  = B256(1)  = 0x00{31}01
     [32:64] = B256(compressed_len) = 0x00{24} || big_endian_u64(compressed_len)
6. OP/Kona blob encode into 131072 bytes
7. Hex encode with 0x prefix -> "0x..."
8. Place in request.blobs[] array
```
