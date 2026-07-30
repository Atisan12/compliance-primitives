# Shared Pausable Capability — Design Document

> Closes #106. Coordinates with #84 (denylist-gate pausability) and #85
> (jurisdiction-flag pausability) by implementing both as part of a single
> shared helper rather than independently.

## Problem

Three contracts in this workspace — `denylist-gate`, `jurisdiction-flag`, and
`allowlist-token` — all need an admin-controlled pause switch that:

- blocks state-mutating operations while active,
- leaves read-only operations unaffected,
- is toggleable only by the contract's admin/issuer,
- emits auditable events on state change.

Implementing this three times independently creates identical logic with three
independent audit surfaces. Issues #84 and #85 each proposed adding pause to a
single contract; this issue (#106) asks for a shared implementation that all
three can adopt.

## Where the shared code lives

A new workspace crate: **`contracts/pausable`** (`compliance-pausable`).

It is a plain `#![no_std]` Rust library. It has:

- No `#[contract]` attribute.
- No `#[contractimpl]` block.
- No wasm function exports of any kind.

It exports four free functions:

```rust
pub fn is_paused(env: &Env) -> bool
pub fn pause(env: &Env)
pub fn unpause(env: &Env)
pub fn require_not_paused<E: Copy>(env: &Env, paused_err: E) -> Result<(), E>
```

Each function reads/writes a single `bool` stored under the symbol key
`"Paused"` in the calling contract's *instance* storage. Instance storage was
chosen because:

1. The pause flag is contract-global (not per-address), matching the semantics
   of the `Admin`/`Issuer` key already stored there.
2. It shares TTL management with the admin key — no separate TTL extension is
   needed.
3. It is cheap: one `get`/`set`/`remove` on a slot the contract already holds
   open.

## Why this doesn't hit the wasm-export-collision issue

The [`examples/denylist-gate-consumer`](../examples/denylist-gate-consumer/src/lib.rs)
notes this constraint:

> "this deliberately does NOT depend on the `denylist-gate` crate directly:
> linking another contract's crate pulls its `#[contractimpl]` wasm exports
> into this binary too and the two export sets collide at link time."

That constraint applies **only** to crates that carry a `#[contract]` /
`#[contractimpl]` annotation. Those macros emit `#[no_mangle] pub extern "C"`
symbols (the wasm host functions). If two such crates are compiled into the
same binary, the linker sees duplicate symbol definitions and errors out.

`compliance-pausable` has no `#[contract]`, no `#[contractimpl]`, and
therefore emits no wasm exports. It is ordinary Rust code; the Rust compiler
inlines it into each contract binary at compile time, contributing zero
additional exported symbols. There is no collision risk.

## How each contract plugs in

Each contract:

1. Adds `compliance-pausable = { workspace = true }` to its `Cargo.toml`.
2. Adds `ContractPaused = 4` to its own `Error` enum (no shared error type
   crossing contract boundaries — each contract owns its error set).
3. Adds `Paused` and `Unpaused` `#[contractevent]` structs (contract-local,
   so event schemas stay self-contained in each binary).
4. Adds `pause`, `unpause`, and `is_paused` methods to its `#[contractimpl]`
   block, each gated by the contract's existing `require_admin` /
   `require_issuer` helper.
5. Calls `compliance_pausable::require_not_paused(&env, Error::ContractPaused)?`
   at the top of every state-mutating method.

Read-only methods (`check`, `get_jurisdiction`, `is_permitted_jurisdiction`,
`is_allowed`) are deliberately **not** gated — compliance queries must always
be answerable regardless of operational status.

### Per-contract gating summary

| Contract           | Paused blocks                                                   | Paused does NOT block                                    |
|--------------------|-----------------------------------------------------------------|----------------------------------------------------------|
| `denylist-gate`    | `add_to_denylist`, `remove_from_denylist`                       | `check`                                                  |
| `jurisdiction-flag`| `set_jurisdiction`                                              | `get_jurisdiction`, `is_permitted_jurisdiction`          |
| `allowlist-token`  | `add_to_allowlist`, `remove_from_allowlist`, `transfer`         | `is_allowed`                                             |

For `allowlist-token`, `transfer` is additionally gated because a paused token
wrapper should not be forwarding funds. Note that a paused `transfer` returns
`Err(ContractPaused)` (which rolls back all state including events), not
`Ok(false)` — `Ok(false)` is reserved for the "address not allowlisted" path
where the audit `Blocked` event must survive.

## Auth model

Each contract's pause/unpause methods run through the same `require_admin` /
`require_issuer` helper that guards all other mutating operations. There is no
separate "pauser" role. This is intentional: the set of principals that can
modify list membership is the same set that can halt the contract entirely,
keeping the permission model flat and auditable.

## `#![no_std]` and wasm compatibility

`compliance-pausable` is `#![no_std]`. All four helpers operate purely through
the `soroban_sdk::Env` reference they receive — no heap allocation, no
`std::error::Error`, no trait objects. The `require_not_paused` function is
generic over the error type `E: Copy` rather than over a trait, which works
cleanly in `no_std` without any `std::error::Error` bound.

## What was considered but rejected

**A `Pausable` Rust trait** (as the issue title suggests): a trait with
`fn pause(&self, env: &Env)` etc. was explored. In `#![no_std]` Soroban
contracts, implementing a trait on a `#[contract]` struct and then calling
into it from the `#[contractimpl]` block is syntactically possible, but it
provides no advantage over free functions — the trait itself cannot be used
for dynamic dispatch (no `dyn Pausable`) without `alloc`, and static dispatch
adds generic parameters that propagate through the call sites. Free functions
with an `&Env` parameter are simpler, zero-cost, and achieve the same
deduplication goal.

**A shared `DataKey::Paused` variant in a common enum**: rejected because it
would require each contract to import an external `DataKey` type, coupling
storage schemas across contract crates. Instead, each contract keeps its own
`DataKey` enum; the shared functions use a fixed `symbol_short!("Paused")` key
that is guaranteed not to collide with any address-keyed persistent storage.

**Duplicated but templated code**: the issue specified this as the fallback
if a shared crate proved impractical. It proved practical, so duplication was
not needed.

## Testing

Each contract's test module gains dedicated pausable tests covering:

- `is_paused` defaults to `false`
- admin/issuer can pause and unpause
- every state-mutating method returns `ContractPaused` while paused
- every read-only method succeeds while paused
- mutations succeed again after unpausing
- non-admin/non-issuer cannot pause or unpause
- `pause` and `unpause` each emit the expected event

`compliance-pausable` itself has unit tests that exercise all four helpers
against a stub contract.
