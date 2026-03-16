# Mock Studio Shasta Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a ticket-based `mock_studio` plus a compile-time generated `mock_gateway` for `/v3/proof/batch/shasta` without changing the existing client-facing request or response shape.

**Architecture:** Add two workspace crates. `mock_gateway` owns the runtime endpoint and compiles one generated restricted ticket handler from `generated/<rule_id>/ticket.rs`. `mock_studio` owns ticket intake, a two-step OpenRouter flow (`requirement -> spec`, `spec -> handler`), artifact persistence, and gateway launch orchestration.

**Tech Stack:** Rust, Axum, Tokio, Reqwest, Serde, Clap, workspace crates from `host`

---

### Task 1: Scaffold `mock_gateway`

**Files:**
- Modify: `/home/yue/tmp/raiko/Cargo.toml`
- Create: `/home/yue/tmp/raiko/mock-gateway/Cargo.toml`
- Create: `/home/yue/tmp/raiko/mock-gateway/build.rs`
- Create: `/home/yue/tmp/raiko/mock-gateway/src/lib.rs`
- Create: `/home/yue/tmp/raiko/mock-gateway/src/main.rs`
- Create: `/home/yue/tmp/raiko/mock-gateway/src/router.rs`
- Create: `/home/yue/tmp/raiko/mock-gateway/src/generated.rs`
- Create: `/home/yue/tmp/raiko/mock-gateway/src/state.rs`
- Create: `/home/yue/tmp/raiko/mock-gateway/generated/example-fourth-call-error/ticket.rs`
- Create: `/home/yue/tmp/raiko/mock-gateway/generated/example-fourth-call-error/meta.json`
- Create: `/home/yue/tmp/raiko/mock-gateway/generated/example-fourth-call-error/conversation.md`
- Create: `/home/yue/tmp/raiko/mock-gateway/generated/index.json`
- Test: `/home/yue/tmp/raiko/mock-gateway/tests/mock_gateway_test.rs`

**Step 1: Write the failing test**

Write an integration test that starts `mock_gateway`, posts four Shasta requests to `/v3/proof/batch/shasta`, and expects the fourth response to be `status=error` while `/health` stays available.

**Step 2: Run test to verify it fails**

Run: `cargo test -p mock-gateway mock_gateway_returns_configured_error_on_fourth_call -- --exact`

Expected: FAIL because `mock-gateway` crate and route do not exist.

**Step 3: Write minimal implementation**

- Add `mock-gateway` to the workspace.
- Implement a minimal Axum app with `/health` and `/v3/proof/batch/shasta`.
- Add build-time selection of one generated `ticket.rs`.
- Provide one example generated rule that returns `ok/status` for calls 1-3 and `error` on call 4.

**Step 4: Run test to verify it passes**

Run: `cargo test -p mock-gateway mock_gateway_returns_configured_error_on_fourth_call -- --exact`

Expected: PASS

### Task 2: Scaffold `mock_studio`

**Files:**
- Modify: `/home/yue/tmp/raiko/Cargo.toml`
- Create: `/home/yue/tmp/raiko/mock-studio/Cargo.toml`
- Create: `/home/yue/tmp/raiko/mock-studio/src/lib.rs`
- Create: `/home/yue/tmp/raiko/mock-studio/src/main.rs`
- Create: `/home/yue/tmp/raiko/mock-studio/src/api.rs`
- Create: `/home/yue/tmp/raiko/mock-studio/src/app.rs`
- Create: `/home/yue/tmp/raiko/mock-studio/src/models.rs`
- Create: `/home/yue/tmp/raiko/mock-studio/src/openrouter.rs`
- Create: `/home/yue/tmp/raiko/mock-studio/src/spec.rs`
- Create: `/home/yue/tmp/raiko/mock-studio/src/generator.rs`
- Create: `/home/yue/tmp/raiko/mock-studio/src/runner.rs`
- Create: `/home/yue/tmp/raiko/mock-studio/src/store.rs`
- Test: `/home/yue/tmp/raiko/mock-studio/tests/ticket_flow_test.rs`

**Step 1: Write the failing test**

Write a test that submits a ticket into the app with a fake planner and fake runner, then verifies the ticket transitions from `pending` to `running` and returns a receipt with `rule_id` and `base_url`.

**Step 2: Run test to verify it fails**

Run: `cargo test -p mock-studio ticket_submission_returns_running_receipt -- --exact`

Expected: FAIL because `mock-studio` does not exist.

**Step 3: Write minimal implementation**

- Implement in-memory ticket storage.
- Implement a constrained mock spec model.
- Implement a fakeable planner interface plus an OpenRouter-backed planner.
- Implement a generator that writes `conversation.md`, `meta.json`, `spec.json`, and `ticket.rs`.
- Implement a runner abstraction that can spawn `mock_gateway` for a generated `rule_id`.
- Expose a minimal HTML submission page and JSON endpoints for ticket creation/status.

**Step 4: Run test to verify it passes**

Run: `cargo test -p mock-studio ticket_submission_returns_running_receipt -- --exact`

Expected: PASS

### Task 3: Wire generated artifacts and regression coverage

**Files:**
- Modify: `/home/yue/tmp/raiko/mock-gateway/build.rs`
- Modify: `/home/yue/tmp/raiko/mock-studio/src/generator.rs`
- Modify: `/home/yue/tmp/raiko/mock-studio/src/runner.rs`
- Modify: `/home/yue/tmp/raiko/mock-studio/src/store.rs`
- Test: `/home/yue/tmp/raiko/mock-studio/tests/generated_artifacts_test.rs`
- Test: `/home/yue/tmp/raiko/mock-gateway/tests/mock_gateway_test.rs`

**Step 1: Write the failing test**

Write a test that generates a rule directory and asserts:

- `mock-gateway/generated/<rule_id>/conversation.md` exists
- `mock-gateway/generated/<rule_id>/meta.json` exists
- `mock-gateway/generated/<rule_id>/spec.json` exists
- `mock-gateway/generated/index.json` is updated

**Step 2: Run test to verify it fails**

Run: `cargo test -p mock-studio generated_rule_updates_memory_index -- --exact`

Expected: FAIL because the artifact index update is incomplete.

**Step 3: Write minimal implementation**

- Complete artifact persistence and index updates.
- Ensure the runner uses the generated `rule_id`.
- Keep gateway compilation selection deterministic.

**Step 4: Run test to verify it passes**

Run: `cargo test -p mock-studio generated_rule_updates_memory_index -- --exact`

Expected: PASS

### Task 4: Verify end-to-end behavior

**Files:**
- Test: `/home/yue/tmp/raiko/mock-studio/tests/ticket_flow_test.rs`
- Test: `/home/yue/tmp/raiko/mock-gateway/tests/mock_gateway_test.rs`

**Step 1: Run focused tests**

Run:

```bash
cargo test -p mock-gateway
cargo test -p mock-studio
```

Expected: PASS

**Step 2: Run formatting**

Run:

```bash
cargo fmt --all
```

Expected: no diffs after formatting
