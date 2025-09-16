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
  - `{"Update": "BatchElf"}`: Update ELF binary (requires `elf` field)
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
- `market_request_id` (string): Boundless market order ID
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

This API specification provides everything needed to integrate with the Boundless Agent for distributed zero-knowledge proof generation within the Raiko ecosystem.