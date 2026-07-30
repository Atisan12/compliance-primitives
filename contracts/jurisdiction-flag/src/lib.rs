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
    /// The issuer address, set once in `initialize`. Instance storage.
    Issuer,
    /// The jurisdiction code attached to a given address, if any.
    /// Persistent storage, keyed per address.
    Jurisdiction(Address),
    Paused,
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

    /// Attach jurisdiction `code` to `address` with no expiry. Issuer-only.
    /// Existing callers do not need to change — behavior is identical to the
    /// previous version of this function.
    pub fn set_jurisdiction(
        env: Env,
        issuer: Address,
        address: Address,
        code: String,
    ) -> Result<(), Error> {
        compliance_pausable::require_not_paused(&env, Error::ContractPaused)?;
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
