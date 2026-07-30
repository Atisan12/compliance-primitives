//! Shared pausable helper for `compliance-primitives` contracts.
//!
//! # Why a shared crate instead of per-contract duplication?
//!
//! All three contracts (`denylist-gate`, `jurisdiction-flag`, `allowlist-token`)
//! need identical pause/unpause/is_paused semantics. Duplicating that logic in
//! each contract adds audit surface with no benefit. A shared *library* crate
//! (no `#[contract]` macro, no wasm exports) is safe to depend on from all
//! three contracts because it contributes zero wasm symbol exports of its own
//! — it is just ordinary Rust code that gets inlined into each contract's
//! binary at compile time.
//!
//! # Why not depend on the other contract crates directly?
//!
//! Each `#[contract]` crate — `denylist-gate`, `jurisdiction-flag`,
//! `allowlist-token` — has a `#[contractimpl]` block that emits wasm function
//! exports. If contract A depended on contract B's crate, B's exports would be
//! linked into A's binary as well, causing symbol collisions at link time (the
//! linker sees two definitions of the same exported symbol). This crate has no
//! `#[contract]` annotation and therefore produces no wasm exports, so that
//! collision cannot occur.
//!
//! # Storage layout
//!
//! Pause state is stored as a `bool` under the fixed symbol key `"Paused"` in
//! the calling contract's *instance* storage. Instance storage was chosen
//! because:
//!
//! - The pause flag is contract-global (not per-address), matching the
//!   semantics of the `Admin`/`Issuer` key already stored there.
//! - It shares TTL management with the admin key — no separate TTL extension
//!   is needed.
//!
//! # Usage
//!
//! Each contract that wants pausability:
//!
//! 1. Adds `ContractPaused = 4` to its own `Error` enum.
//! 2. Adds `Paused`/`Unpaused` `#[contractevent]` structs (kept local so each
//!    binary's event schema is self-contained).
//! 3. Adds `pause`, `unpause`, and `is_paused` methods gated by the contract's
//!    existing admin/issuer auth helper.
//! 4. Calls [`require_not_paused`] at the top of every state-mutating method.
//! 5. Read-only methods are deliberately **not** gated.
#![no_std]

use soroban_sdk::Env;

/// Returns `true` if the calling contract is currently paused.
///
/// Reads a `bool` from the calling contract's instance storage under the
/// fixed key `"Paused"`. Absent key is treated as `false` (not paused).
pub fn is_paused(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&soroban_sdk::symbol_short!("Paused"))
        .unwrap_or(false)
}

/// Set the calling contract into the paused state.
///
/// Stores `true` under `"Paused"` in instance storage. Does not emit an
/// event — callers should publish a `Paused` event after calling this.
pub fn pause(env: &Env) {
    env.storage()
        .instance()
        .set(&soroban_sdk::symbol_short!("Paused"), &true);
}

/// Clear the paused state, allowing mutations to proceed again.
///
/// Removes the `"Paused"` key from instance storage (equivalent to `false`).
pub fn unpause(env: &Env) {
    env.storage()
        .instance()
        .remove(&soroban_sdk::symbol_short!("Paused"));
}

/// Returns `Err(paused_err)` if the calling contract is currently paused,
/// otherwise `Ok(())`.
///
/// The caller supplies the concrete error value to return, keeping this
/// helper free of any generic trait bound that would require `std::error::Error`.
///
/// # Example
/// ```rust,ignore
/// compliance_pausable::require_not_paused(&env, Error::ContractPaused)?;
/// ```
pub fn require_not_paused<E: Copy>(env: &Env, paused_err: E) -> Result<(), E> {
    if is_paused(env) {
        Err(paused_err)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{contract, contractimpl, Env};

    // Minimal stub contract used only to provide a live Env context for tests.
    #[contract]
    struct StubContract;

    #[contractimpl]
    impl StubContract {}

    fn env_with_contract() -> (Env, soroban_sdk::Address) {
        let env = Env::default();
        let id = env.register(StubContract, ());
        (env, id)
    }

    #[test]
    fn test_not_paused_by_default() {
        let (env, id) = env_with_contract();
        env.as_contract(&id, || {
            assert!(!is_paused(&env));
        });
    }

    #[test]
    fn test_pause_and_unpause() {
        let (env, id) = env_with_contract();
        env.as_contract(&id, || {
            assert!(!is_paused(&env));
            pause(&env);
            assert!(is_paused(&env));
            unpause(&env);
            assert!(!is_paused(&env));
        });
    }

    #[test]
    fn test_require_not_paused_ok_when_unpaused() {
        let (env, id) = env_with_contract();
        env.as_contract(&id, || {
            assert_eq!(require_not_paused(&env, 42u32), Ok(()));
        });
    }

    #[test]
    fn test_require_not_paused_err_when_paused() {
        let (env, id) = env_with_contract();
        env.as_contract(&id, || {
            pause(&env);
            assert_eq!(require_not_paused(&env, 99u32), Err(99u32));
        });
    }

    #[test]
    fn test_pause_is_idempotent() {
        let (env, id) = env_with_contract();
        env.as_contract(&id, || {
            pause(&env);
            pause(&env);
            assert!(is_paused(&env));
            unpause(&env);
            assert!(!is_paused(&env));
        });
    }
}
