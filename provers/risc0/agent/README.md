# RISC0 Boundless Agent

A command-line application for generating RISC0 Boundless proofs with support for both batch and aggregation proof types.

## Features

- **Batch Proof Generation**: Generate proofs for individual blocks
- **Aggregation Proof Generation**: Aggregate multiple batch proofs
- **Flexible Input/Output**: Support for custom input and output files
- **Configuration**: JSON-based configuration support
- **ELF Support**: Custom ELF file support (optional)
- **Verbose Logging**: Detailed output for debugging

## Installation

```bash
cargo build --release
```

## Usage

### Basic Usage

```bash
# Generate a batch proof
./target/release/boundless-agent -i input.bin -o proof.bin -t batch

# Generate an aggregation proof
./target/release/boundless-agent -i input.bin -o proof.bin -t agg

# With verbose output
./target/release/boundless-agent -i input.bin -o proof.bin -t batch -v
```

### Advanced Usage

```bash
# With custom configuration
./target/release/boundless-agent -i input.bin -o proof.bin -c config.json -t batch

# With custom ELF file
./target/release/boundless-agent -i input.bin -o proof.bin -e custom.elf -t batch

# Full example with all options
./target/release/boundless-agent \
  -i input.bin \
  -o proof.bin \
  -t agg \
  -c config.json \
  -e custom.elf \
  -v
```

## Command Line Options

| Option | Short | Long | Description | Required |
|--------|-------|------|-------------|----------|
| Input file | `-i` | `--input` | Path to input file | Yes |
| Output file | `-o` | `--output` | Path to output file (proof will be saved here) | Yes |
| Proof type | `-t` | `--proof-type` | `batch` or `agg` | No (default: batch) |
| ELF file | `-e` | `--elf` | Path to custom ELF file | No |
| Config file | `-c` | `--config` | Path to JSON config file | No |
| Verbose | `-v` | `--verbose` | Enable verbose output | No |

## Configuration

The application supports JSON-based configuration. See `config.example.json` for a complete example.

### Configuration Structure

```json
{
  "boundless": {
    "rpc_url": "https://ethereum-sepolia-rpc.publicnode.com",
    "deployment": "sepolia",
    "max_price": "0.0005",
    "min_price": "0.0001",
    "timeout": 4000,
    "lock_timeout": 2000,
    "ramp_up": 1000
  },
  "prover": {
    "execution_po2": 18,
    "profile": false
  }
}
```

### Deployment Types

The agent supports two deployment types:

1. **Sepolia** (default): Uses the Sepolia testnet deployment
2. **Base**: Uses the Base mainnet deployment

The `order_stream_url` can be used to override the default order stream URL for any deployment type.

For detailed configuration examples, see [DEPLOYMENT_CONFIG_EXAMPLES.md](DEPLOYMENT_CONFIG_EXAMPLES.md).

## Environment Variables

- `BOUNDLESS_SIGNER_KEY`: Private key for signing transactions (required)

## Command Line Arguments

- `--deployment-type`: Deployment type to use (sepolia or base, default: sepolia)
- `--rpc-url`: RPC URL for the deployment (default: https://ethereum-sepolia-rpc.publicnode.com)
- `--order-stream-url`: Order stream URL (optional)
- `--offchain`: Enable offchain mode (default: false)
- `--pull-interval`: Pull interval in seconds (default: 10)
- `--signer-key`: Signer private key (optional, can also be set via BOUNDLESS_SIGNER_KEY env var)

## Output

The application saves the generated proof data to the specified output file in binary format.

```bash
# The proof will be saved to proof.bin
./target/release/boundless-agent -i input.bin -o proof.bin -t batch
```

## Error Handling

The application provides detailed error messages for:
- File read/write errors
- Invalid configuration
- Network errors
- Proof generation failures

## Examples

### Generate a Batch Proof

```bash
# Simple batch proof
./target/release/boundless-agent -i block_input.bin -o batch_proof.bin -t batch

# With verbose output
./target/release/boundless-agent -i block_input.bin -o batch_proof.bin -t batch -v
```

### Generate an Aggregation Proof

```bash
# Simple aggregation proof
./target/release/boundless-agent -i agg_input.bin -o agg_proof.bin -t agg

# With custom configuration
./target/release/boundless-agent -i agg_input.bin -o agg_proof.bin -t agg -c my_config.json
```

### Using Custom ELF Files

```bash
# With custom ELF for batch
./target/release/boundless-agent -i input.bin -o proof.bin -e custom_batch.elf -t batch

# With custom ELF for aggregation
./target/release/boundless-agent -i input.bin -o proof.bin -e custom_agg.elf -t agg
```

### Using Different Deployment Types

```bash
# Use Sepolia deployment (default)
./target/release/boundless-agent --deployment-type sepolia

# Use Base deployment
./target/release/boundless-agent --deployment-type base

# Use custom order stream URL
./target/release/boundless-agent --deployment-type sepolia --order-stream-url https://custom-order-stream.com

# Use offchain mode
./target/release/boundless-agent --deployment-type base --offchain --order-stream-url https://offchain-order-stream.com
```

## Development

### Building

```bash
# Debug build
cargo build

# Release build
cargo build --release
```

### Testing

```bash
cargo test
```

### Running Tests with Logging

```bash
RUST_LOG=debug cargo test
``` 