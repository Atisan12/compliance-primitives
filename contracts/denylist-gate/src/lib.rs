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
#![no_std]

use soroban_sdk::{contract, contracterror, contractevent, contractimpl, contracttype, Address, Env};

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Admin,
    Denied(Address),
}

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

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAuthorized = 3,
}

#[contract]
pub struct DenylistGate;

#[contractimpl]
impl DenylistGate {
    /// One-time setup. `admin` is the only address allowed to update the
    /// denylist afterward.
    pub fn initialize(env: Env, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
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
            .persistent()
            .extend_ttl(&key, THRESHOLD, MAX_TTL);

        DenyAdd { address }.publish(&env);
        Ok(())
    }

    /// Remove `address` from the denylist. Admin-only.
    pub fn remove_from_denylist(env: Env, admin: Address, address: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage()
            .persistent()
            .remove(&DataKey::Denied(address.clone()));
        DenyRemove { address }.publish(&env);
        Ok(())
    }

    /// Returns `true` if `address` is clear to transact, i.e. it is NOT on
    /// the denylist. This is the function other contracts should call via
    /// cross-contract invocation before proceeding with a transfer.
    pub fn check(env: Env, address: Address) -> bool {
        !env
            .storage()
            .persistent()
            .get(&DataKey::Denied(address))
            .unwrap_or(false)
    }

    fn require_admin(env: &Env, admin: &Address) -> Result<(), Error> {
        admin.require_auth();
        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        if stored_admin != *admin {
            return Err(Error::NotAuthorized);
        }
        Ok(())
    }
}

#[cfg(test)]
mod test;
