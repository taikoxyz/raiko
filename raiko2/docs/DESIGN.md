# Raiko V2 Architecture Design

## Overview

Raiko V2 is a simplified, modular zkVM proving system for Taiko's Shasta hardfork. This redesign focuses on:

- **Shasta-only support**: Dropping legacy hardfork compatibility
- **zkVM-only provers**: Supporting only RISC0 and SP1, removing SGX
- **Simplified architecture**: Copy-first migration, minimal crate dependencies
- **Modern dependencies**: Using `alethia-reth` and `taiko-client-rs` integrations

## Design Goals

1. **Simplicity**: Reduce crate count and build complexity
2. **Maintainability**: Clear module boundaries, minimal cross-dependencies
3. **Performance**: Leverage zkVM precompiles for crypto operations
4. **Compatibility**: Align with taiko-client-rs protocol types

## Architecture

### Module Structure

```
crates/
├── primitives/     # Core types: GuestInput, ProofContext, Proof       [raiko2-primitives]
├── provider/       # Blockchain data provider trait + implementations  [raiko2-provider]
├── stateless/      # Stateless block validation using reth-stateless   [raiko2-stateless]
├── engine/         # Main orchestrator: input → validate → prove       [raiko2-engine]
├── prover/         # Prover trait + Risc0/Sp1 SDK integrations          [raiko2-prover]
├── protocol/       # Shasta protocol types (from taiko-client-rs)       [raiko2-protocol]
├── guest-risc0/    # RISC0 zkVM guest programs                          [raiko2-guest-risc0]
└── guest-sp1/      # SP1 zkVM guest programs                            [raiko2-guest-sp1]

bin/
└── raiko2/         # New prover server binary
```

> **Naming convention**: Directory names are simple (`crates/primitives/`), package names use `raiko2-` prefix in `Cargo.toml`.

### Data Flow

```
┌─────────────────┐     ┌──────────────────┐     ┌─────────────────┐
│  L1/L2 Provider │────▶│  raiko2-engine   │────▶│  raiko2-prover  │
│  (RPC/Archive)  │     │  (Orchestrator)  │     │  (Risc0/Sp1)    │
└─────────────────┘     └──────────────────┘     └─────────────────┘
                               │                         │
                               ▼                         ▼
                        ┌──────────────┐          ┌─────────────┐
                        │ raiko2-      │          │ guest-*/    │
                        │ stateless    │          │ (zkVM ELF)  │
                        └──────────────┘          └─────────────┘
```

### Component Responsibilities

#### primitives (`raiko2-primitives`)

Core types shared across all modules:

- `GuestInput`, `GuestBatchInput` - Witness data for zkVM guests
- `ProofContext` - Proving configuration and metadata
- `Proof` - Generated proof with public inputs

#### provider (`raiko2-provider`)

Abstracts blockchain data fetching:

- `Provider` trait for L1/L2 data access
- `NetworkProvider` implementation using alloy
- Block, transaction, and state witness retrieval

#### stateless (`raiko2-stateless`)

Stateless block validation:

- Uses `reth-stateless` for execution witness validation
- Validates state roots and consensus rules
- No dependency on full node state

#### engine (`raiko2-engine`)

Main orchestration layer:

- Combines provider → stateless → prover flow
- Manages proof requests and lifecycle
- Exposes API for the binary

#### prover (`raiko2-prover`)

Unified prover interface:

```rust
pub trait Prover {
    async fn prove(&self, input: GuestInput, config: &ProverConfig) -> ProverResult<Proof>;
    async fn aggregate(&self, input: AggregationInput, config: &ProverConfig) -> ProverResult<Proof>;
}
```

Implementations:

- `Risc0Prover` - Uses risc0-zkvm SDK, supports local/Bonsai
- `Sp1Prover` - Uses sp1-sdk, supports local/Network

#### protocol (`raiko2-protocol`)

Shasta protocol types (vendored from taiko-client-rs):

- Inbox event codec (decode proposed/proved events)
- Derivation manifest decoder
- Anchor transaction parsing
- Contract bindings generation script

