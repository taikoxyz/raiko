# Mock Studio Agent Platform Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Evolve `mock_studio` from a prompt-engineered two-step code generation pipeline into a more structured agent platform with first-class skills, explicit tools, and a clean path to MCP integration.

**Architecture:** Keep the existing ticket, generator, and runner flow intact while introducing three new layers: a skill catalog for reusable prompt modules, a tool/action layer for structured local capabilities, and an adapter boundary for future MCP exposure. The migration should be incremental: first make the current OpenRouter path more modular, then add structured tool execution, then optionally expose those tools over MCP.

**Tech Stack:** Rust, Tokio, Axum, anyhow, serde/serde_json, reqwest, cargo tests, markdown docs

---

### Task 1: Write the target architecture down before changing code

**Files:**
- Create: `docs/plans/2026-03-17-mock-studio-agent-platform-design.md`
- Modify: `docs/DEBUG_mock_studio.md`

**Step 1: Write the design doc**

Document:
- what the current system is
- what "skill", "tool", and "MCP-ready" mean in this repo
- which boundaries stay stable: ticket API, generated rule layout, gateway runtime contract
- which boundaries change: prompt assembly, planner/generator orchestration, local execution model

Include a migration diagram with these stages:
- Stage 0: current prompt pipeline
- Stage 1: modular prompt/skill layer
- Stage 2: structured local tool calls
- Stage 3: optional MCP adapter

**Step 2: Review existing docs for terminology drift**

Read:
- `docs/DEBUG_mock_studio.md`
- `docs/plans/2026-03-14-mock-studio-shasta-design.md`
- `docs/plans/2026-03-16-shasta-api-spec-memory-design.md`

Expected outcome:
- the future design doc uses consistent terms and does not overload "skill" or "tool"

**Step 3: Commit the design doc**

Run:
```bash
git add docs/plans/2026-03-17-mock-studio-agent-platform-design.md docs/DEBUG_mock_studio.md
git commit -m "docs: add mock studio agent platform design"
```

### Task 2: Introduce first-class skill specifications for prompt modules

**Files:**
- Create: `mock-studio/src/skills.rs`
- Create: `mock-studio/tests/skills_test.rs`
- Modify: `mock-studio/src/openrouter.rs`
- Modify: `mock-studio/src/lib.rs`

**Step 1: Write the failing tests**

Add tests for:
- planner skill definitions exposing stable ids and versions
- handler generation skill definitions exposing stable ids and versions
- prompt rendering selecting the expected skill metadata

Example test shape:
```rust
#[test]
fn planner_skill_has_stable_id_and_version() {
    let skill = skill_catalog().planner();
    assert_eq!(skill.id, "shasta_mock_planner");
    assert_eq!(skill.version, 1);
}
```

**Step 2: Run tests to verify they fail**

Run:
```bash
cargo test -p raiko-mock-studio skills_test
```

Expected:
- compile failures or missing symbol failures because `skills.rs` does not exist yet

**Step 3: Write minimal implementation**

Create a small `SkillSpec` model that includes:
- `id`
- `version`
- `purpose`
- `system_prompt`
- `input_contract`
- `output_contract`

Replace hardcoded planner and handler system prompts in `openrouter.rs` with lookups from this catalog.

**Step 4: Run tests to verify they pass**

Run:
```bash
cargo test -p raiko-mock-studio skills_test
```

Expected:
- skill catalog tests pass

**Step 5: Commit**

Run:
```bash
git add mock-studio/src/skills.rs mock-studio/src/openrouter.rs mock-studio/src/lib.rs mock-studio/tests/skills_test.rs
git commit -m "refactor: add first-class mock studio skill specs"
```

### Task 3: Add a structured local tool/action layer without changing the external API

**Files:**
- Create: `mock-studio/src/tools.rs`
- Create: `mock-studio/tests/tools_test.rs`
- Modify: `mock-studio/src/runner.rs`
- Modify: `mock-studio/src/generator.rs`
- Modify: `mock-studio/src/lib.rs`

**Step 1: Write the failing tests**

Add tests for:
- a tool registry exposing build, launch, health-check, and artifact-write actions
- typed inputs/outputs for each action
- a ticket execution path that records which tools were invoked

Example test shape:
```rust
#[test]
fn tool_registry_contains_gateway_lifecycle_tools() {
    let registry = tool_registry();
    assert!(registry.get("build_gateway").is_some());
    assert!(registry.get("launch_gateway").is_some());
    assert!(registry.get("check_gateway_health").is_some());
}
```

**Step 2: Run tests to verify they fail**

Run:
```bash
cargo test -p raiko-mock-studio tools_test
```

Expected:
- missing module or missing registry failures

**Step 3: Write minimal implementation**

Add a local tool abstraction:
- `ToolSpec`
- `ToolInput`
- `ToolOutput`
- `ToolExecutor`

Refactor the runner path so it conceptually executes:
- `build_gateway`
- `launch_gateway`
- `check_gateway_health`
- `write_receipt_artifacts`

Keep actual behavior unchanged; this is a refactor toward explicit actions, not a product change.

**Step 4: Run tests to verify they pass**

Run:
```bash
cargo test -p raiko-mock-studio tools_test ticket_flow_test
```

Expected:
- tool tests pass
- ticket flow still passes

**Step 5: Commit**

Run:
```bash
git add mock-studio/src/tools.rs mock-studio/src/runner.rs mock-studio/src/generator.rs mock-studio/src/lib.rs mock-studio/tests/tools_test.rs
git commit -m "refactor: add structured mock studio tool layer"
```

