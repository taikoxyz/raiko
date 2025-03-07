# API

## `POST /v2/proof`

### Request Parameters

- proof_type(string), the type of proof to prove. Allowed values: `zk_any`, `native`, `sgx`, `risc0`, `sp1`. When using `zk_any`, the server will automatically determine the proof type (risc0 or sp1) based on the request parameters.

> NOTE: The support of `zk_any` is introduced in https://github.com/taikoxyz/raiko/pull/454

### Response Parameters

- proof_type(string), the type of proof that was proven. Allowed values: `native`, `sgx`, `risc0`, `sp1`. When requesting a `zk_any` proof, the server will automatically determine the proof type (risc0 or sp1) based on the request parameters, then return the proof type in the response.

> NOTE: The `proof_type` field in response is introduced in https://github.com/taikoxyz/raiko/pull/454

### Example

```sh
curl --location \
     --request POST http://localhost:8091/v2/proof \
     --header 'Content-Type: application/json' \
     --data-raw '{
         "network": "taiko_a7",
         "proof_type": "zk_any",
         "l1_network": "holesky",
         "block_numbers": [[4, null], [5, null]],
         "block_number": 4,
         "prover": "0x70997970C51812dc3A010C7d01b50e0d17dc79C8",
       }'
```

Response:

```json
{"data":{"status":"registered"},"proof_type":"risc0","status":"ok"}
```

Error response:

```json
{"data":{"status":"zk_any_not_drawn"},"proof_type":"native","status":"ok"}
```

## `POST /v3/proof/batch`

Submit a batch proof task with requested config, get task status, or get proof value.

### Request Parameters

- batches(array of objects): An array of batch metadata objects, each containing:
  - batch_id(number): The batch ID to generate a proof for.
  - l1_inclusion_block_number(number): The L1 block number where the batch was proposed.
- aggregate(boolean, optional): Whether to aggregate the proofs of all batches. Default is false.
- network(string): The L2 network to generate the proof for (e.g., "taiko_a7").
- l1_network(string): The L1 network to generate the proof for (e.g., "holesky").
- graffiti(string): A 32-byte hex string used as graffiti.
- prover(address): The Ethereum address of the prover.
- proof_type(string): The type of proof to generate. Allowed values: `native`, `sgx`, `risc0`, `sp1`, "zk_any"
- blob_proof_type(string): The type of blob proof. Allowed values: `kzg_versioned_hash`, `proof_of_equivalence`.
- prover_args(object, optional): Additional prover-specific parameters:
  - native(object, optional): Native prover specific options.
  - sgx(object, optional): SGX prover specific options.
  - sp1(object, optional): SP1 prover specific options.
  - risc0(object, optional): RISC0 prover specific options.

### Response Parameters

- status(string): The status of the request. Possible values: "ok", "error".
- data(object): The response data containing:
  - status(string): The status of the proof generation. Possible values: "registered", "success", "failed".
  - proof(object, optional): The generated proof if status is "success".
- proof_type(string): The type of proof that was generated.

### Example

```sh
curl --location \
     --request POST http://localhost:8080/v3/proof/batch \
     --header 'Content-Type: application/json' \
     --data-raw '{
         "network": "taiko_a7",
         "l1_network": "holesky",
         "batches": [
           {"batch_id": 429, "l1_inclusion_block_number": 2071},
           {"batch_id": 213, "l1_inclusion_block_number": 1656}
         ],
         "aggregate": true,
         "proof_type": "sgx"
       }'
```

This example will generate a proof for batch 429 and 213, and **aggregate** them into a single proof, as the request parameter `aggregate` is set to `true`.

Response:

```json
{"data":{"status":"registered"},"proof_type":"risc0","status":"ok"}
```

Success response (when proof is ready):

```json
{"data": {"status": "success", "proof": ...}, "proof_type": "risc0", "status": "ok"}
```
