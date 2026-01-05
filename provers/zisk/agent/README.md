# ZISK Agent - Consolidated

A completely self-contained ZISK proof generation system with microservice architecture. This consolidated structure contains all ZISK components in one place for easy management and deployment.

## Overview

The ZISK Agent provides a microservice architecture that completely isolates ZISK's unique requirements (riscv64 architecture, specific toolchain dependencies, outdated crypto libraries) from the main Raiko application. This prevents build conflicts with SP1 and RISC0, which use different RISC-V architectures.

### Consolidated Structure

```
provers/zisk/agent/
├── service/          # HTTP agent service
├── driver/           # Raiko integration driver  
├── guest/            # RISC-V guest programs
├── builder/          # Build pipeline (legacy)
├── guest/elf/         # Compiled guest binaries (auto-generated)
├── build/            # Proof/publics output (auto-generated)
├── build.sh          # Unified build script
├── Cargo.toml        # Workspace configuration
└── README.md         # This file
```

This structure makes ZISK completely independent - you can develop, build, test, and deploy ZISK without touching any other part of Raiko.

## Architecture

```
┌─────────────────┐    HTTP     ┌─────────────────┐
│                 │   Request   │                 │
│  Raiko Host     │─────────────▶│  ZISK Agent     │
│  (ZISK Driver)  │             │  (Port 9998)    │
│                 │◀─────────────│                 │
└─────────────────┘   Response  └─────────────────┘
                                          │
                                          ▼
                                ┌─────────────────┐
                                │  cargo-zisk     │
                                │  (riscv64)      │
                                └─────────────────┘
```

## Features

- **Batch Proof Generation**: Generate proofs for individual blocks
- **Aggregation Proof Generation**: Aggregate multiple batch proofs
- **Shasta Aggregation**: Aggregate Shasta proposal proofs
- **Concurrent Execution**: Optional MPI support for parallel processing
- **GPU Acceleration**: Automatic CUDA detection and support
- **Health Monitoring**: Health check endpoint for service monitoring
- **Isolated Dependencies**: No conflicts with SP1/RISC0 build requirements

## Prerequisites

1. **ZISK Toolchain**: Install cargo-zisk
   ```bash
   curl https://raw.githubusercontent.com/0xPolygonHermez/zisk/main/ziskup/install.sh | bash
   source ~/.bashrc
   ```

2. **Rust Toolchain**: Build script expects nightly-2024-12-20
   ```bash
   rustup toolchain install nightly-2024-12-20
   ```

3. **Optional - CUDA Toolkit**: For GPU acceleration
   ```bash
   # Ubuntu/Debian
   sudo apt install nvidia-cuda-toolkit
   ```

4. **Optional - MPI**: For concurrent processing
   ```bash
   # Ubuntu/Debian  
   sudo apt install openmpi-bin openmpi-dev
   ```

## Building

### Quick Build (All Components)
```bash
./build.sh all
```

### Step by Step

1. **Build Guest Programs** (riscv64 ELF files):
   ```bash
   ./build.sh guest
   ```

2. **Build Agent Service**:
   ```bash
   ./build.sh agent
   ```

### Build Commands

| Command | Description |
|---------|-------------|
| `./build.sh all` | Build everything (default) |
| `./build.sh guest` | Build only ZISK guest programs |
| `./build.sh agent` | Build only agent service |
| `./build.sh clean` | Clean build artifacts |
| `./build.sh check` | Check dependencies |

## Running the Agent

### Basic Usage
```bash
# Start agent on default port (9998)
./target/release/zisk-agent

# Start with custom configuration
./target/release/zisk-agent --port 9000 --verbose
```

### Command Line Options

| Option | Description | Default |
|--------|-------------|---------|
| `--port` | Port to listen on | 9998 |
| `--host` | Host to bind to | 0.0.0.0 |
| `--verbose` | Enable verbose logging | false |
| `--concurrent-processes` | MPI processes | None (single process) |
| `--threads-per-process` | Threads per process | None (default) |
| `--verify` | Enable proof verification | true |

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `RUST_LOG` | Logging level | info |

## API Endpoints

### Health Check
```
GET /health
```

