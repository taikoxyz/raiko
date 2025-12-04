# Raiko V2 API Documentation

## Overview

Raiko V2 provides a REST API for requesting and managing zkVM proofs for Taiko's Shasta hardfork.

## Base URL

```
http://localhost:8080
```

## Authentication

Currently no authentication is required. Production deployments should add authentication.

## Endpoints

### Health Check

Check if the server is running and healthy.

```http
GET /health
```

#### Response

```json
{
  "status": "ok",
  "version": "0.1.0",
  "prover": "risc0",
  "supported_provers": ["risc0", "sp1"]
}
```

### Request Batch Proof

Request a proof for a batch of blocks.

```http
POST /v2/proof/batch
Content-Type: application/json
```

#### Request Body

| Field                | Type     | Required | Description                                        |
| -------------------- | -------- | -------- | -------------------------------------------------- |
| `batch_id`           | `u64`    | Yes      | The batch ID to prove                              |
| `l1_inclusion_block` | `u64`    | Yes      | The L1 block number where the batch was included   |
| `prover_type`        | `string` | No       | Prover type: "risc0" or "sp1" (defaults to config) |
| `blob_proof_type`    | `string` | No       | Blob proof type: "kzg" or "proof_of_equivalence"   |
| `prover`             | `string` | No       | Prover address (hex)                               |
| `graffiti`           | `string` | No       | Custom graffiti string                             |

#### Example Request

```json
{
  "batch_id": 12345,
  "l1_inclusion_block": 50000,
  "prover_type": "risc0",
  "blob_proof_type": "kzg"
}
```

#### Response

```json
{
  "id": "proof-12345-50000-1733318400",
  "status": "pending"
}
```

#### Status Codes

| Code | Description                |
| ---- | -------------------------- |
| 200  | Proof request accepted     |
| 400  | Invalid request parameters |
| 500  | Internal server error      |

### Get Proof Status

Query the status of a proof request.

```http
GET /v2/proof/{proof_id}
```

#### Path Parameters

| Parameter  | Type     | Description                                  |
| ---------- | -------- | -------------------------------------------- |
| `proof_id` | `string` | The proof ID returned from the batch request |

#### Response

```json
{
  "id": "proof-12345-50000-1733318400",
  "status": "completed",
  "proof": "0x...",
  "input": "0x...",
  "created_at": "2025-12-04T15:00:00Z",
  "completed_at": "2025-12-04T15:05:00Z"
}
```

#### Proof Status Values

| Status      | Description                                     |
| ----------- | ----------------------------------------------- |
| `pending`   | Proof request received, waiting to be processed |
| `proving`   | Proof generation in progress                    |
| `completed` | Proof successfully generated                    |
| `failed`    | Proof generation failed                         |
| `cancelled` | Proof request was cancelled                     |

## Configuration

### Environment Variables

| Variable             | Default   | Description                 |
| -------------------- | --------- | --------------------------- |
| `RAIKO2_HOST`        | `0.0.0.0` | Server bind address         |
| `RAIKO2_PORT`        | `8080`    | Server port                 |
| `RAIKO2_L1_RPC`      | -         | L1 RPC endpoint URL         |
| `RAIKO2_L2_RPC`      | -         | L2 RPC endpoint URL         |
| `RAIKO2_PROVER`      | `risc0`   | Default prover type         |
| `RAIKO2_L1_CHAIN_ID` | `1`       | L1 chain ID                 |
| `RAIKO2_L2_CHAIN_ID` | `167000`  | L2 chain ID (Taiko Mainnet) |
| `RAIKO2_CONFIG`      | -         | Path to config file         |
| `RUST_LOG`           | `info`    | Log level                   |

### Config File (TOML)

```toml
[server]
host = "0.0.0.0"
port = 8080

[rpc]
l1_rpc = "https://ethereum-rpc.example.com"
l2_rpc = "https://taiko-rpc.example.com"

[prover]
prover_type = "risc0"
# risc0-specific
bonsai_api_key = "..."
bonsai_api_url = "https://api.bonsai.xyz"
# sp1-specific
sp1_private_key = "..."

[chain]
l1_chain_id = 1
l2_chain_id = 167000
```

## CLI Usage

```bash
# Start with default settings
raiko2

# Start with custom port
raiko2 --port 9090

# Start with config file
raiko2 --config /etc/raiko/config.toml

# Start with environment overrides
RAIKO2_L1_RPC=https://... RAIKO2_L2_RPC=https://... raiko2

# Enable verbose logging
raiko2 --verbose

# Output JSON logs
raiko2 --json-logs
```

## Error Responses

All error responses follow this format:

```json
{
  "error": {
    "code": "INVALID_BATCH_ID",
    "message": "Batch ID 12345 not found on L1"
  }
}
```

### Error Codes

| Code               | HTTP Status | Description                        |
| ------------------ | ----------- | ---------------------------------- |
| `INVALID_REQUEST`  | 400         | Malformed request body             |
| `INVALID_BATCH_ID` | 400         | Batch ID not found                 |
| `INVALID_PROVER`   | 400         | Unsupported prover type            |
| `PROOF_NOT_FOUND`  | 404         | Proof ID not found                 |
| `PROVER_ERROR`     | 500         | Error during proof generation      |
| `RPC_ERROR`        | 502         | Error communicating with L1/L2 RPC |

## Docker

```bash
# Build image
docker build -f Dockerfile.raiko2 -t raiko2:latest .

# Run container
docker run -d \
  -p 8080:8080 \
  -e RAIKO2_L1_RPC=https://... \
  -e RAIKO2_L2_RPC=https://... \
  raiko2:latest
```

## Examples

### cURL

```bash
# Health check
curl http://localhost:8080/health

# Request proof
curl -X POST http://localhost:8080/v2/proof/batch \
  -H "Content-Type: application/json" \
  -d '{"batch_id": 12345, "l1_inclusion_block": 50000}'

# Get proof status
curl http://localhost:8080/v2/proof/proof-12345-50000-1733318400
```

### Python

```python
import requests

# Request proof
response = requests.post(
    "http://localhost:8080/v2/proof/batch",
    json={
        "batch_id": 12345,
        "l1_inclusion_block": 50000,
        "prover_type": "risc0"
    }
)
proof_id = response.json()["id"]

# Poll for completion
while True:
    status = requests.get(f"http://localhost:8080/v2/proof/{proof_id}").json()
    if status["status"] in ["completed", "failed"]:
        break
    time.sleep(10)
```
