//! `jurisdiction-flag` is a `#![no_std]` Soroban contract that attaches a
//! jurisdiction code (e.g. an ISO 3166-1 alpha-2 country code) to an
//! address.
//!
//! **Purpose**: let an issuer record which jurisdiction an address has been
//! verified in, so other contracts can restrict activity to a permitted set
//! of jurisdictions without each one reimplementing that bookkeeping.
//!
//! **Callers**: only the configured `issuer` address may call
//! `set_jurisdiction`. Any contract or off-chain client can read a flag via
//! `get_jurisdiction`, and contracts enforcing a jurisdiction allowlist can
//! call `is_permitted_jurisdiction(address, allowed_codes)` directly as part
//! of their own compliance checks.
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
}

#[contractevent]
pub struct JurisdictionSet {
    #[topic]
    pub address: Address,
    pub code: String,
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
        Ok(())
    }

    /// Attach jurisdiction `code` to `address`. Issuer-only.
    pub fn set_jurisdiction(
        env: Env,
        issuer: Address,
        address: Address,
        code: String,
    ) -> Result<(), Error> {
        Self::require_issuer(&env, &issuer)?;
        env.storage()
            .persistent()
            .set(&DataKey::Jurisdiction(address.clone()), &code);
        JurisdictionSet { address, code }.publish(&env);
        Ok(())
    }

    /// Returns the jurisdiction code attached to `address`, if any.
    pub fn get_jurisdiction(env: Env, address: Address) -> Option<String> {
        env.storage().persistent().get(&DataKey::Jurisdiction(address))
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