**Response:**
```json
{
  "status": "healthy",
  "service": "zisk-agent",
  "version": "0.1.0"
}
```

### Generate Proof
```
POST /proof
```

**Request Body:**
```json
{
  "input": [/* input data as byte array */],
  "proof_type": "batch" | "aggregate" | "shasta_aggregate",
  "config": {/* optional configuration */},
  "expected_input": [/* optional 32-byte array for batch */]
}
```

**Response:**
```json
{
  "proof_data": [/* proof data as byte array */],
  "proof_type": "batch" | "aggregate" | "shasta_aggregate", 
  "success": true,
  "error": null
}
```

`proof_data` is a bincode-encoded `ZiskResponse` containing:
- `proof`: hex-encoded proof bytes (0x...)
- `receipt`: optional string
- `input`: 32-byte public input (from guest output)
- `uuid`: request id

## Integration with Raiko

The ZISK driver in Raiko automatically detects and uses the agent when configured:

### Environment Configuration
```bash
# Set agent URL (optional - defaults to localhost:9998)
export ZISK_AGENT_URL="http://localhost:9998/proof"
```

### Raiko Configuration
The existing ZISK configuration in Raiko continues to work. The driver will:
1. Try to connect to the agent first
2. Return an error if agent is unavailable (no local fallback)

## Performance Tuning

### CPU Optimization
```bash
# Single process with multiple threads
./target/release/zisk-agent --threads-per-process 16

# Multiple processes with MPI
./target/release/zisk-agent --concurrent-processes 4 --threads-per-process 8
```

### GPU Acceleration
GPU support is automatically enabled when CUDA toolkit is detected during build.

### Memory Management
The agent cleans request-scoped build folders after proof generation. Proofs/publics are written to fixed output paths (see below).

### Proof/Public Output Paths
`cargo-zisk` writes proof and publics files to fixed directories so the host can read them consistently:
- `build/zisk-output/batch`
- `build/zisk-output/aggregation`
- `build/zisk-output/shasta`

Each run overwrites `vadcop_final_proof.bin` and `publics.json` in the corresponding folder. This is not concurrency-safe per proof type.

## Monitoring and Logging

### Health Monitoring
```bash
# Check if agent is healthy
curl http://localhost:9998/health
```

### Logging Levels
```bash
# Debug logging
RUST_LOG=debug ./target/release/zisk-agent

# Component-specific logging
RUST_LOG=zisk_agent=debug,axum::routing=info ./target/release/zisk-agent
```

## Troubleshooting

### Common Issues

1. **Agent fails to start**:
   - Check if cargo-zisk is installed: `which cargo-zisk`
   - Verify port is not in use: `lsof -i :9998`

2. **Proof generation fails**:
   - Check agent logs for detailed error messages
   - Verify ELF files exist: `ls -la guest/elf/`
   - Test cargo-zisk manually: `cargo-zisk --help`

3. **Performance issues**:
   - Monitor system resources: `htop`, `nvidia-smi`
   - Adjust concurrent processes and threads
   - Check disk space for temporary files

### Debug Mode
```bash
# Start agent with maximum logging
RUST_LOG=trace ./target/release/zisk-agent --verbose
```

## Development

### Project Structure
```
provers/zisk/agent/
├── service/src/
│   ├── main.rs       # HTTP server and CLI
│   ├── prover.rs     # Core ZISK proof logic  
│   └── handlers.rs   # API request handlers
├── driver/src/        # Raiko integration driver
├── guest/src/         # Guest programs (batch/aggregation)
├── guest/elf/         # Pre-built guest programs (auto-generated)
├── build.sh          # Build script
├── Cargo.toml        # Dependencies (isolated workspace)
└── README.md         # This file
```

### Adding New Features
1. Modify the prover logic in `service/src/prover.rs`
2. Update API handlers in `service/src/handlers.rs` if needed
3. Rebuild: `./build.sh agent`
4. Test changes with health check and sample requests

## Security Considerations

- The agent runs on all interfaces (0.0.0.0) by default
- Consider firewall rules in production
- Agent does not implement authentication - use reverse proxy if needed
- Temporary files are cleaned up automatically but contain sensitive data briefly

## License

Same as parent Raiko project.
