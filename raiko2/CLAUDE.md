# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Raizen is a proving system for Ethereum-compatible blocks, supporting multiple zkVM backends (SP1, Risc0, SGX) and native execution. The architecture separates the host service from guest provers, with a task management system for handling proof requests.

## Architecture

### Core Components

1. **Host Service** (`host/`) - HTTP server that handles proof requests and manages tasks
2. **Guest Provers** (`provers/*/guest/`) - zkVM/SGX programs that generate proofs
3. **Core Library** (`lib/`, `core/`) - Shared blockchain execution logic
4. **Task Management** (`taskdb/`, `reqpool/`, `reqactor/`) - Redis-based task queue system
5. **Raizen Crates** (`crates/`) - New modular components:
   - `primitives` - Core types and traits
   - `driver` - Main execution driver
   - `provider` - Blockchain data providers
   - `processor` - Block processing logic
   - `stateless` - Stateless validation components

### Dependency Structure

The project uses:
- **Reth v1.6.0** for Ethereum primitives and EVM implementation
- **Taiko-Reth** (Alethia) for Taiko-specific modifications
- **SP1 v5.0.x**, **Risc0 v2.2.0** for zkVM proving
- **Alloy** for Ethereum types (replacing ethers)

## Common Commands

### Building

```bash
# Build specific prover
TARGET=sp1 ./script/build.sh
TARGET=risc0 ./script/build.sh
TARGET=sgx ./script/build.sh

# Build with CPU optimization
CPU_OPT=1 TARGET=sp1 ./script/build.sh

# Build in debug mode (not recommended for zkVM)
DEBUG=1 TARGET=sp1 ./script/build.sh
```

### Running

```bash
# Start host service with specific prover
TARGET=sp1 RUN=1 ./script/build.sh
TARGET=risc0 RUN=1 ./script/build.sh
TARGET=sgx RUN=1 ./script/build.sh

# Native execution (no proof)
cargo run

# With CPU optimization
CPU_OPT=1 cargo run --release --features sp1

# With GPU acceleration (Risc0)
cargo run -F cuda --release --features risc0  # CUDA
cargo run -F metal --release --features risc0  # Apple Metal
```

### Testing

```bash
# Run tests for specific prover
TARGET=sp1 cargo test --features sp1
TARGET=risc0 cargo test --features risc0
TARGET=sgx cargo test --features sgx

# Integration tests
cargo test -F integration run_scenarios_sequentially

# Run single test
cargo test <test_name> --features <prover>
```

### Proving Blocks

```bash
# Prove specific block
./script/prove-block.sh taiko_a7 native 10
./script/prove-block.sh taiko_a7 sp1 10
./script/prove-block.sh taiko_a7 risc0 10

# Sync and prove new blocks continuously
./script/prove-block.sh taiko_a7 native sync
```

### Task Management API

```bash
# Check all tasks status
curl -X POST 'http://localhost:8080/proof/report'

# List all proofs
curl -X POST 'http://localhost:8080/proof/list'

# Prune tasks
curl -X POST 'http://localhost:8080/proof/prune'
```

## Performance Optimization

### SP1 Specific
```bash
# Maximum performance
SHARD_SIZE=4194304 RUST_LOG=info CPU_OPT=1 cargo run --release --features sp1

# Reduced memory usage
SHARD_BATCH_SIZE=1 SHARD_SIZE=2097152 RUST_LOG=info CPU_OPT=1 cargo run --release --features sp1
```

### Environment Variables
- `CPU_OPT=1` - Enable native CPU optimizations
- `RUST_LOG=info` - Set logging level
- `SHARD_SIZE` - SP1 shard size (affects performance)
- `SHARD_BATCH_SIZE` - SP1 memory usage control

## Development Notes

- Guest programs compile with `opt-level = 3` even in dev profile for performance
- Release builds include debug symbols (`debug = 1`) and LTO
- OpenAPI documentation available at `/swagger-ui` and `/scalar` when running
- Execution traces can be generated with `--features tracer`
- The project uses Redis for task management - ensure Redis is running for full functionality