# API

## `POST /v2/proof`

### Request Parameters

- proof_type(string), the type of proof to prove. Allowed values: `zk_any`, `native`, `sgx`, `risc0`, `sp1`. When using `zk_any`, the server will automatically determine the proof type (risc0 or sp1) based on the request parameters.

> NOTE: The support of `zk_any` is introduced in https://github.com/taikoxyz/raiko/pull/454

### Response Parameters

- proof_type(string), the type of proof that was proven. Allowed values: `Native`, `Sgx`, `Risc0`, `Sp1`. When requesting a `zk_any` proof, the server will automatically determine the proof type (risc0 or sp1) based on the request parameters, then return the proof type in the response.

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
         "graffiti": "8008500000000000000000000000000000000000000000000000000000000000"
       }'
```

Response:

```json
{"data":{"status":"registered"},"proof_type":"Risc0","status":"ok"}
```

Error response:

```json
{"error":"zk_any_not_drawn_error","message":"The zk_any request is not drawn","status":"error"}
```
