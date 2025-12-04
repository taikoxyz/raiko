# Raiko V2 Implementation Plan

## Overview

This document outlines the step-by-step implementation plan for migrating Raiko to V2 architecture. The plan follows a **copy-first** strategy to minimize disruption and enable incremental migration.

## Prerequisites

Before starting:

- [x] Ensure `alethia-reth` is accessible at https://github.com/taikoxyz/alethia-reth
- [x] Review taiko-client-rs protocol module structure
- [ ] Set up development environment with both risc0 and sp1 toolchains

## Phase 1: Foundation Setup ✅ COMPLETED

### 1.1 Rename Existing Crates ✅

**Goal**: Rename `raizen-*` packages to `raiko2-*` in Cargo.toml (keep simple directory names).

**Naming convention**:

- Directory: `crates/primitives/` (simple)
- Package name in Cargo.toml: `raiko2-primitives`

**Tasks**:

- [x] Update `crates/primitives/Cargo.toml`: `name = "raiko2-primitives"`
- [x] Update `crates/driver/Cargo.toml`: `name = "raiko2-driver"`
- [x] Update `crates/provider/Cargo.toml`: `name = "raiko2-provider"`
- [x] Update `crates/engine/Cargo.toml`: `name = "raiko2-engine"`
- [x] Update `crates/stateless/Cargo.toml`: `name = "raiko2-stateless"`
- [x] Update `crates/prover/Cargo.toml`: `name = "raiko2-prover"`

**Files modified**:

```
crates/*/Cargo.toml          # Updated package names to raiko2-*
Cargo.toml                   # Updated workspace dependencies
```

### 1.2 Update Workspace Dependencies ✅

**Goal**: Update root `Cargo.toml` with new raiko2-\* references.

```toml
# Workspace dependencies
# Package name (raiko2-*) -> Directory path (crates/*)
raiko2-primitives = { path = "./crates/primitives" }
raiko2-driver = { path = "./crates/driver" }
raiko2-provider = { path = "./crates/provider" }
raiko2-engine = { path = "./crates/engine" }
raiko2-stateless = { path = "./crates/stateless" }
raiko2-prover = { path = "./crates/prover" }
raiko2-protocol = { path = "./crates/protocol" }
```

### 1.3 Verify Build

```bash
cargo check -p raiko2-primitives
cargo check -p raiko2-engine
```

---

## Phase 2: zkVM Guest Setup ✅ COMPLETED

### 2.1 Copy Guest Programs ✅

**Goal**: Copy zkVM guest programs to crates directory.

**Tasks**:

- [x] Copy `provers/risc0/guest/` to `crates/guest-risc0/`
- [x] Copy `provers/sp1/guest/` to `crates/guest-sp1/`
- [x] Update Cargo.toml paths in both guest crates
- [x] Remove deprecated binary targets (keep only `batch`, `shasta_aggregation`)

**Directory structure**:

```
crates/
├── guest-risc0/              # Package: raiko2-guest-risc0
│   ├── Cargo.toml
│   ├── src/
│   │   ├── batch.rs
│   │   ├── shasta_aggregation.rs
│   │   ├── zk_op.rs
│   │   └── mem.rs
│   └── elf/                  # Built ELF files (gitignored)
└── guest-sp1/                # Package: raiko2-guest-sp1
    ├── Cargo.toml
    ├── src/
    │   ├── batch.rs
    │   ├── shasta_aggregation.rs
    │   ├── sys.rs
    │   └── zk_op.rs
    └── elf/                  # Built ELF files (gitignored)
```

### 2.2 Create Guest Build Script ✅

**Goal**: Replace Rust pipeline with simple shell script.

**File**: `script/build-guest.sh` ✅ Created

