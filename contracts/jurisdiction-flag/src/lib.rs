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
//!
//! **Pausability**: the issuer may call `pause` to halt all mutating
//! operations (`set_jurisdiction`). The read-only `get_jurisdiction` and
//! `is_permitted_jurisdiction` methods are unaffected by pause state. The
//! shared [`compliance_pausable`] helper crate implements the pause storage
//! logic; this contract only supplies issuer-gating and event emission.
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
    ContractPaused = 4,
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

    /// Attach jurisdiction `code` to `address`. Issuer-only. Blocked while paused.
    pub fn set_jurisdiction(
        env: Env,
        issuer: Address,
        address: Address,
        code: String,
    ) -> Result<(), Error> {
        compliance_pausable::require_not_paused(&env, Error::ContractPaused)?;
        Self::require_issuer(&env, &issuer)?;
        env.storage()
            .persistent()
            .set(&DataKey::Jurisdiction(address.clone()), &code);
        JurisdictionSet { address, code }.publish(&env);
        Ok(())
    }

    /// Returns the jurisdiction code attached to `address`, if any.
    ///
    /// **Not** affected by pause state — reads always succeed.
    pub fn get_jurisdiction(env: Env, address: Address) -> Option<String> {
        env.storage().persistent().get(&DataKey::Jurisdiction(address))
    }

    /// Returns `true` if `address` has a jurisdiction code set AND that code
    /// appears in `allowed_codes`. Meant to be called by other contracts
    /// that want to restrict activity to a set of permitted jurisdictions.
    ///
    /// **Not** affected by pause state — reads always succeed.
    pub fn is_permitted_jurisdiction(env: Env, address: Address, allowed_codes: Vec<String>) -> bool {
        match Self::get_jurisdiction(env, address) {
            Some(code) => allowed_codes.iter().any(|c| c == code),
            None => false,
        }
    }

    /// Pause the contract. Issuer-only.
    ///
    /// While paused, `set_jurisdiction` returns `Error::ContractPaused`.
    /// Read methods continue to work normally.
    pub fn pause(env: Env, issuer: Address) -> Result<(), Error> {
        Self::require_issuer(&env, &issuer)?;
        compliance_pausable::pause(&env);
        Paused { issuer }.publish(&env);
        Ok(())
    }

    /// Unpause the contract. Issuer-only.
    pub fn unpause(env: Env, issuer: Address) -> Result<(), Error> {
        Self::require_issuer(&env, &issuer)?;
        compliance_pausable::unpause(&env);
        Unpaused { issuer }.publish(&env);
        Ok(())
    }

    /// Returns `true` if the contract is currently paused.
    pub fn is_paused(env: Env) -> bool {
        compliance_pausable::is_paused(&env)
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
