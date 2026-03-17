# Gateway Public Bind Design

## Goal

Make `raiko-mock-gateway` listen on `0.0.0.0` by default, add explicit CLI bind parameters, and let `raiko-mock-studio` return a useful public base URL without relying on extra environment variables.

## Decision

Use explicit CLI parameters as the single source of truth for gateway bind configuration:

- `raiko-mock-gateway` accepts `--bind <host:port>`
- default bind becomes `0.0.0.0:4000`
- `raiko-mock-studio` always spawns the gateway with an explicit `--bind`

For advertised URLs:

- `raiko-mock-studio` accepts an optional `--public-base-url <url>`
- if absent, it best-effort detects the current machine IP and returns `http://<detected-ip>:<port>`
- if detection fails, it falls back to the bind host/port

## Why

This removes the current split-brain behavior where the gateway bind host is effectively hardcoded in code paths while the port is optionally controlled by environment variables. CLI arguments are clearer, easier to reason about, and easier to extend safely.

Automatic public URL detection is helpful for simple single-host development and demos, but it is not authoritative in NAT, container, or load-balanced deployments. The explicit `--public-base-url` override handles those cases without forcing an environment-variable-based configuration path.

## Scope

Files expected to change:

- `mock-gateway/src/main.rs`
- `mock-gateway/src/lib.rs`
- `mock-gateway/tests/mock_gateway_test.rs`
- `mock-studio/src/main.rs`
- `mock-studio/src/runner.rs`
- `mock-studio/tests/ticket_flow_test.rs`
- `docs/DEBUG_mock_studio.md`

## Testing

- add unit tests for gateway CLI bind parsing
- add unit tests for runner bind/default behavior
- add unit tests for advertised base URL selection
- run `cargo test -p raiko-mock-gateway -p raiko-mock-studio`