### Task 4: Record generation and execution traces as durable structured artifacts

**Files:**
- Modify: `mock-studio/src/generator.rs`
- Modify: `mock-studio/src/models.rs`
- Modify: `mock-studio/tests/ticket_flow_test.rs`
- Create: `mock-studio/tests/trace_artifact_test.rs`

**Step 1: Write the failing tests**

Add tests asserting each ticket writes:
- `llm/trace.json`
- `execution/tools.json`

Each artifact should include:
- skill id/version used for each LLM call
- prompt file paths
- raw response file paths
- tool invocation names
- start/finish timestamps
- success/failure status

**Step 2: Run tests to verify they fail**

Run:
```bash
cargo test -p raiko-mock-studio trace_artifact_test ticket_flow_test
```

Expected:
- missing artifact assertions

**Step 3: Write minimal implementation**

Add lightweight trace models and write the new JSON artifacts into each rule directory.

Do not overbuild observability yet:
- no external tracing backend
- no streaming UI
- no retry engine

**Step 4: Run tests to verify they pass**

Run:
```bash
cargo test -p raiko-mock-studio trace_artifact_test ticket_flow_test
```

Expected:
- trace artifact tests pass

**Step 5: Commit**

Run:
```bash
git add mock-studio/src/generator.rs mock-studio/src/models.rs mock-studio/tests/ticket_flow_test.rs mock-studio/tests/trace_artifact_test.rs
git commit -m "feat: record mock studio skill and tool traces"
```

### Task 5: Add an MCP-ready adapter boundary, but do not build a full MCP server yet

**Files:**
- Create: `mock-studio/src/mcp.rs`
- Create: `mock-studio/tests/mcp_adapter_test.rs`
- Modify: `mock-studio/src/lib.rs`
- Modify: `docs/DEBUG_mock_studio.md`

**Step 1: Write the failing tests**

Add tests proving the adapter can expose existing local tools as MCP-like descriptors:
- tool name
- description
- JSON input schema
- JSON output schema

Example test shape:
```rust
#[test]
fn build_gateway_tool_can_be_exported_as_mcp_descriptor() {
    let descriptors = export_mcp_descriptors();
    assert!(descriptors.iter().any(|tool| tool.name == "build_gateway"));
}
```

**Step 2: Run tests to verify they fail**

Run:
```bash
cargo test -p raiko-mock-studio mcp_adapter_test
```

Expected:
- missing adapter module failures

**Step 3: Write minimal implementation**

Create a pure adapter layer that maps internal `ToolSpec` values into MCP-shaped descriptors.

Do not yet:
- open a transport server
- add authentication
- expose remote execution

This task is about preserving optionality, not shipping MCP itself.

**Step 4: Run tests to verify they pass**

Run:
```bash
cargo test -p raiko-mock-studio mcp_adapter_test tools_test
```

Expected:
- MCP adapter tests pass
- tool tests remain green

**Step 5: Commit**

Run:
```bash
git add mock-studio/src/mcp.rs mock-studio/src/lib.rs mock-studio/tests/mcp_adapter_test.rs docs/DEBUG_mock_studio.md
git commit -m "refactor: add MCP-ready tool descriptor adapter"
```

### Task 6: Tighten verification around model behavior before expanding scope

**Files:**
- Modify: `mock-studio/tests/ticket_flow_test.rs`
- Modify: `mock-studio/src/openrouter.rs`
- Create: `mock-studio/tests/regression_contract_test.rs`

**Step 1: Write the failing tests**

Add regression tests for:
- planner output schema drift
- handler source violating restricted contract
- missing trace metadata
- tool sequencing regressions

**Step 2: Run tests to verify they fail**

Run:
```bash
cargo test -p raiko-mock-studio regression_contract_test
```

Expected:
- failures proving the current suite does not fully lock these contracts down yet

**Step 3: Write minimal implementation**

Add the smallest validation and test helper changes necessary to lock the contracts:
- schema invariants
- skill version assertions
- ordered tool execution assertions

**Step 4: Run final verification**

Run:
```bash
cargo test -p raiko-mock-studio
cargo test -p raiko-mock-gateway
bash script/test-run-mock-studio-demo.sh
bash script/test-submit-mock-ticket.sh
```

Expected:
- all non-ignored Rust tests pass
- shell tests pass

**Step 5: Commit**

Run:
```bash
git add mock-studio/src/openrouter.rs mock-studio/tests/ticket_flow_test.rs mock-studio/tests/regression_contract_test.rs
git commit -m "test: lock mock studio agent platform contracts"
```

### Task 7: Decide whether to stop at MCP-ready or ship real MCP

**Files:**
- Modify: `docs/plans/2026-03-17-mock-studio-agent-platform-design.md`
- Modify: `docs/plans/2026-03-17-mock-studio-agent-platform.md`

**Step 1: Review the completed refactors**

Answer:
- are internal tools stable enough to expose remotely?
- do we need auth, tenancy, rate limiting, and audit logging first?
- is remote MCP actually needed, or is local structured tooling enough?

**Step 2: Record the decision**

Write a short follow-up section:
- `Stop at MCP-ready for now`
- or `Proceed to full MCP server`

**Step 3: Commit**

Run:
```bash
git add docs/plans/2026-03-17-mock-studio-agent-platform-design.md docs/plans/2026-03-17-mock-studio-agent-platform.md
git commit -m "docs: record MCP direction for mock studio"
```
