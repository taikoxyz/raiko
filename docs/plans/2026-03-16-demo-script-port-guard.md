# Demo Script Port Guard Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make `script/run-mock-studio-demo.sh` fail fast when the target studio address is already occupied by another process.

**Architecture:** Add a small preflight port-bind check to the demo script before spawning `raiko-mock-studio`. Cover the behavior with a shell-level regression test that occupies the target port and asserts the script exits before it starts submitting tickets.

**Tech Stack:** Bash, Python 3, existing demo script

---

### Task 1: Add failing regression test for occupied studio port

**Files:**
- Create: `/home/yue/tmp/raiko/script/test-run-mock-studio-demo.sh`
- Test: `/home/yue/tmp/raiko/script/run-mock-studio-demo.sh`

**Step 1: Write the failing test**

Create a shell test that:
- starts a temporary `python3 -m http.server` on `127.0.0.1:<port>`
- runs `script/run-mock-studio-demo.sh` with `STUDIO_ADDR` set to that port
- expects a non-zero exit and an error message explaining the address is already in use
- fails if the script reaches the `Submitting ticket` phase

**Step 2: Run test to verify it fails**

Run:

```bash
bash script/test-run-mock-studio-demo.sh
```

Expected: FAIL because the current script only notices the conflict after trying to start and then talking to the wrong service.

### Task 2: Add fast-fail occupied-port check

**Files:**
- Modify: `/home/yue/tmp/raiko/script/run-mock-studio-demo.sh`
- Modify: `/home/yue/tmp/raiko/docs/DEBUG_mock_studio.md`

**Step 1: Write minimal implementation**

- Before spawning `cargo run -p raiko-mock-studio`, use a small `python3` socket bind probe against `STUDIO_ADDR`
- If bind fails, print a clear error and exit non-zero
- Keep the rest of the script unchanged
- Document the behavior in the debug README

**Step 2: Run test to verify it passes**

Run:

```bash
bash script/test-run-mock-studio-demo.sh
bash -n script/run-mock-studio-demo.sh
```

Expected: PASS
