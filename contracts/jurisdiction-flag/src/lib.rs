//! `jurisdiction-flag` is a `#![no_std]` Soroban contract that attaches a
//! jurisdiction code (e.g. an ISO 3166-1 alpha-2 country code) to an
//! address.
//!
//! **Purpose**: let an issuer record which jurisdiction an address has been
//! verified in, so other contracts can restrict activity to a permitted set
//! of jurisdictions without each one reimplementing that bookkeeping.
//!
//! **Callers**: only the configured `issuer` address may call
//! `set_jurisdiction` (or `set_jurisdiction_until`). Any contract or
//! off-chain client can read a flag via `get_jurisdiction`, and contracts
//! enforcing a jurisdiction allowlist can call
//! `is_permitted_jurisdiction(address, allowed_codes)` directly as part of
//! their own compliance checks.
//!
//! **Time-bound flags**: `set_jurisdiction_until` stores the flag with a
//! `valid_until` ledger sequence number. Once `env.ledger().sequence()`
//! exceeds that value the flag is treated as unset (returns `None` / `false`).
//! `set_jurisdiction` sets `valid_until: None` (never expires) and is
//! fully backward-compatible.
//!
//! **Composition**: designed to be called into from another contract's
//! `transfer` or similar gating logic — the same pattern `denylist-gate`
//! uses — rather than deployed standalone.
#![no_std]

use soroban_sdk::{contract, contracterror, contractevent, contractimpl, contracttype, Address, Env, String, Vec};

/// Storage value for a jurisdiction flag. `valid_until` is the last ledger
/// sequence number at which the flag is still valid. `None` means the flag
/// never expires.
#[contracttype]
#[derive(Clone)]
pub struct JurisdictionEntry {
    pub code: String,
    pub valid_until: Option<u32>,
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Issuer,
    Jurisdiction(Address),
}

/// Emitted whenever a jurisdiction flag is set (with or without expiry).
#[contractevent]
pub struct JurisdictionSet {
    #[topic]
    pub address: Address,
    pub code: String,
    pub valid_until: Option<u32>,
}

/// Emitted (as a signal for off-chain indexers) when an expired flag is
/// encountered during a read. The flag is not removed from storage — it is
/// simply ignored — but this event lets listeners react.
#[contractevent]
pub struct JurisdictionExpired {
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
pub struct JurisdictionFlag;

#[contractimpl]
impl JurisdictionFlag {
    /// One-time setup. `issuer` is the only address allowed to set
    /// jurisdiction codes afterward.
    pub fn initialize(env: Env, issuer: Address) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Issuer) {
            return Err(Error::AlreadyInitialized);
        }
        issuer.require_auth();
        env.storage().instance().set(&DataKey::Issuer, &issuer);
        Ok(())
    }

    /// Attach jurisdiction `code` to `address` with no expiry. Issuer-only.
    /// Existing callers do not need to change — behavior is identical to the
    /// previous version of this function.
    pub fn set_jurisdiction(
        env: Env,
        issuer: Address,
        address: Address,
        code: String,
    ) -> Result<(), Error> {
        Self::require_issuer(&env, &issuer)?;
        let entry = JurisdictionEntry {
            code: code.clone(),
            valid_until: None,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Jurisdiction(address.clone()), &entry);
        JurisdictionSet {
            address,
            code,
            valid_until: None,
        }
        .publish(&env);
        Ok(())
    }

    /// Attach jurisdiction `code` to `address` that expires after ledger
    /// sequence `valid_until` (inclusive). Issuer-only.
    pub fn set_jurisdiction_until(
        env: Env,
        issuer: Address,
        address: Address,
        code: String,
        valid_until: u32,
    ) -> Result<(), Error> {
        Self::require_issuer(&env, &issuer)?;
        let entry = JurisdictionEntry {
            code: code.clone(),
            valid_until: Some(valid_until),
        };
        env.storage()
            .persistent()
            .set(&DataKey::Jurisdiction(address.clone()), &entry);
        JurisdictionSet {
            address,
            code,
            valid_until: Some(valid_until),
        }
        .publish(&env);
        Ok(())
    }

    /// Returns the jurisdiction code attached to `address`, if any.
    ///
    /// Returns `None` if:
    /// - no jurisdiction has been set, or
    /// - the flag has a `valid_until` that is strictly less than the current
    ///   ledger sequence (i.e. the flag has expired).
    pub fn get_jurisdiction(env: Env, address: Address) -> Option<String> {
        let entry: JurisdictionEntry = env
            .storage()
            .persistent()
            .get(&DataKey::Jurisdiction(address.clone()))?;

        if let Some(valid_until) = entry.valid_until {
            if env.ledger().sequence() > valid_until {
                // Flag has expired — treat as unset.
                JurisdictionExpired { address }.publish(&env);
                return None;
            }
        }

        Some(entry.code)
    }

    /// Returns `true` if `address` has a non-expired jurisdiction code set
    /// AND that code appears in `allowed_codes`. Meant to be called by other
    /// contracts that want to restrict activity to a set of permitted
    /// jurisdictions.
    pub fn is_permitted_jurisdiction(env: Env, address: Address, allowed_codes: Vec<String>) -> bool {
        match Self::get_jurisdiction(env, address) {
            Some(code) => allowed_codes.iter().any(|c| c == code),
            None => false,
        }
    }

    fn require_issuer(env: &Env, issuer: &Address) -> Result<(), Error> {
        issuer.require_auth();
        let stored_issuer: Address = env
            .storage()
            .instance()
            .get(&DataKey::Issuer)
            .ok_or(Error::NotInitialized)?;
        if stored_issuer != *issuer {
            return Err(Error::NotAuthorized);
        }
        Ok(())
    }
}

#[cfg(test)]
mod test;