#### guest-risc0 / guest-sp1 (`raiko2-guest-*`)

zkVM guest programs (separate workspace with patches):

- `batch.rs` - Batch block proving
- `shasta_aggregation.rs` - Proof aggregation

## Dependencies

### Key External Dependencies

| Crate                     | Version | Purpose                        |
| ------------------------- | ------- | ------------------------------ |
| taiko-reth (alethia-reth) | git     | Taiko-specific reth components |
| reth-stateless            | v1.6.0  | Stateless block validation     |
| risc0-zkvm                | 2.2.0   | RISC0 prover SDK               |
| sp1-sdk                   | 5.0.x   | SP1 prover SDK                 |
| alloy                     | 1.0.23  | Ethereum types and RPC         |

### zkVM Guest Patches

Guest programs require patched crypto libraries for precompiles:

**RISC0 Patches:**

```toml
[patch.crates-io]
k256 = { git = "risc0/RustCrypto-elliptic-curves", tag = "k256/v0.13.4-risczero.1" }
sha2 = { git = "risc0/RustCrypto-hashes", tag = "sha2-v0.10.6-risczero.0" }
substrate-bn = { git = "risc0/paritytech-bn", tag = "v0.6.0-risczero.0" }
revm = { git = "taikoxyz/revm.git", branch = "v36-taiko-shasta" }
```

**SP1 Patches:**

```toml
[patch.crates-io]
k256 = { git = "sp1-patches/elliptic-curves", tag = "patch-k256-13.4-sp1-5.0.0" }
sha2 = { git = "sp1-patches/RustCrypto-hashes", tag = "patch-sha2-0.10.8-sp1-4.0.0" }
substrate-bn = { git = "sp1-patches/bn", tag = "patch-0.6.0-sp1-5.0.0" }
revm = { git = "taikoxyz/revm.git", branch = "v36-taiko" }
```

## Build System

### Simplified Guest Build

Replace complex Rust pipeline with shell script:

```bash
# script/build-guest.sh
# 1. Build zkVM ELF targets
# 2. Compute image ID (risc0) / VK hash (sp1)
# 3. Generate const files for driver inclusion
```

### Makefile Targets

```makefile
raiko2:           # Build raiko2 binary
raiko2-guest:     # Build zkVM guest ELFs
raiko2-risc0:     # Build with risc0 prover
raiko2-sp1:       # Build with sp1 prover
```

## API Design

### HTTP Endpoints

```
POST /v2/proof/batch     # Request batch proof
POST /v2/proof/aggregate # Request aggregation proof
GET  /v2/proof/:id       # Query proof status
DELETE /v2/proof/:id     # Cancel proof request
```

### Proof Request

```json
{
  "batch_id": 123,
  "block_numbers": [1000, 1001, 1002],
  "l1_inclusion_block": 50000,
  "prover_type": "risc0",
  "config": {
    "bonsai": true,
    "snark": true
  }
}
```

## Migration Strategy

1. **Copy-first**: New code in `crates/`, preserve old code initially
2. **Incremental**: Migrate one module at a time
3. **Parallel running**: Both old and new binaries can coexist
4. **Feature flags**: Use cargo features to toggle between implementations

## Comparison: V1 vs V2

| Aspect          | V1 (Current)           | V2 (New)              |
| --------------- | ---------------------- | --------------------- |
| Hardforks       | Ontake, Pacaya, Shasta | Shasta only           |
| Provers         | SGX, RISC0, SP1        | RISC0, SP1 only       |
| Build system    | Rust pipeline crate    | Shell scripts         |
| Driver crates   | Separate per prover    | Unified raiko2-prover |
| Protocol types  | Custom implementation  | taiko-client-rs based |
| reth dependency | Scattered components   | alethia-reth unified  |

## Future Considerations

1. **Preconfirmation support**: Interface with taiko-client-rs preconf module
2. **Multi-chain**: Abstract chain-specific logic for L2 variations
3. **Proof caching**: Local/remote proof storage for aggregation
4. **Metrics**: OpenTelemetry integration for observability