```bash
#!/usr/bin/env bash
set -e

TARGET=${1:-all}  # risc0, sp1, or all

build_risc0() {
    echo "Building RISC0 guests..."
    cd crates/guest-risc0

    # Build ELF
    cargo +nightly-2024-12-20 build --release \
        --target riscv32im-risc0-zkvm-elf \
        --bin risc0-batch \
        --bin risc0-shasta-aggregation

    # Copy ELF files
    mkdir -p elf
    cp target/riscv32im-risc0-zkvm-elf/release/risc0-batch elf/
    cp target/riscv32im-risc0-zkvm-elf/release/risc0-shasta-aggregation elf/

    # Generate image IDs (using helper binary)
    cargo run --bin risc0-imageid-helper
}

build_sp1() {
    echo "Building SP1 guests..."
    cd crates/guest-sp1

    # Build ELF
    cargo +succinct build --release \
        --target riscv32im-succinct-zkvm-elf \
        --bin sp1-batch \
        --bin sp1-shasta-aggregation

    # Copy ELF files
    mkdir -p elf
    cp target/riscv32im-succinct-zkvm-elf/release/sp1-batch elf/
    cp target/riscv32im-succinct-zkvm-elf/release/sp1-shasta-aggregation elf/

    # Generate VK hashes (using sp1-sdk)
    cargo run --bin sp1-vkhash-helper
}

case $TARGET in
    risc0) build_risc0 ;;
    sp1) build_sp1 ;;
    all) build_risc0 && build_sp1 ;;
esac
```

### 2.3 Create Helper Binaries for Image ID Generation

**RISC0**: `crates/guest-risc0/src/bin/imageid_helper.rs`

```rust
use risc0_binfmt::ProgramBinary;
use risc0_zkos_v1compat::V1COMPAT_ELF;

fn main() {
    for name in ["risc0-batch", "risc0-shasta-aggregation"] {
        let elf_path = format!("elf/{}", name);
        let user_elf = std::fs::read(&elf_path).unwrap();
        let elf = ProgramBinary::new(&user_elf, V1COMPAT_ELF).encode();
        let image_id = risc0_binfmt::compute_image_id(&elf).unwrap();

        // Write combined ELF
        std::fs::write(format!("{}.bin", elf_path), &elf).unwrap();

        // Print image ID
        println!("{}: {}", name, hex::encode(image_id.as_bytes()));
    }
}
```

**SP1**: `crates/guest-sp1/src/bin/vkhash_helper.rs`

```rust
use sp1_sdk::{CpuProver, HashableKey, Prover};

fn main() {
    for name in ["sp1-batch", "sp1-shasta-aggregation"] {
        let elf_path = format!("elf/{}", name);
        let elf = std::fs::read(&elf_path).unwrap();

        let prover = CpuProver::new();
        let (_, vk) = prover.setup(&elf);

        println!("{}: {}", name, hex::encode(vk.hash_bytes()));
    }
}
```

---

## Phase 3: Prover Integration ✅ COMPLETED

### 3.1 Integrate Prover SDK into raiko2-prover ✅

**Goal**: Move driver logic from `provers/*/driver/` into unified `raiko2-prover`.

**Tasks**:

- [x] Add risc0-zkvm and sp1-sdk as optional dependencies
- [x] Create `risc0/mod.rs` module with `Risc0Prover` implementation
- [x] Create `sp1/mod.rs` module with `Sp1Prover` implementation
- [x] Include ELF files via `include_bytes!`
- [x] Implement `Prover` trait for both

**File structure**:

```
crates/prover/                # Package: raiko2-prover
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── risc0/
    │   ├── mod.rs            # Risc0Prover implementation
    │   └── methods.rs        # ELF constants
    ├── sp1/
    │   ├── mod.rs            # Sp1Prover implementation
    │   └── elf.rs            # ELF constants
    └── config.rs             # ProverConfig types
```

### 3.2 Update Cargo.toml Features ✅

```toml
[package]
name = "raiko2-prover"

[dependencies]
raiko2-primitives.workspace = true
raiko-lib.workspace = true

# RISC0
risc0-zkvm = { workspace = true, optional = true }
bonsai-sdk = { workspace = true, optional = true }

# SP1
sp1-sdk = { workspace = true, optional = true }
sp1-prover = { workspace = true, optional = true }

[features]
default = []
risc0 = ["dep:risc0-zkvm", "dep:bonsai-sdk"]
sp1 = ["dep:sp1-sdk", "dep:sp1-prover"]
```

