# Mock Studio Shasta Design

**Date:** 2026-03-14

## Goal

Add an internal ticket-style `mock_studio` that can accept a single mock request, ask an LLM to first reduce it into a constrained spec and then generate restricted Rust code for `/v3/proof/batch/shasta`, build and run a dedicated `mock_gateway`, and return a receipt containing the running base URL. The request and response shapes exposed by the gateway must stay compatible with the long-lived client integration.

## Scope

- Target one API only: `/v3/proof/batch/shasta`
- Preserve existing input and output JSON shapes
- Generate Rust source code per mock rule directory
- Keep a simple memory index of generated rules and ticket outcomes
- Use a ticket workflow, not a chat workflow

## Non-Goals

- No generic mock platform
- No multi-round chat UI
- No runtime DSL interpreter
- No support for other Raiko APIs in the first version
- No free-form model-authored gateway crate

## Architecture

### Mock Gateway

`mock_gateway` is a dedicated crate with a small Axum server. It exposes:

- `GET /health`
- `POST /v3/proof/batch/shasta`

The server keeps the existing request and response contract. It does not know about OpenRouter or ticket orchestration. Its ticket handler is selected at build time from `mock_gateway/generated/<rule_id>/ticket.rs`.

### Mock Studio

`mock_studio` is a separate crate that exposes a small internal web UI and JSON API for tickets. A ticket is a one-shot request such as "return error on the 4th call". The studio:

1. creates a ticket
2. sends the requirement to OpenRouter
3. validates the model output into a constrained mock spec
4. asks the model for a restricted Rust handler module
5. validates and stores `conversation.md`, `meta.json`, `spec.json`, and `ticket.rs`
6. builds and launches `mock_gateway`
7. returns the receipt and updates ticket status

### Generated Artifacts

Each generated mock lives under `mock_gateway/generated/<rule_id>/`:

- `conversation.md`
- `meta.json`
- `spec.json`
- `ticket.rs`

`mock_gateway/generated/index.json` stores lightweight memory entries such as `ticket_id`, `rule_id`, summary, status, and port.

## Data Flow

1. User submits a ticket in `mock_studio`.
2. Studio writes the ticket as `pending`.
3. OpenRouter returns a constrained spec.
4. Studio asks the model to generate a restricted handler and writes generated files for the rule directory.
5. Studio invokes a local build/run step for `mock_gateway` with that `rule_id`.
6. `mock_gateway` starts on a dedicated local port.
7. Studio marks the ticket `running` and returns the base URL.

## Spec Shape

The first model call targets a small schema. First-version fields:

- `summary`
- `default_response.kind`
- `default_response.task_status`
- `nth_responses[].n`
- `nth_responses[].kind`
- `nth_responses[].error`
- `nth_responses[].message`
- optional `proposal_id_match`

The second model call converts the validated spec into a restricted Rust handler module.

## Error Handling

- Ticket failures stay in `mock_studio`; they do not change gateway I/O.
- Gateway mock failures should prefer the existing JSON `Status::Error` shape over inventing a new schema.
- Invalid request parsing in the gateway should keep returning Axum/host-compatible error JSON.

## Testing

1. Unit test the generated Shasta handler behavior: first calls return `ok`, the configured nth call returns `error`.
2. Integration test `mock_gateway` on `/v3/proof/batch/shasta` and `/health`.
3. Unit test spec validation and restricted handler validation in `mock_studio`.
4. Unit test ticket state transitions with a fake planner and fake gateway runner.

## Implementation Notes

- Reuse existing `host` request/response types where possible instead of copying JSON contracts.
- Reuse patterns from the existing `gateway` crate where it reduces boilerplate, but keep mock responsibilities isolated.
- Keep the first UI intentionally small: a single ticket submission form and a ticket status view.
