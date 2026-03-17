# Gateway Public Bind Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make mock gateway default to public bind, support explicit CLI bind parameters, and let mock studio advertise either an explicit public base URL or an auto-detected machine IP.

**Architecture:** Move gateway bind parsing into testable helpers, have mock studio compute both a bind address and an advertised base URL, and keep process launching fully CLI-driven. Avoid new environment variables except the existing fixed-port override.

**Tech Stack:** Rust, Tokio, Axum, anyhow, reqwest, existing cargo tests

---

### Task 1: Document the intended behavior in tests

**Files:**
- Modify: `mock-gateway/tests/mock_gateway_test.rs`
- Modify: `mock-studio/tests/ticket_flow_test.rs`
- Modify: `mock-studio/src/runner.rs`

**Step 1: Write the failing tests**

- Add a gateway test covering default bind parsing and explicit `--bind`.
- Add runner tests covering:
  - default bind host `0.0.0.0`
  - configured port still using public bind host
  - advertised URL override
  - auto-detected host formatting

**Step 2: Run tests to verify they fail**

Run: `cargo test -p raiko-mock-gateway -p raiko-mock-studio`

Expected: failures around old `127.0.0.1` defaults and missing CLI parsing helpers.

### Task 2: Implement minimal gateway CLI parsing

**Files:**
- Modify: `mock-gateway/src/lib.rs`
- Modify: `mock-gateway/src/main.rs`

**Step 1: Write minimal implementation**

- Add a helper that parses `--bind <host:port>` from argv.
- Default to `0.0.0.0:4000`.
- Wire `main.rs` to use that helper.

**Step 2: Run tests to verify they pass**

Run: `cargo test -p raiko-mock-gateway`

Expected: gateway tests pass.

### Task 3: Implement runner bind and public URL handling

**Files:**
- Modify: `mock-studio/src/runner.rs`
- Modify: `mock-studio/src/main.rs`
- Modify: `mock-studio/tests/ticket_flow_test.rs`

**Step 1: Write minimal implementation**

- Change bind generation to use `0.0.0.0`.
- Launch gateway with `--bind <host:port>`.
- Add an advertised base URL resolver:
  - explicit CLI `--public-base-url`
  - otherwise best-effort local IP detection
  - otherwise fallback to bind host

**Step 2: Run tests to verify they pass**

Run: `cargo test -p raiko-mock-studio`

Expected: runner and ticket-flow tests pass with updated expectations.

### Task 4: Update docs and run final verification

**Files:**
- Modify: `docs/DEBUG_mock_studio.md`

**Step 1: Update docs**

- Document the new gateway CLI and studio public URL override.

**Step 2: Run final verification**

Run: `cargo test -p raiko-mock-gateway -p raiko-mock-studio`

Expected: all non-ignored tests pass.
