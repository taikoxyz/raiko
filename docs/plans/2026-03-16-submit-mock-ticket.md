# Submit Mock Ticket Script Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a standalone script that submits a ticket to an already running `mock_studio` instance and prints the resulting receipt.

**Architecture:** Introduce `script/submit-mock-ticket.sh` as a pure client-side helper. It will POST one requirement to the existing studio API, parse the response, fetch the saved receipt, and print the generated rule directory path without starting any local services.

**Tech Stack:** Bash, curl, python3

---

### Task 1: Add a failing shell regression test

**Files:**
- Create: `/home/yue/tmp/raiko/script/test-submit-mock-ticket.sh`
- Test: `/home/yue/tmp/raiko/script/submit-mock-ticket.sh`

**Step 1: Write the failing test**

Create a shell test that:
- starts a tiny local Python HTTP server stub on `127.0.0.1:<port>`
- serves fixed JSON for `POST /api/tickets` and `GET /api/tickets/ticket-9`
- runs `script/submit-mock-ticket.sh "demo requirement"` against that address
- asserts the output contains `Ticket ID`, `Rule ID`, `Status`, `Base URL`, and the fetched receipt

**Step 2: Run test to verify it fails**

Run:

```bash
bash script/test-submit-mock-ticket.sh
```

Expected: FAIL because `script/submit-mock-ticket.sh` does not exist yet.

### Task 2: Implement the standalone submit script

**Files:**
- Create: `/home/yue/tmp/raiko/script/submit-mock-ticket.sh`
- Modify: `/home/yue/tmp/raiko/docs/DEBUG_mock_studio.md`

**Step 1: Write minimal implementation**

- Accept requirement as the first positional argument
- Use `STUDIO_ADDR` with default `127.0.0.1:4010`
- Call `POST /api/tickets`
- Parse `ticket_id`, `rule_id`, `status`, `base_url`, and `error`
- Call `GET /api/tickets/:ticket_id`
- Print the generated rule directory path under `mock-gateway/generated/<rule_id>`

**Step 2: Run test to verify it passes**

Run:

```bash
bash script/test-submit-mock-ticket.sh
bash -n script/submit-mock-ticket.sh
```

Expected: PASS

### Task 3: Run project verification

**Files:**
- Verify only

**Step 1: Run verification commands**

Run:

```bash
cargo test -p raiko-mock-studio
cargo test -p raiko-mock-gateway
```

Expected: PASS
