# Mock Studio Debug Guide

## Purpose

`mock_studio` is the ticket-style orchestrator for the Shasta mock flow. It receives one requirement, calls OpenRouter twice, writes a generated rule into `mock-gateway/generated/<rule_id>/`, builds and starts `raiko-mock-gateway`, checks `/health`, and returns a receipt with the running base URL.

The first version is intentionally simple:

- single machine
- synchronous request flow
- one target API: `/v3/proof/batch/shasta`
- deterministic local orchestration
- no auto-repair loop

## Architecture

The backend flow is:

1. `ticket requirement -> spec`
2. `task spec + shasta api spec -> restricted handler`
3. write generated files
4. `cargo build -p raiko-mock-gateway`
5. start `raiko-mock-gateway`
6. poll `/health`
7. write `receipt.json`

OpenRouter is only used for reasoning and code generation.
Local Rust code controls the orchestration, file layout, build, launch, and receipt handling.

`mock_studio` now feeds the second LLM call with two kinds of context:

- `Task Spec`: the requested mock behavior
- `Shasta API Spec`: a curated local summary of `/v3/proof/batch/shasta`, response helpers, aggregation notes, and memory contract notes

The generated handler is still restricted. It may only use the fixed helper and context surface defined by the system.

## Required Environment

At minimum:

```bash
export OPENROUTER_API_KEY=...
```

Optional model overrides:

```bash
export OPENROUTER_MODEL=openai/gpt-4o-mini
export OPENROUTER_SPEC_MODEL=openai/gpt-4o-mini
export OPENROUTER_HANDLER_MODEL=openai/gpt-4o-mini
```

Optional fixed gateway port:

```bash
export MOCK_GATEWAY_PORT=23001
```

If set, `mock_studio` will ask the spawned `mock_gateway` to bind that exact localhost port instead of auto-allocating one.

When `--public-base-url` is not provided, `mock_studio` will best-effort detect the current machine IP and advertise `http://<detected-ip>:<port>` in receipts and ticket responses. If detection fails, it falls back to `127.0.0.1`.

## Manual Run

Start studio:

```bash
cargo run -p raiko-mock-studio -- --bind 0.0.0.0:9090
```

Start studio with an explicit advertised public URL:

```bash
cargo run -p raiko-mock-studio -- \
  --bind 0.0.0.0:9090 \
  --public-base-url https://mock.example.com
```

The browser UI is served from `/` on the studio process itself. The UI bootstraps from `GET /api/ui/state`, submits tickets to `POST /api/tickets`, and sends gateway requests through `POST /api/tickets/:ticket_id/gateway`.
The gateway target field in the browser is editable. If `PUBLIC_BASE_URL` is set in the studio environment, the UI uses it as the default target before falling back to ticket data.

Submit a ticket:

```bash
curl -s http://127.0.0.1:9090/api/tickets \
  -H 'content-type: application/json' \
  -d '{
    "requirement": "Generate a mock for /v3/proof/batch/shasta: return registered for the first 3 calls, then return error on the 4th call."
  }'
```

Query a ticket:

```bash
curl -s http://127.0.0.1:9090/api/tickets/ticket-1
```

## Demo Script

Use:

```bash
script/run-mock-studio-demo.sh
```

Optional custom requirement:

```bash
script/run-mock-studio-demo.sh "Generate a mock that returns error on the 2nd call."
```

Optional advertised public URL for the demo script:

```bash
PUBLIC_BASE_URL=https://mock.example.com script/run-mock-studio-demo.sh
```

The script:

- starts `raiko-mock-studio`
- fails fast if `STUDIO_ADDR` is already occupied by another process
- waits for the studio root page
- submits one ticket
- prints the receipt
- prints the generated rule directory
- shows a sample curl for the resulting mock gateway

## Submit To Running Studio

If `mock_studio` is already running and you only want to submit another requirement, use:

```bash
script/submit-mock-ticket.sh "Generate a mock that returns error on the 2nd call."
```

Optional address override:

```bash
STUDIO_ADDR=127.0.0.1:4011 script/submit-mock-ticket.sh "Generate a mock that returns error on the 2nd call."
```

This script does not start or stop `mock_studio`. It only talks to the existing HTTP API.

## Gateway Bind

