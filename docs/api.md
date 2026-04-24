# API

> **Note:** As of the Shasta-only refactor (2026-03), the API has been simplified. See [CHANGELOG-shasta-only-refactor.md](CHANGELOG-shasta-only-refactor.md) for migration details.

## Current Endpoints

- `POST /v3/proof/batch/shasta` — Shasta batch proof (supports aggregation via `aggregate: true`)
- `GET /v3/proof/report` — Task status report
- `GET /v3/proof/list` — List proofs
- `POST /v3/proof/prune` — Prune tasks

The same proof routes are also available at `/proof/*` (without the `/v3` prefix).

---

## `POST /v3/proof/batch/shasta`

Submit a Shasta batch proof task. Supports aggregation when `aggregate: true`.

### Request Parameters

- proposals(array of objects): An array of Shasta proposal metadata, each containing:
  - proposal_id(number): The proposal ID.
  - l1_inclusion_block_number(number): The L1 block number where the proposal was included.
  - l2_block_numbers(array of numbers): L2 block numbers in this proposal.
  - last_anchor_block_number(number): Last anchor block number.
  - checkpoint(object, optional): `{ block_number, block_hash, state_root }`.
- aggregate(boolean, optional): Whether to aggregate the proofs of all proposals. Default is false.
- network(string): The L2 network (e.g., "taiko_a7").
- l1_network(string): The L1 network (e.g., "hoodi").
- graffiti(string, optional): A 32-byte hex string. Defaults to zero.
- prover(address): The Ethereum address of the prover.
- proof_type(string): `native`, `sgx`, `risc0`, `sp1`, or `zk_any`.
- blob_proof_type(string, optional): `kzg_versioned_hash` or `proof_of_equivalence`. Default: `proof_of_equivalence`.
- prover_args(object, optional): Prover-specific options (native, sgx, sp1, risc0).

### Response Parameters

- status(string): "ok" or "error".
- data(object): Contains `status` ("registered", "success", "failed") and optionally `proof` when done.
- proof_type(string): The proof type used.

### Example

```sh
curl --location \
     --request POST http://localhost:8080/v3/proof/batch/shasta \
     --header 'Content-Type: application/json' \
     --data-raw '{
         "network": "taiko_a7",
         "l1_network": "hoodi",
         "proposals": [
           {
             "proposal_id": 429,
             "l1_inclusion_block_number": 2071,
             "l2_block_numbers": [100, 101, 102],
             "last_anchor_block_number": 99,
             "checkpoint": null
           }
         ],
         "aggregate": false,
         "proof_type": "sgx"
       }'
```

Response:

```json
{"data":{"status":"registered"},"proof_type":"sgx","status":"ok"}
```

Success response (when proof is ready):

```json
{"data": {"status": "success", "proof": ...}, "proof_type": "sgx", "status": "ok"}
```
