# Boundless Agent API

## Overview

The Boundless Agent is a web service that acts as an intermediary between the Raiko server and the Boundless market for zero-knowledge proof generation. It provides a REST API for submitting proof requests, monitoring their progress, and retrieving completed proofs.

### Architecture

```
Raiko Server → Boundless Agent → Boundless Market → Proof Providers
```

The Boundless Agent:
- Receives proof requests from the Raiko server
- Submits them to the Boundless market for distributed proving
- Polls the market for completion status
- Returns completed proofs back to the Raiko server

### Key Concepts

- **Asynchronous Processing**: All proof requests are processed asynchronously. The agent returns a request ID immediately and the client must poll for completion status.
- **Request Lifecycle**: Requests go through multiple states: `preparing` → `submitted` → `in_progress` → `completed`/`failed`
- **Proof Types**: Supports batch proofs, aggregation proofs, and ELF update proofs
- **Market Integration**: Automatically handles Boundless market submission, pricing, and prover assignment

## Base URL

The default Boundless Agent runs on `http://localhost:9999` but can be configured via the `BOUNDLESS_AGENT_URL` environment variable.

## API Endpoints

### Health Check

#### `GET /health`

Check if the agent service is running and healthy.

**Response:**
```json
{
  "status": "healthy",
  "service": "boundless-agent"
}
```

### Submit Proof Request

#### `POST /proof`

Submit a new proof request for asynchronous processing.