### 3.3 Implement Prover Trait

```rust
// src/lib.rs
pub trait Prover: Send + Sync {
    async fn prove_batch(
        &self,
        input: GuestBatchInput,
        config: &ProverConfig,
    ) -> ProverResult<Proof>;

    async fn aggregate(
        &self,
        input: AggregationInput,
        config: &ProverConfig,
    ) -> ProverResult<Proof>;
}

#[cfg(feature = "risc0")]
pub mod risc0;

#[cfg(feature = "sp1")]
pub mod sp1;
```

---

## Phase 4: Protocol Integration ✅ COMPLETED

### 4.1 Create protocol Crate (`raiko2-protocol`) ✅

**Goal**: Vendor Shasta protocol types from taiko-client-rs.

**Tasks**:

- [x] Create `crates/protocol/` with `name = "raiko2-protocol"` in Cargo.toml
- [x] Create Shasta types based on taiko-client-rs patterns
- [x] Create codec module for event decoding
- [x] Create manifest module for derivation sources
- [x] Create error types for protocol errors

**File structure**:

```
crates/protocol/              # Package: raiko2-protocol
├── Cargo.toml
└── src/
    ├── lib.rs
    └── shasta/
        ├── mod.rs
        ├── constants.rs      # Chain IDs, contract addresses
        ├── error.rs          # Protocol errors
        ├── types.rs          # Event types (BatchProposed, etc.)
        ├── manifest.rs       # Derivation manifests
        └── codec.rs          # Event decoding
```

### 4.2 Add Bindings Generation

**Script**: `script/gen-bindings.sh` (TODO: Create when needed)

```bash
#!/usr/bin/env bash
# Generate Rust contract bindings from Solidity ABIs

forge bind \
    --crate-name raiko2-bindings \
    --bindings-path crates/raiko2-protocol/src/bindings \
    contracts/
```

---

## Phase 5: Binary and API (Week 4)

### 5.1 Create raiko2 Binary

**Goal**: New prover server binary using raiko2-engine.

**File**: `bin/raiko2/src/main.rs`

```rust
use raiko2_engine::Engine;
use raiko2_prover::{Prover, Risc0Prover, Sp1Prover};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = load_config()?;

    let prover: Box<dyn Prover> = match config.prover_type {
        ProverType::Risc0 => Box::new(Risc0Prover::new(config.risc0)?),
        ProverType::Sp1 => Box::new(Sp1Prover::new(config.sp1)?),
    };

    let engine = Engine::new(config.provider, prover);

    run_server(engine, config.server).await
}
```

### 5.2 Implement API Endpoints

```rust
// POST /v2/proof/batch
async fn request_batch_proof(
    State(engine): State<Arc<Engine>>,
    Json(req): Json<BatchProofRequest>,
) -> Result<Json<ProofResponse>, ApiError> {
    let proof_id = engine.submit_batch_proof(req).await?;
    Ok(Json(ProofResponse { id: proof_id }))
}

// GET /v2/proof/:id
async fn get_proof_status(
    State(engine): State<Arc<Engine>>,
    Path(id): Path<String>,
) -> Result<Json<ProofStatus>, ApiError> {
    let status = engine.get_proof_status(&id).await?;
    Ok(Json(status))
}
```

---

## Phase 6: Build System Cleanup (Week 4)

### 6.1 Update Makefile

```makefile
# New raiko2 targets
raiko2: raiko2-guest
	cargo build --release -p raiko2 --features "risc0,sp1"

raiko2-guest:
	./script/build-guest.sh all

raiko2-risc0:
	./script/build-guest.sh risc0
	cargo build --release -p raiko2 --features risc0

raiko2-sp1:
	./script/build-guest.sh sp1
	cargo build --release -p raiko2 --features sp1

# Remove SGX targets (deprecated)
```

