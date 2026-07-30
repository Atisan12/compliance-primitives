// Copyright (c) 2026 Stellar Compliance Kit contributors
// SPDX-License-Identifier: MIT
// See the LICENSE file in the repository root for the full license text.

//! `denylist-gate` is a `#![no_std]` Soroban contract that maintains a
//! standalone on-chain denylist.
//!
//! **Purpose**: give issuers a shared, independently auditable place to
//! record addresses that must never transact (sanctions hits, fraud, court
//! orders, etc.), decoupled from any single token contract's own storage.
//!
//! **Callers**: an `admin` address manages the denylist through
//! `add_to_denylist`/`remove_from_denylist`. Other contracts — typically a
//! token's `transfer` function — call the read-only `check(address)` via a
//! cross-contract call before moving funds, so the denylist can be updated
//! without redeploying or touching the token contract itself.
//!
//! **Composition**: this contract is meant to be called into, not deployed
//! as a token itself. See `/examples/denylist-gate-consumer` for a worked
//! example of a token contract wiring `check()` into its `transfer` path.
//!
//! **Audit-log integration (opt-in)**: call `set_audit_log(admin,
//! audit_log_address)` after deploying to wire in an `audit-log` contract
//! instance. Once set, every `add_to_denylist` and `remove_from_denylist`
//! call will additionally invoke `audit_log.record(...)` as a structured
//! compliance event. If `set_audit_log` is never called the behaviour is
//! identical to before — the extra call path is guarded by an
//! `Option<Address>` check on the stored audit-log address.
#![no_std]

use soroban_sdk::{contract, contracterror, contractevent, contractimpl, contracttype, Address, Env, Vec};

/// Batch operations are capped to reduce the chance of a single invocation
/// exceeding Soroban instruction/resource limits.
const MAX_BATCH_SIZE: u32 = 100;

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

/// Storage keys for this contract's state.
#[contracttype]
#[derive(Clone)]
enum DataKey {
    /// The admin address, set once in `initialize`. Instance storage.
    Admin,
    /// Whether a given address is on the denylist. Persistent storage,
    /// keyed per address.
    Denied(Address),
    /// Optional address of an `audit-log` contract to emit structured
    /// compliance events to. Not set by default — must be explicitly
    /// configured via `set_audit_log`.
    AuditLog,
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

#[contractevent]
pub struct DenyAdd {
    #[topic]
    pub address: Address,
}

#[contractevent]
pub struct DenyRemove {
    #[topic]
    pub address: Address,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAuthorized = 3,
    BatchTooLarge = 4,
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct DenylistGate;

#[contractimpl]
impl DenylistGate {
    /// One-time setup. Stores `admin` as the only address allowed to update
    /// the denylist afterward.
    ///
    /// # Auth
    /// Requires authorization from `admin` via `require_auth()`.
    ///
    /// # Returns
    /// `Ok(())` on success.
    ///
    /// # Errors
    /// - [`Error::AlreadyInitialized`] if `initialize` was already called.
    pub fn initialize(env: Env, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    /// Register an `audit-log` contract address. Admin-only. Once set,
    /// state-mutating calls (`add_to_denylist`, `remove_from_denylist`) will
    /// additionally call `audit_log.record(...)` via cross-contract
    /// invocation so that a single contract instance aggregates compliance
    /// events across all primitives.
    pub fn set_audit_log(env: Env, admin: Address, audit_log: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage()
            .instance()
            .set(&DataKey::AuditLog, &audit_log);
        Ok(())
    }

    /// Add `address` to the denylist. Admin-only.
    pub fn add_to_denylist(env: Env, admin: Address, address: Address) -> Result<(), Error> {
        compliance_pausable::require_not_paused(&env, Error::ContractPaused)?;
        Self::require_admin(&env, &admin)?;
        env.storage()
            .persistent()
            .set(&DataKey::Denied(address.clone()), &true);
        DenyAdd {
            address: address.clone(),
        }
        .publish(&env);

        // Opt-in: forward a structured compliance event to the audit-log if
        // one has been configured.
        Self::maybe_record(
            &env,
            &address,
            Symbol::new(&env, "deny_add"),
            String::from_str(&env, "added to denylist"),
        );

        Ok(())
    }

    /// Remove `address` from the denylist. Admin-only. Blocked while paused.
    pub fn remove_from_denylist(env: Env, admin: Address, address: Address) -> Result<(), Error> {
        compliance_pausable::require_not_paused(&env, Error::ContractPaused)?;
        Self::require_admin(&env, &admin)?;
        env.storage()
            .persistent()
            .remove(&DataKey::Denied(address.clone()));
        DenyRemove {
            address: address.clone(),
        }
        .publish(&env);

        Self::maybe_record(
            &env,
            &address,
            Symbol::new(&env, "deny_remove"),
            String::from_str(&env, "removed from denylist"),
        );

        Ok(())
    }

    /// Remove every address in `addresses` from the denylist. Admin-only.
    pub fn remove_multiple_from_denylist(env: Env, admin: Address, addresses: Vec<Address>) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        if addresses.len() > MAX_BATCH_SIZE {
            return Err(Error::BatchTooLarge);
        }

        for address in addresses.iter() {
            env.storage()
                .persistent()
                .remove(&DataKey::Denied(address.clone()));
            DenyRemove { address }.publish(&env);
        }
        Ok(())
    }

    /// Returns `true` if `address` is clear to transact, i.e. it is NOT on
    /// the denylist. This is the function other contracts should call via
    /// cross-contract invocation before proceeding with a transfer.
    ///
    /// **Not** affected by pause state — reads always succeed.
    pub fn check(env: Env, address: Address) -> bool {
        !env.storage()
            .persistent()
            .get(&DataKey::Denied(address))
            .unwrap_or(false)
    }

    /// Returns `true` if `address` is currently on the denylist.
    pub fn is_denylisted(env: Env, address: Address) -> bool {
        !Self::check(env, address)
    }

    fn require_admin(env: &Env, admin: &Address) -> Result<(), Error> {
        admin.require_auth();
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin).ok_or(Error::NotInitialized)?;
        if stored_admin != *admin {
            return Err(Error::NotAuthorized);
        }
        Ok(())
    }

    /// If an audit-log address has been configured, call `record` on it with
    /// the contract's own address as the `source`. This is the opt-in path:
    /// if `DataKey::AuditLog` is not set, this function is a no-op.
    fn maybe_record(env: &Env, subject: &Address, kind: Symbol, detail: String) {
        if let Some(audit_log_address) = env
            .storage()
            .instance()
            .get::<DataKey, Address>(&DataKey::AuditLog)
        {
            let client = AuditLogClient::new(env, &audit_log_address);
            // The source is this contract itself — Soroban's auth model
            // allows a contract to authorize calls it makes from within its
            // own execution context.
            let source = env.current_contract_address();
            client.record(&source, &kind, subject, &detail);
        }
    }
}

#[cfg(test)]
mod test;
