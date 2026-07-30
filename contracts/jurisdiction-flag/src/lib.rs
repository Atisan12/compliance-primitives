//! `jurisdiction-flag` is a `#![no_std]` Soroban contract that attaches one or
//! more jurisdiction codes (e.g. ISO 3166-1 alpha-2 country codes) to an
//! address.
//!
//! **Purpose**: let an issuer record which jurisdiction(s) an address has
//! been verified in — including dual citizenship/residency — so other
//! contracts can restrict activity to a permitted set of jurisdictions
//! without each one reimplementing that bookkeeping.
//!
//! **Storage shape**: `DataKey::Jurisdiction(Address)` stores a
//! `Vec<String>` of codes. The legacy single-code helpers
//! `set_jurisdiction` / `get_jurisdiction` remain as conveniences:
//! `set_jurisdiction` replaces the address's entire set with a one-element
//! vector, and `get_jurisdiction` returns the first code (if any). Prefer
//! `add_jurisdiction` / `remove_jurisdiction` / `list_jurisdictions` when
//! managing multiple codes. This shape leaves room for #83 (batch remove
//! over the same vec) and #110 (richer per-code metadata) without a second
//! parallel key.
//!
//! **Permission semantics**: `is_permitted_jurisdiction` uses *any*
//! matching — it returns `true` if at least one of the address's codes
//! appears in `allowed_codes`. An address with no codes is never permitted.
//!
//! **Pause**: the issuer can `pause` write-side mutations (`set`/`add`/
//! `remove`) during an incident without breaking read-side callers of
//! `get_jurisdiction` / `list_jurisdictions` / `is_permitted_jurisdiction`.
//! Same pattern as denylist-gate (#84).
//!
//! **Callers**: only the configured `issuer` address may mutate flags or
//! pause state. Any contract or off-chain client can read flags, and
//! contracts enforcing a jurisdiction allowlist can call
//! `is_permitted_jurisdiction(address, allowed_codes)` as part of their
//! own compliance checks.
//!
//! **Composition**: designed to be called into from another contract's
//! `transfer` or similar gating logic — the same pattern `denylist-gate`
//! uses — rather than deployed standalone.
#![no_std]

use soroban_sdk::{contract, contracterror, contractevent, contractimpl, contracttype, Address, Env, String, Vec};

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Issuer,
    Jurisdiction(Address),
    Paused,
}

#[contractevent]
pub struct JurisdictionSet {
    #[topic]
    pub address: Address,
    pub code: String,
}

#[contractevent]
pub struct JurisdictionAdded {
    #[topic]
    pub address: Address,
    pub code: String,
}

#[contractevent]
pub struct JurisdictionRemoved {
    #[topic]
    pub address: Address,
    pub code: String,
}

#[contractevent]
pub struct Paused {
    #[topic]
    pub issuer: Address,
}

