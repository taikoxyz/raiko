# Raiko V1 to V2 Migration Guide

## Overview

Raiko V2 is a major rewrite focused on simplification and Shasta-only support. This guide helps operators migrate from V1 to V2.

## Key Differences

| Aspect           | V1                     | V2                  |
| ---------------- | ---------------------- | ------------------- |
| Hardfork Support | Ontake, Pacaya, Shasta | **Shasta only**     |
| Provers          | SGX, RISC0, SP1        | **RISC0, SP1 only** |
| Binary           | `raiko-host`           | `raiko2`            |
| Config Format    | JSON                   | **TOML**            |
| API Prefix       | `/v1/`                 | `/v2/`              |
| Package Names    | `raiko-*`              | `raiko2-*`          |

## Breaking Changes

### 1. No SGX Support

V2 removes SGX prover support entirely. If you're using SGX:

- Continue using V1 for SGX proofs
- Migrate to RISC0 or SP1 for zkVM proofs

### 2. Shasta Hardfork Only

V2 only supports Shasta hardfork. For older hardforks:

- Use V1 for Ontake/Pacaya blocks
- Use V2 for Shasta blocks only

### 3. New API Endpoints

| V1 Endpoint      | V2 Endpoint            | Notes                     |
| ---------------- | ---------------------- | ------------------------- |
| `POST /proof`    | `POST /v2/proof/batch` | Request structure changed |
| `GET /proof/:id` | `GET /v2/proof/:id`    | Same                      |
| `GET /health`    | `GET /health`          | Response format changed   |

### 4. Configuration Format

V1 uses JSON, V2 uses TOML:

**V1 (config.json):**

```json
{
  "address": "0.0.0.0:8080",
  "l1_rpc": "https://...",
  "l2_rpc": "https://...",
  "prover": "risc0"
}
```

**V2 (config.toml):**

```toml
[server]
host = "0.0.0.0"
port = 8080

[rpc]
l1_rpc = "https://..."
l2_rpc = "https://..."

[prover]
prover_type = "risc0"
```

## Migration Steps

### Step 1: Check Compatibility

1. Verify you're running Shasta hardfork blocks only
2. Verify you're using RISC0 or SP1 prover (not SGX)

### Step 2: Update Configuration

Convert your V1 JSON config to V2 TOML:

```bash
# Example conversion
cat config.json | jq -r '
  "[server]\nhost = \"" + (.address | split(":")[0]) + "\"\nport = " + (.address | split(":")[1]) + "\n\n[rpc]\nl1_rpc = \"" + .l1_rpc + "\"\nl2_rpc = \"" + .l2_rpc + "\"\n\n[prover]\nprover_type = \"" + .prover + "\""
' > config.toml
```

### Step 3: Update API Calls

Update your proof request client:

**V1 Request:**

```json
{
  "block_number": 12345,
  "l1_block_number": 50000,
  "prover": "risc0"
}
```

**V2 Request:**

```json
{
  "batch_id": 12345,
  "l1_inclusion_block": 50000,
  "prover_type": "risc0"
}
```

### Step 4: Update Docker Images

**V1:**

```bash
docker pull taikoxyz/raiko:latest
docker run taikoxyz/raiko:latest
```

**V2:**

```bash
docker build -f Dockerfile.raiko2 -t raiko2:latest .
docker run raiko2:latest
```

### Step 5: Update Environment Variables

| V1 Variable   | V2 Variable     |
| ------------- | --------------- |
| `RAIKO_HOST`  | `RAIKO2_HOST`   |
| `RAIKO_PORT`  | `RAIKO2_PORT`   |
| `L1_RPC_URL`  | `RAIKO2_L1_RPC` |
| `L2_RPC_URL`  | `RAIKO2_L2_RPC` |
| `PROVER_TYPE` | `RAIKO2_PROVER` |

### Step 6: Deploy and Test

1. Deploy V2 alongside V1 initially
2. Route new Shasta requests to V2
3. Monitor for errors
4. Deprecate V1 once stable

## Parallel Operation

V1 and V2 can run simultaneously:

```yaml
# docker-compose.yml
services:
  raiko-v1:
    image: taikoxyz/raiko:latest
    ports:
      - "8080:8080"
    # For legacy hardforks

  raiko-v2:
    build:
      dockerfile: Dockerfile.raiko2
    ports:
      - "8081:8080"
    # For Shasta hardfork
```

## Rollback

If V2 has issues, rollback is straightforward:

1. Stop V2 container
2. Route traffic back to V1
3. V1 can handle all requests

## Support Matrix

| Block Range     | Hardfork      | Recommended Version |
| --------------- | ------------- | ------------------- |
| < Ontake        | Legacy        | V1                  |
| Ontake - Pacaya | Ontake/Pacaya | V1                  |
| Shasta+         | Shasta        | V2                  |

## FAQ

### Q: Can I use V2 for Pacaya blocks?

No. V2 only supports Shasta hardfork. Use V1 for older hardforks.

### Q: Is SGX support coming back to V2?

No. V2 focuses on zkVM provers only. SGX users should continue using V1.

### Q: Are V1 proofs compatible with V2?

The proof format is compatible, but V2 uses different protocol types internally.

### Q: How do I report V2 issues?

Open an issue on GitHub with the `raiko-v2` label.
