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
    Paused,
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

#[contractevent]
pub struct GatePaused {
    paused: bool,
}

#[contractevent]
pub struct GateUnpaused {
    paused: bool,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAuthorized = 3,
    Paused = 4,
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

    /// Pause admin mutations (`add_to_denylist` / `remove_from_denylist`).
    /// `check()` continues to work while paused. Admin-only.
    pub fn pause(env: Env, admin: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &true);
        GatePaused { paused: true }.publish(&env);
        Ok(())
    }

    /// Resume admin mutations after a `pause`. Admin-only.
    pub fn unpause(env: Env, admin: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &false);
        GateUnpaused { paused: false }.publish(&env);
        Ok(())
    }

    /// Add `address` to the denylist. Admin-only.
    ///
    /// # Storage TTL
    /// Denylist entries use persistent storage. If an entry were to fall out
    /// of the ledger's live-state window (archival) and the archive were not
    /// restored, `check()` would return `true` ("clear to transact") for the
    /// archived address — a **fail-open** footgun that is far more dangerous
    /// than the analogous case for an allowlist.
    ///
    /// To guard against this, we extend the TTL to `MAX_TTL` immediately
    /// after writing.  `MAX_TTL` (1 year ≈ 6 311 520 ledgers at 5 s/ledger)
    /// should be refreshed by the keeper script on every admin write; this
    /// call ensures a fresh write always starts with the maximum window.
    pub fn add_to_denylist(env: Env, admin: Address, address: Address) -> Result<(), Error> {
        Self::reject_if_paused(&env)?;
        Self::require_admin(&env, &admin)?;
        env.storage()
            .instance()
            .set(&DataKey::ComplianceOfficer, &officer);
        Ok(())
    }

    /// Revoke the compliance-officer role. Admin-only.
    pub fn revoke_compliance_officer(env: Env, admin: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        let key = DataKey::Denied(address.clone());
        env.storage().persistent().set(&key, &true);

        // Extend to ~1 year (6_311_520 ledgers at 5 s each).  The threshold
        // is set to half that so a keeper calling extend on every admin
        // interaction keeps entries perpetually live without on-chain storage
        // for the extension schedule.
        const MAX_TTL: u32 = 6_311_520;
        const THRESHOLD: u32 = MAX_TTL / 2;
        env.storage()
            .instance()
            .remove(&DataKey::ComplianceOfficer);
        Ok(())
    }

    /// Add `address` to the denylist. Admin or compliance-officer.
    pub fn add_to_denylist(env: Env, admin: Address, address: Address) -> Result<(), Error> {
        Self::require_compliance_authority(&env, &admin)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, THRESHOLD, MAX_TTL);

        DenyAdd { address }.publish(&env);
        Ok(())
    }

    /// Remove `address` from the denylist. Admin or compliance-officer.
    pub fn remove_from_denylist(env: Env, admin: Address, address: Address) -> Result<(), Error> {
        Self::reject_if_paused(&env)?;
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

    fn reject_if_paused(env: &Env) -> Result<(), Error> {
        if env
            .storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false)
        {
            return Err(Error::Paused);
        }
        Ok(())
    }

    fn require_admin(env: &Env, admin: &Address) -> Result<(), Error> {
        admin.require_auth();
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin).ok_or(Error::NotInitialized)?;
        if stored_admin != *admin {
            return Err(Error::NotAuthorized);
        }
        Ok(())
    }

    /// Checks that `caller` is either the admin or the compliance officer.
    fn require_compliance_authority(env: &Env, caller: &Address) -> Result<(), Error> {
        caller.require_auth();
        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        if stored_admin == *caller {
            return Ok(());
        }
        if let Some(officer) = env
            .storage()
            .instance()
            .get(&DataKey::ComplianceOfficer)
        {
            if officer == *caller {
                return Ok(());
            }
        }
        Err(Error::NotAuthorized)
    }
}

#[cfg(test)]
mod test;

#[cfg(test)]
mod fuzz_test;
