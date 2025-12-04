# Repository Guidelines

## Project Structure & Module Organization
The runtime lives in `host/`, while reusable crates sit in `crates/` (notably `driver`, `primitives`, `stateless`, `provider`, `processor`, and `prover`). Prover-specific assets reside in `provers/` with `builder/`, `driver/`, and `guest/` subtrees for each backend (`sp1/`, `risc0/`, `sgx/`). Shared logic and primitives appear in `core/` and `lib/`; automation scripts live in `script/`, and auxiliary binaries are under `bin/`. Request handling and storage components are split across `reqpool/`, `reqactor/`, `taskdb/`, and `ballot/`. Docs and additional guides live in `docs/`. Keep new modules scoped to the appropriate crate to avoid cross-crate cycles.

## Build, Test, and Development Commands
- `make install` (or `TARGET=sp1 make install`): install toolchains and dependencies.
- `make build` / `TARGET=risc0 make build`: compile host and selected prover targets.
- `cargo run` or `make run`: start the local host; follow with `./script/prove-block.sh taiko_a7 native 10` for sample proofs.
- `make test`, `TARGET=<backend> make test`: execute unit suites; `make integration` for end-to-end coverage.
- `make fmt` and `make clippy`: enforce formatting and lint standards before sending patches.

## Coding Style & Naming Conventions
The workspace targets Rust 2024 with four-space indentation. Module and crate names use `snake_case`; types and traits use `UpperCamelCase`. Run `cargo fmt --all` to format and `cargo clippy -D warnings` to catch regressions. Favor focused crates and avoid leaking backend-specific code into shared layers.

## Testing Guidelines
Place unit tests beside the implementation with `#[cfg(test)]`. Use deterministic data; prefer `rstest` or `proptest` when variation is needed. Name tests after the behavior they assert (`fn verifies_signature_with_valid_key`). Run backend-specific suites via `TARGET=<sp1|risc0|sgx> make test` and ensure integration coverage with `make integration` before merges.

## Commit & Pull Request Guidelines
Follow Conventional Commits (`feat:`, `fix:`, `chore:`). Each PR should link relevant issues (e.g., `#123`), describe changes, list build/test steps (`make fmt clippy test`), and note doc or metrics updates. Include screenshots or logs when touching developer tooling or dashboards.

## Security & Configuration Tips
Store secrets in `.env`; local overrides live outside version control. Use performance toggles such as `CPU_OPT=1`, `MOCK=1`, `RISC0_DEV_MODE=1`, and `SP1_PROVER=mock` when profiling or running prover hosts. Rebuild after changing SGX or prover configs to refresh generated artifacts.