### 6.2 Update Dockerfile.zk

```dockerfile
# Simplified for raiko2
FROM rust:1.85.0 AS builder

# Install zkVM toolchains
RUN make install-risc0 install-sp1

# Build guests
COPY . /opt/raiko
WORKDIR /opt/raiko
RUN ./script/build-guest.sh all

# Build binary
RUN cargo build --release -p raiko2 --features "risc0,sp1"

FROM ubuntu:22.04 AS raiko2
COPY --from=builder /opt/raiko/target/release/raiko2 /usr/local/bin/
COPY --from=builder /opt/raiko/.env /etc/raiko/
ENTRYPOINT ["raiko2"]
```

---

## Phase 7: Testing and Validation (Week 5)

### 7.1 Unit Tests

- [ ] Test raiko2-primitives serialization
- [ ] Test raiko2-protocol codec decoding
- [ ] Test raiko2-prover with mock inputs

### 7.2 Integration Tests

- [ ] End-to-end batch proof generation (mock mode)
- [ ] Aggregation proof generation
- [ ] API endpoint tests

### 7.3 Compatibility Validation

- [ ] Compare proof outputs with V1
- [ ] Verify on-chain verification works

---

## Cleanup (Post-Migration)

After V2 is stable:

- [ ] Remove `provers/sgx/` directory
- [ ] Remove `provers/*/builder/` directories
- [ ] Remove `provers/*/driver/` directories
- [ ] Remove `pipeline/` directory
- [ ] Remove `gaiko/` directory
- [ ] Remove legacy feature flags from `host/`
- [ ] Update CI/CD pipelines

---

## Timeline Summary

| Week | Phase              | Deliverables                              | Status |
| ---- | ------------------ | ----------------------------------------- | ------ |
| 1    | Foundation         | Renamed crates, updated dependencies      | ✅     |
| 1-2  | Guest Setup        | Copied guests, build script, helpers      | ✅     |
| 2-3  | Prover Integration | Unified raiko2-prover with SDK calls      | ✅     |
| 3    | Protocol           | raiko2-protocol with Shasta types         | ✅     |
| 4    | Binary             | raiko2 binary, API endpoints              | ⏳     |
| 4    | Build System       | Updated Makefile, Dockerfile              | ⏳     |
| 5    | Testing            | Unit tests, integration tests, validation | ⏳     |

---

## Risk Mitigation

1. **Breaking changes in alethia-reth**: Pin to specific commit initially
2. **zkVM SDK version conflicts**: Use feature flags for SDK versions
3. **Performance regression**: Benchmark V1 vs V2 before full migration
4. **API compatibility**: Keep V1 API available during transition period

---

## Success Criteria

- [x] Crates renamed from raizen-_ to raiko2-_
- [x] Guest programs copied to crates/guest-risc0 and crates/guest-sp1
- [x] build-guest.sh script created
- [x] raiko2-prover with risc0 and sp1 modules
- [x] raiko2-protocol crate with Shasta types
- [ ] `cargo build -p raiko2 --features risc0` succeeds
- [ ] `cargo build -p raiko2 --features sp1` succeeds
- [ ] Guest ELF build script works for both provers
- [ ] Batch proof generation works end-to-end
- [ ] Aggregation proof generation works
- [ ] Docker image builds successfully
- [ ] All tests pass

---

## Implementation Progress

**Completed (Phase 1-4):**

- ✅ Phase 1: Foundation - All 6 crates renamed from `raizen-*` to `raiko2-*`
- ✅ Phase 2: zkVM Guest Setup - Copied guest programs, created build script
- ✅ Phase 3: Prover Integration - Created `risc0/` and `sp1/` modules in raiko2-prover
- ✅ Phase 4: Protocol Integration - Created `raiko2-protocol` crate with Shasta types

**Next Steps (Phase 5-7):**

- ⏳ Phase 5: Create `bin/raiko2` binary with API endpoints
- ⏳ Phase 6: Update Makefile and Dockerfile.zk
- ⏳ Phase 7: Testing and validation
