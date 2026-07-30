# Contributing to compliance-primitives

Thanks for your interest in contributing. This repo is part of the **Drips
Wave Stellar Program**, and issues are labeled by complexity
(`complexity: trivial`, `complexity: medium`, `complexity: high`) so you can
pick something that matches how deep you want to go. Issues good for a first
contribution are also tagged `good first issue`.

## Workflow

1. **Fork** the repository and clone your fork.
2. **Branch** off `main` with a descriptive name, e.g. `add-jurisdiction-remove-fn`.
3. **Make your change**, keeping it scoped to the issue you're addressing.
   Each contract crate is meant to stay small, single-responsibility, and
   under ~300 lines — if a change grows a contract past that, consider
   whether it belongs in a new crate or `/examples` instead.
4. **Add tests.** Every public function needs coverage for its happy path
   and at least one failure/auth case. New functionality without tests
   won't be merged.
5. **Before submitting a PR, run:**
   ```sh
   make test
   make lint
   ```
   (equivalent to `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
   and `cargo fmt --all -- --check`). Both must pass locally — the same test and clippy checks run
   in CI on every PR. `make build` runs the same wasm build CI checks. See the
   [Makefile](./Makefile) for the full list of targets.
6. **Open a pull request** against `main`, referencing the issue it closes
   (e.g. `Closes #12`). Describe what changed and why.

## Picking up an issue

Comment on the issue to let others know you're working on it. If you go
quiet for a while, don't worry — someone else may pick it up, and you're
welcome to jump back in on something else.

## Code style

- `#![no_std]` throughout; no `std`-only dependencies in contract crates.
- Public functions should have doc comments explaining behavior, especially
  auth requirements and failure conditions.
- Prefer returning `Result<T, Error>` with a `#[contracterror]` enum over
  panicking, except where `require_auth()`'s own panic behavior is the
  expected failure mode.
- Keep events auditable: if a function's outcome should be visible off-chain
  (e.g. a blocked transfer), don't put it behind a code path that would
  cause the whole invocation to revert — Soroban rolls back events emitted
  during any invocation that ultimately fails.

## Questions

Open an issue with your question, or comment on the relevant existing issue.