> **Required contract:** The `input` field must contain a bincode-encoded Rust struct (see [Payload Encoding & Contracts](#payload-encoding--contracts)). Supplying JSON or other encodings will cause Raiko’s driver to fail deserialization.

**Request Body:**
```json
{
  "input": [/* binary data as array of bytes */],
  "proof_type": "Batch" | "Aggregate" | {"Update": "ElfType"},
  "elf": [/* optional: ELF binary data for Update proof type */],
  "config": {/* optional: additional configuration */}
}
```

**Request Parameters:**
- `input` (array of numbers): Binary input data serialized as byte array
- `proof_type` (string|object): Type of proof to generate:
  - `"Batch"`: Generate a batch proof
  - `"Aggregate"`: Aggregate multiple existing proofs
  - `{"Update": "Batch"}` or `{"Update": "Aggregation"}`: Update ELF binary (requires `elf` field)
- `elf` (array of numbers, optional): ELF binary data, required for Update proof types
- `config` (object, optional): Additional prover configuration

**Response (202 Accepted):**
```json
{
  "request_id": "req_12345abc...",
  "market_request_id": "0",
  "status": "preparing",
  "message": "Proof request received and preparing for market submission"
}
```

**Response Fields:**
- `request_id` (string): Unique identifier for tracking this request
- `market_request_id` (string): Boundless market order ID (initially "0", updated when submitted)
- `status` (string): Current request status
- `message` (string): Human-readable status description

**Error Response (500 Internal Server Error):**
```json
{
  "request_id": "req_12345abc...",
  "market_request_id": "0",
  "status": "error",
  "message": "Failed to submit proof: [error details]"
}
```

### Upload Prover ELF Images

Raiko uploads the RISC0 batch and aggregation ELFs during startup. Your implementation must accept these binaries, compute the image IDs, and return the metadata so the driver can verify the contents match its expectations.

#### `POST /upload-image/{image_type}`

**Path Parameters:**
- `image_type` (string): `"batch"` or `"aggregation"`

**Request:**
- Content-Type: `application/octet-stream`
- Body: raw ELF bytes (max 50 MB)

**Response (200 OK):**
```json
{
  "image_id": [3537337764,1055695413,664197713,1225410428,3705161813,2151977348,4164639052,2614443474],
  "status": "uploaded",
  "market_url": "https://storage.example/programs/batch123",
  "message": "batch image processed successfully"
}
```

**Error cases:**
- 400 for invalid image type or empty payload (`InvalidImageType`, `EmptyELF`, `ELFTooLarge`)
- 429 when rate limits are exceeded
- 500 if the upload or storage integration fails

### Query Uploaded Images

#### `GET /images`

Returns metadata for the currently registered ELFs. Raiko calls this endpoint to confirm that both images are available (particularly after agent restarts).

**Response (200 OK):**
```json
{
  "batch": {
    "uploaded": true,
    "image_id": [3537337764,1055695413,664197713,1225410428,3705161813,2151977348,4164639052,2614443474],
    "image_id_hex": "0xd2b5a444...",
    "market_url": "https://storage.example/programs/batch123",
    "elf_size_bytes": 8723456
  },
  "aggregation": {
    "uploaded": true,
    "image_id": [2700732721,2547473741,423687947,895656626,623487531,3508625552,2848442538,2984275190],
    "image_id_hex": "0xa0f2b431...",
    "market_url": "https://storage.example/programs/agg456",
    "elf_size_bytes": 2432104
  }
}
```

If an image is not present, its entry will be `null`. Use this signal to trigger re-uploads when necessary.

### Check Request Status

#### `GET /status/{request_id}`

Get the current status of a proof request.

**Path Parameters:**
- `request_id` (string): The request ID returned from the proof submission

**Response (200 OK):**
```json
{
  "request_id": "req_12345abc...",
  "market_request_id": "123456789",
  "status": "completed",
  "status_message": "The proof has been successfully generated and is ready for download.",
  "proof_data": [/* binary proof data as array of bytes */],
  "error": null
}
```

**Response Fields:**
- `request_id` (string): The original request identifier
- `market_request_id` (string): Boundless market order ID. If you operate purely off-chain, return a stable placeholder such as `"0"`—Raiko expects the field to exist.
- `status` (string): Current status (see Status Values below)
- `status_message` (string): Detailed human-readable status description
- `proof_data` (array of numbers|null): Binary proof data when completed, null otherwise
- `error` (string|null): Error message if status is "failed"

**Status Values:**
- `"preparing"`: Request received, executing guest program and preparing for market submission
- `"submitted"`: Request submitted to Boundless market, waiting for prover assignment
- `"in_progress"`: A prover has accepted the request and is generating the proof
- `"completed"`: Proof generation completed successfully, proof_data is available
- `"failed"`: Proof generation failed, see error field for details

These strings form part of the public contract with Raiko’s driver. Do not rename them; instead map your internal lifecycle to the closest status.

**Error Response (404 Not Found):**
```json
{
  "error": "Request not found",
  "message": "No proof request found with the specified request_id"
}
```

### List Active Requests

#### `GET /requests`

List all active proof requests being tracked by the agent.

**Response (200 OK):**
```json
{
  "active_requests": 3,
  "requests": [
    {
      "request_id": "req_12345abc...",
      "market_request_id": "123456789",
      "status": "in_progress",
      "status_message": "A prover has accepted the request and is generating the proof.",
      "proof_data": null,
      "error": null
    },
    {
      "request_id": "req_67890def...",
      "market_request_id": "987654321", 
      "status": "completed",
      "status_message": "The proof has been successfully generated and is ready for download.",
      "proof_data": [/* binary proof data */],
      "error": null
    }
  ]
}
```

### Database Statistics

#### `GET /db/stats`

Get database statistics for monitoring purposes.

**Response (200 OK):**
```json
{
  "database_stats": {
    "total_requests": 1247,
    "active_requests": 3,
    "completed_requests": 1200,
    "failed_requests": 44,
    "database_size_bytes": 2048576
  }
}
```

### Delete All Requests

#### `DELETE /requests`

Delete all requests from the agent's database. Use with caution - this will remove all tracking information.

**Response (200 OK):**
```json
{
  "message": "Successfully deleted 1247 requests",
  "deleted_count": 1247
}
```

## Request Lifecycle

### 1. Preparing Phase
When a request is first submitted, the agent:
- Generates a deterministic request ID
- Prepares the request for market submission
- Returns status "preparing"

### 2. Market Submission
The agent:
- Submits the request to the Boundless market
- Updates status to "submitted"
- Receives a market_request_id for tracking

### 3. Prover Assignment
When a prover accepts the request:
- Status changes to "in_progress"
- The prover information may be included in status messages

### 4. Completion
When proof generation completes:
- Status becomes "completed" (success) or "failed" (error)
- For completed proofs, proof_data contains the binary proof
- For failed proofs, error field contains the failure reason

## Integration with Raiko Server

The Raiko server integrates with the Boundless Agent through the `BoundlessProver` struct in `provers/risc0/driver/src/boundless.rs`:

### Environment Configuration
- `BOUNDLESS_AGENT_URL`: Agent endpoint URL (default: "http://localhost:9999/proof")
- `BOUNDLESS_REQUEST_CONCURRENCY_LIMIT`: Configure HTTP request concurrency limit (default: 4)

### Proof Types Supported
- **Batch Proofs**: For individual batch proving using `batch_run()` method
- **Aggregation Proofs**: For combining multiple proofs using `aggregate()` method

### Polling Behavior
The Raiko server polls the agent status endpoint:
- **Poll Interval**: 15 seconds
- **Max Timeout**: 1 hour
- **Retry Logic**: Up to 8 retries with 5-second delays on network errors

## Error Handling

### Common Error Codes
- **400 Bad Request**: Invalid request format or missing required fields
- **404 Not Found**: Request ID not found
- **500 Internal Server Error**: Agent internal error (prover initialization, market communication, etc.)
- **502 Bad Gateway**: Boundless market communication error

### Error Response Format
All error responses follow this structure:
```json
{
  "error": "Error Type",
  "message": "Detailed error description"
}
```

### Troubleshooting
1. **"Failed to initialize prover"**: Check Boundless market connectivity and authentication
2. **"Request not found"**: Verify the request_id is correct and the request hasn't been deleted
3. **"Agent returned error status"**: Check agent logs for detailed error information
4. **Timeout errors**: Consider increasing timeout values for large proofs

## Configuration

### Agent Configuration
The Boundless Agent accepts these command-line arguments:
- `--address`: Bind address (default: "0.0.0.0")
- `--port`: Port number (default: 9999)
- Additional prover-specific configuration options

### Environment Variables
- `BOUNDLESS_AGENT_URL`: Full URL to the agent's /proof endpoint
- Database and logging configuration (see agent documentation)

## cURL Examples

#### Submit a Batch Proof
```bash
# Convert binary file to JSON array (using a helper script)
python3 -c "
import sys, json
with open('input.bin', 'rb') as f:
    data = list(f.read())
print(json.dumps({'input': data, 'proof_type': 'Batch', 'config': {}}))
" > request.json

# Submit the request
curl -X POST http://localhost:9999/proof \
  -H "Content-Type: application/json" \
  -d @request.json

# Response:
# {
#   "request_id": "req_abc123...",
#   "market_request_id": "0",
#   "status": "preparing",
#   "message": "Proof request received and preparing for market submission"
# }
```

#### Check Request Status
```bash
curl http://localhost:9999/status/req_abc123...

# Response:
# {
#   "request_id": "req_abc123...",
#   "market_request_id": "123456789",
#   "status": "in_progress", 
#   "status_message": "A prover has accepted the request and is generating the proof.",
#   "proof_data": null,
#   "error": null
# }
```

#### List All Requests
```bash
curl http://localhost:9999/requests

# Response:
# {
#   "active_requests": 2,
#   "requests": [
#     {
#       "request_id": "req_abc123...",
#       "status": "in_progress",
#       ...
#     }
#   ]
# }
```

#### Health Check
```bash
curl http://localhost:9999/health

# Response:
# {
#   "status": "healthy",
#   "service": "boundless-agent"
# }
```

## Best Practices

### For Agent Developers

1. **Implement Proper Error Handling**: Always check HTTP status codes and handle error responses appropriately.

2. **Use Exponential Backoff**: When polling for status, implement exponential backoff to avoid overwhelming the agent.

3. **Handle Network Failures**: Implement retry logic for transient network issues.

4. **Monitor Request Lifecycle**: Track requests through their entire lifecycle and handle all possible status transitions.

5. **Validate Input Data**: Ensure input data is properly serialized and within reasonable size limits.

### For Production Deployment

1. **Set Appropriate Timeouts**: Configure timeouts based on expected proof generation times.

2. **Monitor Agent Health**: Regularly check the `/health` endpoint and monitor database statistics.

3. **Database Maintenance**: Periodically clean up old completed requests to prevent database bloat.

4. **Logging and Monitoring**: Implement comprehensive logging and monitoring for debugging and performance analysis.

5. **Security Considerations**: If deployed in production, consider implementing authentication and rate limiting.

## Payload Encoding & Contracts

To drop in behind Raiko without driver changes, a third-party broker must match the same serialization that the in-repo agent uses. All payloads are encoded with [`bincode`](https://docs.rs/bincode/latest/bincode/) (little-endian, variable-length integers, no explicit length prefix).

### Proof Submission (`POST /proof`)

| `proof_type` value | Expected Rust type | Notes |
|--------------------|--------------------|-------|
| `"Batch"` | `raiko_lib::input::GuestBatchInput` | Includes Taiko batch metadata and inputs. |
| `"Aggregate"` | `provers::risc0::driver::boundless::Risc0AgengAggGuestInput` | Contains the batch image ID plus a list of `risc0_zkvm::Receipt` values. |
| `{"Update": "Batch"}` | Raw ELF bytes (not bincode) | Replaces the batch ELF; payload is the ELF file itself. |
| `{"Update": "Aggregation"}` | Raw ELF bytes | Replaces the aggregation ELF. |

If you cannot deserialize the `input` with `bincode::deserialize`, reject the request with a 400 rather than attempting to interpret it differently.
Both `GuestBatchInput` and `Risc0AgengAggGuestInput` are defined in the Raiko repository (`provers/risc0/driver/src/boundless.rs`) and can be referenced for precise field layouts.

### Proof Completion (`GET /status/{request_id}`)

When `status == "completed"`, the `proof_data` field is the bincode encoding of:

```rust
pub struct Risc0AgentResponse {
    pub seal: Vec<u8>,
    pub journal: Vec<u8>,
    pub receipt: Option<String>, // JSON of the boundless receipt for batch proofs
}
```

The Raiko driver deserializes this struct, verifies the Groth16 proof locally, and caches the bytes on disk. Do not switch to JSON or other serialization formats.
`Risc0AgentResponse` lives alongside the driver code in `provers/risc0/driver/src/boundless.rs`.

### Status & Retry Expectations

- Keep the status strings exactly: `preparing`, `submitted`, `in_progress`, `completed`, `failed`.
- Populate `status_message` with human-readable progress information.
- Provide a descriptive `error` string whenever the request fails.
- Leave `market_request_id` present. In non-market deployments, return `"0"` or another deterministic value.

### Image Lifecycle

1. Raiko uploads both ELFs through `POST /upload-image/{image_type}`. Your service must compute and return the RISC0 image ID (`[u32; 8]`) so the driver can verify it matches its compiled artifact.
2. The driver calls `GET /images` to confirm both ELFs are still registered, especially after restarts. Report missing images as `null` so the driver knows it must re-upload.
3. If you rotate URLs (e.g., presigned S3 links), refresh them internally while keeping the stored image IDs consistent.

## Limitations and Considerations

### Request Size Limits
- Large input data may hit HTTP request size limits
- Consider implementing request compression for large batches

### Concurrent Requests
- The agent can handle multiple concurrent requests
- Each request is tracked independently in the database

### Market Dependencies
- Agent functionality depends on Boundless market availability
- Network issues may affect request submission and status updates

### Data Persistence
- Request tracking data is stored in a local database
- Consider backup strategies for production deployments

By following the endpoint definitions and payload contracts above, a third-party prover can integrate with the Raiko driver without any code changes on the Raiko side.
