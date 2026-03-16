# Shasta API Spec And Memory Contract Design

**Date:** 2026-03-16

## Goal

Improve `mock_studio` so the second LLM call can generate more capable `/v3/proof/batch/shasta` handlers by giving it a curated `Shasta API Spec` plus a fixed `Memory API` contract. The immediate target is tickets like:

- normal request first returns `registered`
- same normal request later returns a mock proof
- aggregation request returns `error`

## Why Change

The current second-stage code generation mostly sees `Task Spec` plus a helper signature. That is enough for simple nth-call mocks, but not for behavior that depends on request shape, aggregation semantics, or remembered request state.

## Design

### Two Specs

`mock_studio` should keep two separate specs:

1. `Task Spec`
- produced by the first LLM call
- captures the requested behavior

2. `API Spec`
- produced locally by `studio`
- captures the stable contract of `/v3/proof/batch/shasta`
- provides exactly the context the second LLM needs to write a correct restricted handler

### Static Shasta API Spec

The first version should use a static curated `ShastaApiSpec`, not dynamic repo retrieval. The spec should include:

- route path
- key request fields such as `aggregate`, `proof_type`, and `proposals[].proposal_id`
- response envelope rules
- aggregation semantics summary
- helper contract summary
- memory contract summary
- one or two reference snippets

This keeps token usage predictable and makes debugging prompt quality much easier.

### Fixed Memory Contract

Whether a ticket uses memory is up to the LLM. The shape of memory is not.

The first version should provide a fixed body-centric memory API through a `MockContext` runtime object:

- `call_index()`
- `request_key(body)`
- `has_seen_request(body)`
- `mark_request_seen(body)`

This is enough to express "first request registers, second request returns proof" without exposing arbitrary storage design to the LLM.

### Restricted Handler Contract

Change the generated handler signature from:

```rust
pub fn handle_shasta_request(call_index: u64, body: &Value) -> Value
```

to:

```rust
pub fn handle_shasta_request(ctx: &MockContext, body: &Value) -> Value
```

This lets the generated handler access fixed runtime state while staying constrained.

### Mock Proof Helper

Add a local helper that returns a proof-shaped success payload compatible with the existing status envelope. The generated handler should not invent proof JSON from scratch if the system can provide a stable helper.

## Testing

1. `mock_gateway` unit tests for the new memory contract
2. `mock_gateway` tests for the mock proof response helper shape
3. `mock_studio` tests for the curated `ShastaApiSpec`
4. `mock_studio` tests that handler prompts include API spec and memory contract context
