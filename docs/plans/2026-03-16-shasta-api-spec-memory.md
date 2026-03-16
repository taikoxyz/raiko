# Shasta API Spec And Memory Contract Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a static `Shasta API Spec` and a fixed `Memory API` contract so `mock_studio` can generate more capable `/v3/proof/batch/shasta` handlers, including first-register-then-proof and aggregation-error behavior.

**Architecture:** Extend `mock_studio` with a curated `ShastaApiSpec` that is injected into the second LLM prompt, and extend `mock_gateway` with a restricted `MockContext` runtime that exposes a small request-memory surface to generated handlers. Keep all orchestration deterministic and local.

**Tech Stack:** Rust, Axum, Tokio, Serde, Reqwest, Bash

---

### Task 1: Add failing tests for the new gateway runtime contract

**Files:**
- Modify: `/home/yue/tmp/raiko/mock-gateway/src/state.rs`
- Modify: `/home/yue/tmp/raiko/mock-gateway/tests/mock_gateway_test.rs`

**Step 1: Write the failing tests**

Add tests that verify:
- a context can mark a request as seen and later detect it
- a proof helper returns an `ok` status envelope with `data.proof`

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p raiko-mock-gateway
```

Expected: FAIL because the context memory API and proof helper do not exist.

### Task 2: Implement the memory contract in `mock_gateway`

**Files:**
- Modify: `/home/yue/tmp/raiko/mock-gateway/src/state.rs`
- Modify: `/home/yue/tmp/raiko/mock-gateway/src/router.rs`
- Modify: `/home/yue/tmp/raiko/mock-gateway/src/lib.rs`
- Modify: `/home/yue/tmp/raiko/mock-gateway/src/generated.rs`
- Modify: `/home/yue/tmp/raiko/mock-gateway/generated/example-fourth-call-error/ticket.rs`

**Step 1: Write minimal implementation**

- Add `MockContext`
- Add request-memory storage to `AppState`
- Change generated handler signature to take `&MockContext`
- Add a stable proof-shaped success helper
- Keep the existing example rule working on the new contract

**Step 2: Run test to verify it passes**

Run:

```bash
cargo test -p raiko-mock-gateway
```

Expected: PASS

### Task 3: Add failing tests for `ShastaApiSpec` prompt context

**Files:**
- Create: `/home/yue/tmp/raiko/mock-studio/src/api_spec.rs`
- Modify: `/home/yue/tmp/raiko/mock-studio/src/openrouter.rs`

**Step 1: Write the failing tests**

Add tests that verify:
- `ShastaApiSpec` contains aggregation semantics and memory contract notes
- the second-stage handler prompt includes serialized API spec context

**Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p raiko-mock-studio
```

Expected: FAIL because no API spec module or prompt integration exists.

### Task 4: Implement `ShastaApiSpec` and integrate it into code generation

**Files:**
- Create: `/home/yue/tmp/raiko/mock-studio/src/api_spec.rs`
- Modify: `/home/yue/tmp/raiko/mock-studio/src/models.rs`
- Modify: `/home/yue/tmp/raiko/mock-studio/src/lib.rs`
- Modify: `/home/yue/tmp/raiko/mock-studio/src/openrouter.rs`
- Modify: `/home/yue/tmp/raiko/mock-studio/tests/ticket_flow_test.rs`

**Step 1: Write minimal implementation**

- Add a curated `ShastaApiSpec`
- Extend `MockSpec` with optional fields that describe memory-sensitive behavior
- Pass both `Task Spec` and `ShastaApiSpec` into the second LLM step
- Tighten handler validation to the new `MockContext` contract
- Keep fake planner and fake handler generator compatible

**Step 2: Run test to verify it passes**

Run:

```bash
cargo test -p raiko-mock-studio
```

Expected: PASS

### Task 5: Update docs and verify the whole workspace slice

**Files:**
- Modify: `/home/yue/tmp/raiko/docs/DEBUG_mock_studio.md`

**Step 1: Update docs**

- Document the new `ShastaApiSpec`
- Document the fixed memory contract available to restricted handlers

**Step 2: Run verification commands**

Run:

```bash
cargo fmt --all
cargo test -p raiko-mock-studio
cargo test -p raiko-mock-gateway
```

Expected: PASS
