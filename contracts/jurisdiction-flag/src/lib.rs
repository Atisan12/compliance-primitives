// Copyright (c) 2026 Stellar Compliance Kit contributors
// SPDX-License-Identifier: MIT
// See the LICENSE file in the repository root for the full license text.

//! `jurisdiction-flag` is a `#![no_std]` Soroban contract that attaches a
//! jurisdiction code (e.g. an ISO 3166-1 alpha-2 country code) to an
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
//!
//! **Pausability**: the issuer may call `pause` to halt all mutating
//! operations (`set_jurisdiction`). The read-only `get_jurisdiction` and
//! `is_permitted_jurisdiction` methods are unaffected by pause state. The
//! shared [`compliance_pausable`] helper crate implements the pause storage
//! logic; this contract only supplies issuer-gating and event emission.
#![no_std]

use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, Address, Env, String, Vec,
};

/// Storage keys for this contract's state.
#[contracttype]
#[derive(Clone)]
enum DataKey {
    /// The issuer address, set once in `initialize`. Instance storage.
    Issuer,
    /// The jurisdiction code attached to a given address, if any.
    /// Persistent storage, keyed per address.
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
    /// `allowed_codes` was empty. An empty allowlist means nothing can ever
    /// pass, which is almost certainly a caller configuration mistake rather
    /// than a legitimate "nothing is permitted" intent. Callers should either
    /// supply at least one code or omit the jurisdiction check entirely.
    EmptyAllowedCodes = 4,
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

    /// Attach jurisdiction `code` to `address`. Issuer-only. Blocked while paused.
    pub fn set_jurisdiction(
        env: Env,
        issuer: Address,
        address: Address,
        code: String,
    ) -> Result<(), Error> {
        compliance_pausable::require_not_paused(&env, Error::ContractPaused)?;
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

    /// Returns the jurisdiction code attached to `address`, if any.
    ///
    /// **Not** affected by pause state — reads always succeed.
    pub fn get_jurisdiction(env: Env, address: Address) -> Option<String> {
        env.storage()
            .persistent()
            .get(&DataKey::Jurisdiction(address))
    }

    /// Returns the stored issuer address.
    pub fn get_issuer(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Issuer)
            .ok_or(Error::NotInitialized)
    }

    /// Attach jurisdiction codes to many addresses in a single transaction.
    /// Issuer-only; authorizes `issuer` once and then applies each entry via
    /// the same logic as `set_jurisdiction`.
    pub fn set_multiple_jurisdictions(
        env: Env,
        issuer: Address,
        entries: Vec<(Address, String)>,
    ) -> Result<(), Error> {
        Self::require_issuer(&env, &issuer)?;
        for (address, code) in entries.iter() {
            env.storage()
                .persistent()
                .set(&DataKey::Jurisdiction(address.clone()), &code);
            JurisdictionSet { address, code }.publish(&env);
        }
        Ok(())
    }

    /// Returns the jurisdiction code attached to `address`, or `default` if
    /// none has been set. Convenience wrapper around `get_jurisdiction` for
    /// callers that want to treat an unset address as belonging to a known
    /// fallback jurisdiction (e.g. `"XX"` for unknown/unrestricted) without
    /// having to unwrap an `Option` themselves.
    pub fn get_jurisdiction_or(env: Env, address: Address, default: String) -> String {
        Self::get_jurisdiction(env, address).unwrap_or(default)
    }

    /// Returns `Ok(true)` if `address` has a jurisdiction code set AND that
    /// code appears in `allowed_codes`. Returns `Ok(false)` if the address has
    /// no code set or its code is not in `allowed_codes`. Returns
    /// `Err(Error::EmptyAllowedCodes)` when `allowed_codes` is empty, because
    /// an empty allowlist means nothing can ever pass — that is almost
    /// certainly a caller configuration mistake rather than a deliberate
    /// "nothing is permitted" intent.
    pub fn is_permitted_jurisdiction(
        env: Env,
        address: Address,
        allowed_codes: Vec<String>,
    ) -> Result<bool, Error> {
        if allowed_codes.is_empty() {
            return Err(Error::EmptyAllowedCodes);
        }
        Ok(match Self::get_jurisdiction(env, address) {
            Some(code) => allowed_codes.iter().any(|c| c == code),
            None => false,
        })
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