#[contractevent]
pub struct Unpaused {
    #[topic]
    pub issuer: Address,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAuthorized = 3,
    Paused = 4,
    NotFound = 5,
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
        env.storage().instance().set(&DataKey::Paused, &false);
        Ok(())
    }

    /// Replace `address`'s jurisdiction set with a single `code`.
    /// Issuer-only convenience for single-code callers; clears any
    /// previously stored codes for that address.
    pub fn set_jurisdiction(
        env: Env,
        issuer: Address,
        address: Address,
        code: String,
    ) -> Result<(), Error> {
        Self::require_issuer(&env, &issuer)?;
        Self::require_not_paused(&env)?;
        let mut codes = Vec::new(&env);
        codes.push_back(code.clone());
        env.storage()
            .persistent()
            .set(&DataKey::Jurisdiction(address.clone()), &codes);
        JurisdictionSet { address, code }.publish(&env);
        Ok(())
    }

    /// Add `code` to `address`'s jurisdiction set if not already present.
    /// Issuer-only. No-op (still emits) if the code is already stored.
    pub fn add_jurisdiction(
        env: Env,
        issuer: Address,
        address: Address,
        code: String,
    ) -> Result<(), Error> {
        Self::require_issuer(&env, &issuer)?;
        Self::require_not_paused(&env)?;
        let mut codes = Self::list_jurisdictions(env.clone(), address.clone());
        let already = codes.iter().any(|c| c == code);
        if !already {
            codes.push_back(code.clone());
            env.storage()
                .persistent()
                .set(&DataKey::Jurisdiction(address.clone()), &codes);
        }
        JurisdictionAdded { address, code }.publish(&env);
        Ok(())
    }

    /// Remove a single `code` from `address`'s jurisdiction set. Issuer-only.
    /// Returns `Error::NotFound` if the code is not present.
    pub fn remove_jurisdiction(
        env: Env,
        issuer: Address,
        address: Address,
        code: String,
    ) -> Result<(), Error> {
        Self::require_issuer(&env, &issuer)?;
        Self::require_not_paused(&env)?;
        let existing = Self::list_jurisdictions(env.clone(), address.clone());
        let mut next = Vec::new(&env);
        let mut found = false;
        for c in existing.iter() {
            if c == code {
                found = true;
            } else {
                next.push_back(c);
            }
        }
        if !found {
            return Err(Error::NotFound);
        }
        if next.is_empty() {
            env.storage()
                .persistent()
                .remove(&DataKey::Jurisdiction(address.clone()));
        } else {
            env.storage()
                .persistent()
                .set(&DataKey::Jurisdiction(address.clone()), &next);
        }
        JurisdictionRemoved { address, code }.publish(&env);
        Ok(())
    }

    /// Returns all jurisdiction codes attached to `address` (empty if none).
    pub fn list_jurisdictions(env: Env, address: Address) -> Vec<String> {
        env.storage()
            .persistent()
            .get(&DataKey::Jurisdiction(address))
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Returns the first jurisdiction code attached to `address`, if any.
    /// Kept for backward compatibility with single-code callers; prefer
    /// `list_jurisdictions` when multiple codes may be present.
    pub fn get_jurisdiction(env: Env, address: Address) -> Option<String> {
        let codes = Self::list_jurisdictions(env, address);
        if codes.is_empty() {
            None
        } else {
            Some(codes.get(0).unwrap())
        }
    }

    /// Returns `true` if *any* of `address`'s jurisdiction codes appear in
    /// `allowed_codes` ("any" semantics, not "all"). An address with no
    /// codes set is never permitted. Meant to be called by other contracts
    /// that want to restrict activity to a set of permitted jurisdictions.
    pub fn is_permitted_jurisdiction(env: Env, address: Address, allowed_codes: Vec<String>) -> bool {
        let codes = Self::list_jurisdictions(env, address);
        if codes.is_empty() {
            return false;
        }
        for code in codes.iter() {
            if allowed_codes.iter().any(|c| c == code) {
                return true;
            }
        }
        false
    }

    /// Pause write-side mutations. Issuer-only. Reads remain available.
    pub fn pause(env: Env, issuer: Address) -> Result<(), Error> {
        Self::require_issuer(&env, &issuer)?;
        env.storage().instance().set(&DataKey::Paused, &true);
        Paused { issuer }.publish(&env);
        Ok(())
    }

    /// Resume write-side mutations. Issuer-only.
    pub fn unpause(env: Env, issuer: Address) -> Result<(), Error> {
        Self::require_issuer(&env, &issuer)?;
        env.storage().instance().set(&DataKey::Paused, &false);
        Unpaused { issuer }.publish(&env);
        Ok(())
    }

    /// Returns whether write-side mutations are currently paused.
    pub fn is_paused(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false)
    }

    fn require_not_paused(env: &Env) -> Result<(), Error> {
        if Self::is_paused(env.clone()) {
            return Err(Error::Paused);
        }
        Ok(())
    }

    fn require_issuer(env: &Env, issuer: &Address) -> Result<(), Error> {
        issuer.require_auth();
        let stored_issuer: Address = env.storage().instance().get(&DataKey::Issuer).ok_or(Error::NotInitialized)?;
        if stored_issuer != *issuer {
            return Err(Error::NotAuthorized);
        }
        Ok(())
    }
}

#[cfg(test)]
mod test;
