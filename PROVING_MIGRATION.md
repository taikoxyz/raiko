# RealTime Fork: Proving Pipeline Reference

> Documents the existing Shasta proving flow step-by-step, and specifies the parallel
> **RealTime fork** that must be wired in alongside it — following the same pattern used
> for Hekla → Ontake → Pacaya → Shasta. All RealTime code is **additive**; no existing
> Shasta code is modified or removed.
>
> See [PROTOCOL_MIGRATION_REAL_TIME_FORK.md](PROTOCOL_MIGRATION_REAL_TIME_FORK.md) for
> the on-chain contract changes this fork implements.

---

## Table of Contents

1. [Step 1 — Receiving the Proving Request](#step-1--receiving-the-proving-request)
2. [Step 2 — Parsing L1 Proposal Events](#step-2--parsing-l1-proposal-events)
3. [Step 3 — Constructing GuestInput (Preflight)](#step-3--constructing-guestinput-preflight)
4. [Step 4 — Anchor Transaction Decoding & Validation](#step-4--anchor-transaction-decoding--validation)
5. [Step 5 — Transaction Generation & Execution](#step-5--transaction-generation--execution)
6. [Step 6 — Computing GuestOutput & ProtocolInstance](#step-6--computing-guestoutput--protocolinstance)
7. [Step 7 — Commitment & Hash Construction](#step-7--commitment--hash-construction)
8. [Step 8 — Proof Generation (Prover Dispatch)](#step-8--proof-generation-prover-dispatch)
9. [Step 9 — ProofCarryData & Aggregation](#step-9--proofcarrydata--aggregation)
10. [Step 10 — Proof Output & API Response](#step-10--proof-output--api-response)

---

## Step 1 — Receiving the Proving Request

### Existing Shasta Flow

The prover receives a request via HTTP API (V2/V3). The Shasta flow uses the **batch/proposal** path.

**Entry points:**

- V3 Shasta batch handler: [`host/src/server/api/v3/proof/batch_handler.rs`](host/src/server/api/v3/proof/batch_handler.rs)
- V2 proof handler: [`host/src/server/api/v2/proof/mod.rs:48`](host/src/server/api/v2/proof/mod.rs#L48)

**Key types:**

```rust
// core/src/interfaces.rs:495-531
pub struct ProofRequest {
    pub block_number: u64,
    pub batch_id: u64,                        // proposal ID in Shasta
    pub l1_inclusion_block_number: u64,        // L1 block where proposal was included
    pub l2_block_numbers: Vec<u64>,            // L2 blocks covered by this proposal
    pub network: String,
    pub l1_network: String,
    pub graffiti: B256,
    pub prover: Address,
    pub proof_type: ProofType,
    pub blob_proof_type: BlobProofType,
    pub prover_args: HashMap<String, Value>,
    pub checkpoint: Option<ShastaProposalCheckpoint>,   // optional checkpoint
    pub last_anchor_block_number: Option<u64>,
    pub cached_event_data: Option<BlockProposedFork>,
    pub gpu_number: Option<u32>,
}
```

```rust
// core/src/interfaces.rs:661-666
pub struct ShastaProposalCheckpoint {
    pub block_number: u64,
    pub block_hash: B256,
    pub state_root: B256,
}
```

**Request flow:**

1. JSON request arrives at HTTP handler
2. Merged with default config via `ProofRequestOpt::merge()` ([`core/src/interfaces.rs:826`](core/src/interfaces.rs#L826))
3. Validated and converted to `ProofRequest` via `TryFrom<ProofRequestOpt>` ([`core/src/interfaces.rs:834`](core/src/interfaces.rs#L834))
4. Request key and entity created, then dispatched to the Actor/Backend system

The Shasta request specifically populates:

- `batch_id` = proposal ID
- `l2_block_numbers` = blocks in the proposal
- `l1_inclusion_block_number` = L1 block containing the `Proposed` event
- `checkpoint` = previous finalized checkpoint
- `last_anchor_block_number` = hint for anchor block

### RealTime Fork — New Code

Since the prover must generate the proof **before** the proposal exists on-chain, the request carries the data the caller will later submit to `RealTimeInbox.propose(data, checkpoint, proof)`.

**New request type** in `core/src/interfaces.rs`:

```rust
pub struct RealTimeProofRequest {
    pub l2_block_numbers: Vec<u64>,
    pub network: String,
    pub l1_network: String,
    pub prover: Address,
    pub proof_type: ProofType,
    pub blob_proof_type: BlobProofType,
    pub prover_args: HashMap<String, Value>,
    pub gpu_number: Option<u32>,

    // --- RealTime-specific fields ---
    pub max_anchor_block_number: u64,       // highest L1 block the L2 derivation references
    pub signal_slots: Vec<B256>,            // L1 signal slots to relay
    pub parent_proposal_hash: B256,         // from getLastProposalHash()
    pub basefee_sharing_pctg: u8,           // % of basefee paid to coinbase
    pub checkpoint: Option<ShastaProposalCheckpoint>, // previous finalized checkpoint
}
```

**New API endpoint** in `host/src/server/api/v3/proof/`:

- `POST /v3/proof/batch/realtime` — routed to a new `realtime_handler.rs` (same pattern as `shasta_handler.rs`)

**New request opt type** `RealTimeProofRequestOpt` for JSON-level optional fields, with `TryFrom` into `RealTimeProofRequest`.

---

## Step 2 — Parsing L1 Proposal Events

### Existing Shasta Flow

Before proving, raiko fetches and parses the L1 `Proposed` event to know what data was proposed.

**Entry point:**

- [`core/src/preflight/mod.rs:31`](core/src/preflight/mod.rs#L31) — `parse_l1_batch_proposal_tx_for_shasta_fork()`

**Key types:**

```rust
// lib/src/input/shasta.rs:52-72
sol! {
    struct Proposal {
        uint48 id;
        uint48 timestamp;
        uint48 endOfSubmissionWindowTimestamp;
        address proposer;
        bytes32 parentProposalHash;
        uint48 originBlockNumber;
        bytes32 originBlockHash;
        uint8 basefeeSharingPctg;
        DerivationSource[] sources;
    }
}
```

```rust
// lib/src/input/shasta.rs:124-131
sol! {
    event Proposed(
        uint48 indexed id,
        address indexed proposer,
        bytes32 parentProposalHash,
        uint48 endOfSubmissionWindowTimestamp,
        uint8 basefeeSharingPctg,
        DerivationSource[] sources
    );
}
```

```rust
// lib/src/input/shasta.rs:136-138
pub struct ShastaEventData {
    pub proposal: Proposal,
}
```

**Flow:**

1. Query L1 RPC for the `Proposed` event at `l1_inclusion_block_number`
2. Decode event into `ShastaEventData`
3. Extract block numbers from the proposal's derivation sources (manifest data in blobs)
4. Return `(block_numbers, cached_event_data)` for preflight

**Data wrapped into:**

```rust
// lib/src/input.rs:173-180
pub enum BlockProposedFork {
    Nothing,
    Hekla(BlockProposed),
    Ontake(BlockProposedV2),
    Pacaya(BatchProposed),
    Shasta(ShastaEventData),
}
```

### RealTime Fork — New Code

There is **no on-chain `Proposed` event** to fetch. The prover constructs the proposal locally from caller-supplied data. The on-chain event `ProposedAndProved` is emitted only **after** the proof is submitted.

**New file `lib/src/input/realtime.rs`** — sol! types:

```rust
sol! {
    // Maps to IRealTimeInbox.Proposal (transient, never stored on-chain)
    struct RealTimeProposal {
        bytes32                parentProposalHash;
        uint48                 maxAnchorBlockNumber;
        bytes32                maxAnchorBlockHash;
        uint8                  basefeeSharingPctg;
        DerivationSource[]     sources;       // reuses IInbox.DerivationSource
        bytes32                signalSlotsHash;
    }

    // Maps to IRealTimeInbox.ProposeInput
    struct RealTimeProposeInput {
        LibBlobs.BlobReference blobReference;
        bytes32[]              signalSlots;
        uint48                 maxAnchorBlockNumber;
    }

    // Maps to IRealTimeInbox.Commitment (one proposal, no batching)
    struct RealTimeCommitment {
        bytes32   proposalHash;
        Checkpoint checkpoint;    // reuses ICheckpointStore.Checkpoint
    }

    // Emitted after atomic propose+prove
    event ProposedAndProved(
        bytes32 indexed proposalHash,
        bytes32 parentProposalHash,
        uint48  maxAnchorBlockNumber,
        uint8   basefeeSharingPctg,
        DerivationSource[] sources,
        bytes32 signalSlotsHash,
        Checkpoint checkpoint
    );
}

pub struct RealTimeEventData {
    pub proposal: RealTimeProposal,
    pub signal_slots: Vec<B256>,       // raw slots, needed for hash verification
}
```

**New `BlockProposedFork` variant** in `lib/src/input.rs`:

```rust
pub enum BlockProposedFork {
    // ... existing variants untouched ...
    RealTime(RealTimeEventData),  // NEW
}
```

All match arms on `BlockProposedFork` get a new `RealTime(...)` case. Methods like `blob_used()`, `fork_spec()`, `proposal_hash()`, etc. get new branches — the existing Shasta branches are untouched.

**New data construction** instead of event parsing:

- Caller provides `max_anchor_block_number`, `signal_slots`, `parent_proposal_hash`, `basefee_sharing_pctg` in the request.
- Prover reads `blockhash(maxAnchorBlockNumber)` from L1 RPC → `max_anchor_block_hash`.
- Prover computes `signalSlotsHash` (empty → `bytes32(0)`, non-empty → `keccak256(abi.encode(signalSlots))`).
- Prover assembles `RealTimeProposal` locally — no L1 event query.

---

## Step 3 — Constructing GuestInput (Preflight)

### Existing Shasta Flow

Preflight fetches all data needed to re-execute the L2 block(s) inside the guest prover.

**Entry points:**

- Single block: [`core/src/preflight/mod.rs:86`](core/src/preflight/mod.rs#L86) — `preflight()`
- Batch (Shasta): [`core/src/preflight/mod.rs:248`](core/src/preflight/mod.rs#L248) — `batch_preflight()`

**Key types:**

```rust
// lib/src/input.rs:46-66
pub struct GuestInput {
    pub block: TaikoBlock,
    pub chain_spec: ChainSpec,
    pub parent_header: Header,
    pub parent_state_trie: MptNode,
    pub parent_storage: HashMap<Address, StorageEntry>,
    pub contracts: Vec<Bytes>,
    pub ancestor_headers: Vec<Header>,
    pub taiko: TaikoGuestInput,
}

// lib/src/input.rs:412-429
pub struct TaikoGuestInput {
    pub l1_header: Header,
    pub tx_data: Vec<u8>,
    pub anchor_tx: Option<TaikoTxEnvelope>,
    pub block_proposed: BlockProposedFork,
    pub prover_data: TaikoProverData,
    pub blob_commitment: Option<Vec<u8>>,
    pub blob_proof: Option<Vec<u8>>,
    pub blob_proof_type: BlobProofType,
    pub extra_data: Option<bool>,          // forced inclusion flag
    pub grandparent_timestamp: Option<u64>,
}

// lib/src/input.rs:100-103
pub struct GuestBatchInput {
    pub inputs: Vec<GuestInput>,
    pub taiko: TaikoGuestBatchInput,
}

// lib/src/input.rs:81-95
pub struct TaikoGuestBatchInput {
    pub batch_id: u64,
    pub l1_header: Header,
    pub l1_ancestor_headers: Vec<Header>,
    pub batch_proposed: BlockProposedFork,
    pub chain_spec: ChainSpec,
    pub prover_data: TaikoProverData,
    pub data_sources: Vec<InputDataSource>,
    pub l2_grandparent_header: Option<Header>,
}
```

**Preflight steps (for batch/Shasta):**

1. Fetch L2 blocks and their parent blocks via RPC
2. Call `prepare_taiko_chain_batch_input()` → `prepare_taiko_chain_batch_input_shasta()` — fetches L1 data, parses blob/calldata, builds `TaikoGuestBatchInput`
3. Generate transactions for each block via `generate_transactions_for_batch_blocks()`
4. For each block: create `ProviderDb`, re-execute transactions, collect Merkle proofs, build state/storage tries, fetch ancestor headers, extract contract code
5. Assemble final `GuestBatchInput`

### RealTime Fork — New Code

The shared types `GuestInput`, `GuestBatchInput`, `TaikoGuestInput`, `TaikoGuestBatchInput` are **reused unchanged**. The fork-specific data enters through the `BlockProposedFork::RealTime(...)` variant inside `batch_proposed`.

**New preflight function** in `core/src/preflight/util.rs`:

```rust
pub async fn prepare_taiko_chain_batch_input_realtime(
    l1_chain_spec: &ChainSpec,
    taiko_chain_spec: &ChainSpec,
    max_anchor_block_number: u64,
    signal_slots: Vec<B256>,
    parent_proposal_hash: B256,
    basefee_sharing_pctg: u8,
    all_prove_blocks: &[TaikoBlock],
    prover_data: TaikoProverData,
    blob_proof_type: &BlobProofType,
    l2_grandparent_header: Option<Header>,
) -> RaikoResult<TaikoGuestBatchInput>
```

This function:

1. Reads L1 header at `max_anchor_block_number` (the header's hash is `maxAnchorBlockHash`)
2. Reads L1 ancestor headers for anchor linkage
3. Computes `signalSlotsHash`
4. Assembles `RealTimeProposal` with `DerivationSource[]` from blob data
5. Wraps into `BlockProposedFork::RealTime(RealTimeEventData { proposal, signal_slots })`
6. Returns `TaikoGuestBatchInput` with `batch_proposed` set to the new variant

**Wired into the dispatcher** at `prepare_taiko_chain_batch_input()` via the existing `TaikoSpecId` match:

```rust
TaikoSpecId::REALTIME => prepare_taiko_chain_batch_input_realtime(...).await,
```

**Key differences from Shasta preflight:**

- No RPC call to fetch `Proposed` event
- `l1_header` is the header at `maxAnchorBlockNumber` (not `originBlockNumber`)
- `extra_data` (forced inclusion flag) always `None` — forced inclusions are removed
- Blob handling reuses `DerivationSource[]` + `blob_tx_slice_param_for_source()` unchanged

---

## Step 4 — Anchor Transaction Decoding & Validation

### Existing Shasta Flow

Each L2 block starts with an anchor transaction that ties it to L1.

**Entry point:**

- [`lib/src/anchor.rs:203-206`](lib/src/anchor.rs#L203) — `decode_anchor_shasta()`

**Anchor function (Shasta):**

```solidity
// lib/src/anchor.rs:164-170
function anchorV4(Checkpoint calldata _checkpoint) external onlyValidSender nonReentrant {}
```

**Checkpoint type:**

```rust
// lib/src/anchor.rs:150-157 (also lib/src/input/shasta.rs:18-23)
sol! {
    struct Checkpoint {
        uint48 blockNumber;
        bytes32 blockHash;
        bytes32 stateRoot;
    }
}
```

**Validation in [`lib/src/protocol_instance.rs:611-657`](lib/src/protocol_instance.rs#L611):**

- Verifies L1 anchor linkage via `verify_shasta_anchor_linkage()`: checks that anchor transactions reference valid L1 blocks within the `originBlockNumber` range
- Validates `originBlockNumber` and `originBlockHash` match the L1 header
- Checks the checkpoint matches expected values

### RealTime Fork — New Code

The RealTime fork uses a **new anchor function** for the first block in each batch:

```solidity
// L2 Anchor.sol — first block of a RealTime batch
function anchorV4WithSignalSlots(
    ICheckpointStore.Checkpoint calldata _checkpoint,
    bytes32[]                   calldata _signalSlots
) external;
```

Subsequent blocks in the same batch call `anchorV4WithSignalSlots` with an empty `_signalSlots` array:

```
Batch (from one propose() call)
├── Block 0 — anchorV4WithSignalSlots(checkpoint, signalSlots)   ← all slots here
├── Block 1 — anchorV4WithSignalSlots(checkpoint, [])
└── Block N — anchorV4WithSignalSlots(checkpoint, [])
```

**New decoder** in `lib/src/anchor.rs`:

```rust
// Parallel to decode_anchor_shasta(); handles anchorV4WithSignalSlots
pub fn decode_anchor_realtime(anchor_tx: &TaikoTxEnvelope)
    -> Result<(Checkpoint, Vec<B256>)>
```

Returns the decoded `Checkpoint` and the `signalSlots` array (empty for all blocks except the first).

**New validation function** in `lib/src/protocol_instance.rs`:

```rust
fn verify_realtime_anchor_linkage(
    inputs: &[GuestInput],
    l1_ancestor_headers: &[Header],
    max_anchor_block_number: u64,
    max_anchor_block_hash: &B256,  // instead of originBlockHash
    signal_slots: &[B256],         // from RealTimeEventData
) -> bool
```

This function performs two checks (parallel to `verify_shasta_anchor_linkage()`, no existing code modified):

1. **Anchor block linkage** — each block's anchor transaction references an L1 block at or **before** `maxAnchorBlockNumber`, using `maxAnchorBlockHash` as the upper bound (rather than `originBlockHash`).

2. **Signal slots integrity** — the `signalSlots` extracted from the first block's `anchorV4WithSignalSlots` call must hash to the `signalSlotsHash` committed in the proposal:
   ```
   expected_hash = hash_signal_slots(signal_slots)   // from Step 7 / libhash.rs
   actual_hash   = keccak(abi_encode(anchor_signal_slots))
   assert expected_hash == actual_hash
   ```
   All subsequent blocks must have an empty signal slots array in their anchor transaction.

No existing Shasta validation code is modified.

---

## Step 5 — Transaction Generation & Execution

### Existing Shasta Flow

Transactions are generated from blob/calldata and executed against the state DB.

**Entry points:**

- [`lib/src/utils/txs.rs`](lib/src/utils/txs.rs) — `generate_transactions()`, `generate_transactions_for_batch_blocks()`
- [`core/src/preflight/mod.rs:160-173`](core/src/preflight/mod.rs#L160) — builder + execution

**Flow:**

1. Decompress `tx_data` from blob or calldata
2. Prepend anchor transaction
3. Execute via `RethBlockBuilder::execute_transactions()`
4. Verify resulting header matches expected

### RealTime Fork — New Code

**No new code needed.** Transaction execution is fork-agnostic. The same blob decompression, transaction ordering, and execution logic applies.

The only behavioral difference: `DerivationSource.isForcedInclusion` is always `false` in RealTime (forced inclusions are removed from the protocol). This is handled naturally — the `RealTimeProposal.sources` constructed in Step 3 will always have `isForcedInclusion = false`.

---

## Step 6 — Computing GuestOutput & ProtocolInstance

### Existing Shasta Flow

After re-executing the block, raiko computes the `GuestOutput` which contains the public instance hash.

**Entry points:**

- [`core/src/lib.rs:162-195`](core/src/lib.rs#L162) — `get_batch_output()`
- [`lib/src/protocol_instance.rs:588-682`](lib/src/protocol_instance.rs#L588) — `ProtocolInstance::new_batch()`

**Key types:**

```rust
// lib/src/input.rs:492-496
pub struct GuestBatchOutput {
    pub blocks: Vec<TaikoBlock>,
    pub hash: B256,  // ProtocolInstance.instance_hash()
}

// lib/src/protocol_instance.rs
pub struct ProtocolInstance {
    pub transition: TransitionFork,
    pub block_metadata: BlockMetaDataFork,
    pub sgx_instance: Address,
    pub prover: Address,
    pub chain_id: ChainId,
    pub verifier_address: Address,
}
```

**Shasta `TransitionFork` and instance hash:**

```rust
// TransitionFork variant
TransitionFork::Shasta(TransitionInputData {
    proposal_id, proposal_hash, parent_proposal_hash,
    parent_block_hash, actual_prover,
    transition: ShastaTransitionInput { proposer, timestamp },
    checkpoint: Checkpoint { blockNumber, blockHash, stateRoot },
})

// Instance hash (protocol_instance.rs:750-761)
TransitionFork::Shasta(shasta_trans_input) => {
    hash_shasta_subproof_input(&ProofCarryData {
        chain_id, verifier, transition_input: shasta_trans_input,
    })
}
```

`hash_shasta_subproof_input` ([`lib/src/libhash.rs:103-112`](lib/src/libhash.rs#L103)) computes:
```
hash_four_values(VERIFY_PROOF_B256, chain_id, verifier, hash_shasta_transition_input(...))
```

`hash_shasta_transition_input` ([`lib/src/libhash.rs:114-140`](lib/src/libhash.rs#L114)) hashes 11 values: `proposal_id`, `proposal_hash`, `parent_proposal_hash`, `parent_block_hash`, `actual_prover`, `proposer`, `timestamp`, `hash_checkpoint(...)`, `blockNumber`, `blockHash`, `stateRoot`.

### RealTime Fork — New Code

**New `TransitionFork` variant:**

```rust
// In lib/src/protocol_instance.rs
pub enum TransitionFork {
    Hekla(Transition),
    OnTake(Transition),
    Pacaya(PacayaTransition),
    Shasta(TransitionInputData),
    RealTime(RealTimeTransitionData),  // NEW
}
```

**New transition data type** in `lib/src/protocol_instance.rs` (or `lib/src/input/realtime.rs`):

```rust
pub struct RealTimeTransitionData {
    pub proposal_hash: B256,      // keccak256(abi.encode(RealTimeProposal))
    pub checkpoint: Checkpoint,   // { blockNumber, blockHash, stateRoot }
}
```

**New `BlockMetaDataFork` variant:**

```rust
pub enum BlockMetaDataFork {
    // ... existing variants untouched ...
    RealTime(RealTimeProposal),  // NEW
}
```

**New `ProtocolInstance::new_batch()` branch** for `BlockProposedFork::RealTime`:

The existing `new_batch()` dispatches on `batch_input.taiko.batch_proposed`. Add a new arm:

```rust
BlockProposedFork::RealTime(event_data) => {
    verify_realtime_anchor_linkage(...);  // new function from Step 4
    let checkpoint = Checkpoint { blockNumber, blockHash, stateRoot };  // from last block
    TransitionFork::RealTime(RealTimeTransitionData {
        proposal_hash: hash_realtime_proposal(&event_data.proposal),
        checkpoint,
    })
}
```

**New `instance_hash()` branch:**

```rust
TransitionFork::RealTime(rt) => {
    hash_realtime_commitment(rt.proposal_hash, &rt.checkpoint)
}
```

Where `hash_realtime_commitment` computes (from [PROTOCOL_MIGRATION §6](PROTOCOL_MIGRATION_REAL_TIME_FORK.md)):

```
commitmentHash = keccak256(abi.encode(
    bytes32 proposalHash,
    uint48  checkpoint.blockNumber,
    bytes32 checkpoint.blockHash,
    bytes32 checkpoint.stateRoot
))
```

This is the value passed to `verifyProof(0, commitmentHash, proof)`.

No existing Shasta `instance_hash()` or `new_batch()` code is modified.

---

## Step 7 — Commitment & Hash Construction

### Existing Shasta Flow

**Commitment type ([`lib/src/input/shasta.rs:85-103`](lib/src/input/shasta.rs#L85)):**

```rust
sol! {
    struct Commitment {
        uint48 firstProposalId;
        bytes32 firstProposalParentBlockHash;
        bytes32 lastProposalHash;
        address actualProver;
        uint48 endBlockNumber;
        bytes32 endStateRoot;
        Transition[] transitions;
    }
}
```

**Commitment hash** ([`lib/src/libhash.rs:145-184`](lib/src/libhash.rs#L145)) — flattens the struct into a word buffer and keccak-hashes it, matching Solidity's `LibHashOptimized`.

**Proposal hash** ([`lib/src/libhash.rs:202-204`](lib/src/libhash.rs#L202)):

```rust
pub fn hash_proposal(proposal: &Proposal) -> B256 {
    keccak(proposal.abi_encode().as_slice()).into()
}
```

**Public input hash** ([`lib/src/libhash.rs:362-375`](lib/src/libhash.rs#L362)):

```rust
pub fn hash_public_input(prove_input_hash, chain_id, verifier_address, sgx_instance) -> B256 {
    hash_five_values(VERIFY_PROOF_B256, chain_id, verifier_address, prove_input_hash, sgx_instance)
}
```

### RealTime Fork — New Code

All new hashing functions are **additive** — existing functions are untouched.

**New functions** in `lib/src/libhash.rs`:

```rust
/// Hash the RealTimeInbox proposal — plain abi.encode, no LibHashOptimized.
pub fn hash_realtime_proposal(proposal: &RealTimeProposal) -> B256 {
    keccak(proposal.abi_encode().as_slice()).into()
}

/// Hash the RealTimeInbox commitment (one proposal, no batching).
/// commitmentHash = keccak256(abi.encode(proposalHash, blockNumber, blockHash, stateRoot))
pub fn hash_realtime_commitment(proposal_hash: B256, checkpoint: &Checkpoint) -> B256 {
    keccak(
        (proposal_hash, checkpoint.blockNumber, checkpoint.blockHash, checkpoint.stateRoot)
            .abi_encode()
            .as_slice()
    ).into()
}

/// Hash signal slots for RealTimeInbox.
/// Empty → bytes32(0), non-empty → keccak256(abi.encode(signalSlots))
pub fn hash_signal_slots(signal_slots: &[B256]) -> B256 {
    if signal_slots.is_empty() {
        B256::ZERO
    } else {
        keccak(signal_slots.abi_encode().as_slice()).into()
    }
}
```

**Key differences from Shasta hashing:**

| Aspect | Shasta | RealTime |
|--------|--------|----------|
| Proposal hash | `keccak(Proposal.abi_encode())` with 9 fields (id, timestamp, proposer, ...) | `keccak(RealTimeProposal.abi_encode())` with 6 fields (parentHash, maxAnchor, sources, ...) |
| Commitment hash | Custom `hash_commitment()` with `Transition[]` buffer layout | Simple `keccak(abi.encode(proposalHash, checkpoint))` |
| Public input | `hash_public_input()` with 5-value domain separation | `commitmentHash` used directly — `verifyProof(0, commitmentHash, proof)` |
| Signal slots hash | N/A (not in Shasta Proposal) | `hash_signal_slots()` — first-class field |
| proposalAge | Computed from transition timestamps | Always `0` |

---

## Step 8 — Proof Generation (Prover Dispatch)

### Existing Shasta Flow

**Entry points:**

- Shasta proposal proof: [`core/src/lib.rs:290-307`](core/src/lib.rs#L290) — `shasta_proposal_prove()`
- Dispatch: [`core/src/interfaces.rs:257-315`](core/src/interfaces.rs#L257) — `run_shasta_proposal_prover()`

**Prover trait** ([`lib/src/prover.rs:124-208`](lib/src/prover.rs#L124)):

```rust
pub trait Prover {
    async fn run(...) -> ProverResult<Proof>;           // Single block
    async fn batch_run(...) -> ProverResult<Proof>;     // Batch
    async fn proposal_run(...) -> ProverResult<Proof>;  // Shasta proposal
    async fn aggregate(...) -> ProverResult<Proof>;
    async fn shasta_aggregate(...) -> ProverResult<Proof>;
    async fn cancel(...) -> ProverResult<()>;
}
```

**Default `proposal_run`** ([`lib/src/prover.rs:150-191`](lib/src/prover.rs#L150)):

1. Calls `self.batch_run(input, output, config, store)`
2. Attaches `ProofCarryData` with full `TransitionInputData`

### RealTime Fork — New Code

**New `Prover` trait method:**

```rust
/// Run the prover for RealTime proposals (default: batch_run, no ProofCarryData)
async fn realtime_run(
    &self,
    input: GuestBatchInput,
    output: &GuestBatchOutput,
    config: &ProverConfig,
    store: Option<&mut dyn IdWrite>,
) -> ProverResult<Proof> {
    // Default: just delegates to batch_run — no ProofCarryData needed
    self.batch_run(input, output, config, store).await
}
```

Unlike Shasta's `proposal_run`, there is **no `ProofCarryData` attachment** in the default implementation — the RealTime model has no aggregation, so carry data is unnecessary.

**New dispatch function** in `core/src/interfaces.rs`:

```rust
pub async fn run_realtime_prover(
    proof_type: ProofType,
    input: GuestBatchInput,
    output: &GuestBatchOutput,
    config: &Value,
    store: Option<&mut dyn IdWrite>,
    mock_key: Option<String>,
) -> RaikoResult<Proof> {
    // Same match-on-ProofType pattern as run_shasta_proposal_prover
    // Calls .realtime_run() on each prover backend
}
```

**New `Raiko` method** in `core/src/lib.rs`:

```rust
pub async fn realtime_prove(
    &self,
    input: GuestBatchInput,
    output: &GuestBatchOutput,
    store: Option<&mut dyn IdWrite>,
    mock_key: Option<String>,
) -> RaikoResult<Proof> {
    let config = serde_json::to_value(&self.request)?;
    run_realtime_prover(self.request.proof_type, input, output, &config, store, mock_key).await
}
```

**Guest program changes** (provers/sp1/guest, provers/risc0/guest, etc.):

- The guest receives `GuestBatchInput` as before
- But the `instance_hash()` now dispatches through `TransitionFork::RealTime` → `hash_realtime_commitment()`
- The public output is `commitmentHash` — the same value the verifier contract checks

No existing Shasta prover dispatch code is modified.

---

## Step 9 — ProofCarryData & Aggregation

### Existing Shasta Flow

**ProofCarryData types** ([`lib/src/prover.rs:38-63`](lib/src/prover.rs#L38)):

```rust
pub struct ShastaTransitionInput { pub proposer: Address, pub timestamp: u64 }

pub struct TransitionInputData {
    pub proposal_id: u64, pub proposal_hash: B256,
    pub parent_proposal_hash: B256, pub parent_block_hash: B256,
    pub actual_prover: Address, pub transition: ShastaTransitionInput,
    pub checkpoint: Checkpoint,
}

pub struct ProofCarryData {
    pub chain_id: ChainId, pub verifier: Address,
    pub transition_input: TransitionInputData,
}
```

**Aggregation flow:**

1. Multiple sub-proofs generated (one per proposal), each carries `ProofCarryData`
2. Validated via `validate_shasta_proof_carry_data_vec()` ([`protocol_instance.rs:829-870`](lib/src/protocol_instance.rs#L829))
3. Commitment built via `build_shasta_commitment_from_proof_carry_data_vec()` ([`protocol_instance.rs:872-899`](lib/src/protocol_instance.rs#L872))
4. Aggregation hash via `shasta_pcd_aggregation_hash()` ([`protocol_instance.rs:911-924`](lib/src/protocol_instance.rs#L911))

### RealTime Fork — New Code

**No aggregation pipeline.** RealTimeInbox proves exactly one proposal per proof. There is no `Transition[]` array, no batch commitment spanning `[N..M]`, no `ProofCarryData` chaining.

Therefore:

- No new `ProofCarryData` variant needed
- No new aggregation types, validation functions, or commitment builders
- No `RealTimeAggregationGuestInput` or similar
- The `Proof.extra_data` field can remain `None` for RealTime proofs

All existing Shasta aggregation code remains untouched — it simply isn't invoked for the RealTime fork path.

---

## Step 10 — Proof Output & API Response

### Existing Shasta Flow

**Proof type** ([`lib/src/prover.rs:65-80`](lib/src/prover.rs#L65)):

```rust
pub struct Proof {
    pub proof: Option<String>,
    pub input: Option<B256>,              // Public input hash
    pub quote: Option<String>,
    pub uuid: Option<String>,
    pub kzg_proof: Option<String>,
    pub extra_data: Option<ProofCarryData>,  // Shasta transition data
}
```

**Caller workflow (Shasta):**

1. Prove each proposal → collect proofs with `ProofCarryData`
2. Aggregate → get final proof
3. Submit `prove(data, proof)` to `Inbox` contract

### RealTime Fork — New Code

The existing `Proof` struct is **reused unchanged**. For RealTime proofs:

- `proof` — the ZK/TEE proof bytes
- `input` — the `commitmentHash` (what the on-chain verifier checks)
- `extra_data` — `None` (no carry data needed)

**New API response wrapper** (optional, for clarity):

```rust
pub struct RealTimeProofResponse {
    pub proof: Proof,
    pub proposal_hash: B256,        // so caller can verify
    pub commitment_hash: B256,      // what the proof attests to
    pub checkpoint: Checkpoint,     // to include in propose() call
}
```

**Caller workflow (RealTime):**

1. Send request to raiko → receive proof + proposal_hash + commitment_hash + checkpoint
2. Submit `propose(data, checkpoint, proof)` to `RealTimeInbox` contract **atomically**
3. No aggregation step