`raiko-mock-gateway` accepts an explicit bind flag:

```bash
cargo run -p raiko-mock-gateway -- --bind 0.0.0.0:4000
```

If no bind argument is provided, it defaults to `0.0.0.0:4000`.

## Generated Files

Each successful ticket writes into:

```text
mock-gateway/generated/<rule_id>/
```

Key files:

- `conversation.md`: original requirement text
- `meta.json`: lightweight metadata
- `spec.json`: parsed intermediate spec
- `ticket.rs`: restricted handler source compiled into `mock-gateway`
- `llm/spec_prompt.md`: first OpenRouter prompt
- `llm/spec_response.json`: first OpenRouter raw response
- `llm/handler_prompt.md`: second OpenRouter prompt
- `llm/handler_response.json`: second OpenRouter raw response
- `build.log`: output from `cargo build -p raiko-mock-gateway`
- `runtime.log`: stdout/stderr from the running gateway process
- `receipt.json`: final run result

The memory index is:

```text
mock-gateway/generated/index.json
```

## Restricted Runtime Contract

Generated handlers do not get arbitrary access to the server. They compile against a fixed contract:

- signature:
  - `pub fn handle_shasta_request(ctx: &MockContext, body: &Value) -> Value`
- allowed helpers:
  - `error_status(error, message)`
  - `ok_status(proof_type(body), proposal_batch_id(body), task_status)`
  - `mock_proof_response(body, label)`
  - `mock_proof_response_with_type(body, label, Some("fixed-type"))`
- allowed memory/context methods:
  - `ctx.call_index()`
  - `ctx.request_key(body)`
  - `ctx.has_seen_request(body)`
  - `ctx.mark_request_seen(body)`

Unknown fields in planner JSON are ignored. The planner may only request a fixed `proof_type_override` for proof-shaped success responses. Proof payload bytes stay runtime-generated, and the helper validates that the emitted proof string is valid hex.

## Debug Order

When a ticket fails, inspect in this order:

1. `receipt.json`
2. `llm/spec_response.json`
3. `llm/handler_response.json`
4. `build.log`
5. `runtime.log`

That usually tells you which stage failed:

- bad spec
- bad handler
- build failure
- runtime startup failure
- health timeout

## Common Failure Modes

### Spec parse failure

Symptom:
- ticket status becomes `failed`
- `receipt.json` contains a planner error

Check:
- `llm/spec_prompt.md`
- `llm/spec_response.json`

Cause:
- model returned prose instead of JSON
- wrong field names
- legacy `index.json` entries missing newly added fields

### Handler validation failure

Symptom:
- ticket status becomes `failed`
- error mentions restricted handler generation

Check:
- `llm/handler_response.json`

Cause:
- generated source is missing `pub fn handle_shasta_request`
- forbidden tokens such as `fn main`, `Router`, or `mod `
- generated source does not import or call the allowed helper surface correctly

### Build failure

Symptom:
- `build.log` contains Rust compile errors

Cause:
- generated handler does not match the fixed helper surface

### Health timeout

Symptom:
- `runtime.log` ends with a health timeout note

Cause:
- binary started but failed before binding
- local environment blocked port binding
- startup panic inside generated code

### Demo script exits before startup

Symptom:
- `script/run-mock-studio-demo.sh` prints `mock_studio address already in use`

Cause:
- another process is already listening on `STUDIO_ADDR`
- an old `raiko-mock-studio` instance was left running

## Tests

Default unit/integration tests:

```bash
cargo test -p raiko-mock-studio
cargo test -p raiko-mock-gateway
```

There is one ignored real integration test for the full backend loop:

```bash
cargo test -p raiko-mock-studio \
  local_runner_builds_starts_and_writes_receipt \
  -- --exact --ignored --nocapture
```

It is ignored by default because sandboxed environments may block local port binding for spawned processes.

## Where To Extend Next

If you want to evolve the system later, the clean extension points are:

- `mock-studio/src/openrouter.rs`
  - improve prompts
  - add response repair
- `mock-studio/src/runner.rs`
  - better process supervision
  - cleanup and pid tracking
- `mock-studio/src/lib.rs`
  - add more ticket states
  - add retries or auto-fix loop
- `mock-gateway/src/lib.rs`
  - extend helper surface for restricted handlers
