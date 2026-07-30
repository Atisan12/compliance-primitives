// Copyright (c) 2026 Stellar Compliance Kit contributors
// SPDX-License-Identifier: MIT
// See the LICENSE file in the repository root for the full license text.

//! `allowlist-token` is a `#![no_std]` Soroban contract that wraps an existing
//! SEP-41 token and only permits `transfer` calls between two addresses that
//! are both present on an on-chain allowlist.
//!
//! **Purpose**: give issuers of permissioned tokens (e.g. RWA or regulated
//! stablecoins) a drop-in gate that blocks transfers to or from addresses
//! that haven't cleared KYC/onboarding, without modifying the underlying
//! token contract's own logic.
//!
//! **Callers**: an `admin` address manages the allowlist through
//! `add_to_allowlist`/`remove_from_allowlist`. End users — or the wallets
//! and apps acting on their behalf — call `transfer` exactly as they would
//! on a plain SEP-41 token; the allowlist check happens transparently.
//!
//! **Composition**: deploy this contract in front of an issuer's real token
//! and point clients at it instead of the underlying token — cleared
//! transfers are forwarded on via a cross-contract call. This is the one
//! primitive in the workspace meant to be deployed standalone rather than
//! called into by another contract; contrast with `denylist-gate` and
//! `jurisdiction-flag`, which are designed to be composed into a caller's
//! own token contract.
#![no_std]

use soroban_sdk::{contract, contracterror, contractevent, contractimpl, contracttype, token, Address, Env, Vec};

/// Storage keys for this contract's state.
#[contracttype]
#[derive(Clone)]
enum DataKey {
    /// The admin address, set once in `initialize`. Instance storage.
    Admin,
    /// The underlying SEP-41 token contract address transfers are
    /// forwarded to. Instance storage.
    Token,
    /// Whether a given address is on the allowlist. Persistent storage,
    /// keyed per address.
    Allowed(Address),
}

#[contractevent]
pub struct AllowAdd {
    #[topic]
    pub address: Address,
}

#[contractevent]
pub struct AllowRemove {
    #[topic]
    pub address: Address,
}

#[contractevent]
pub struct Blocked {
    #[topic]
    pub from: Address,
    #[topic]
    pub to: Address,
    pub amount: i128,
}

#[contractevent]
pub struct Paused {
    #[topic]
    pub by: Address,
}

#[contractevent]
pub struct Unpaused {
    #[topic]
    pub by: Address,
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
pub struct AllowlistToken;

#[contractimpl]
impl AllowlistToken {
    /// One-time setup. `admin` may manage the allowlist; `token` is the
    /// address of the underlying SEP-41 token contract that real transfers
    /// are forwarded to once both parties clear the allowlist check.
    pub fn initialize(env: Env, admin: Address, token: Address) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Token, &token);
        Ok(())
    }

    /// Add `address` to the allowlist. Admin-only.
    pub fn add_to_allowlist(env: Env, admin: Address, address: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().persistent().set(&DataKey::Allowed(address.clone()), &true);
        AllowAdd { address }.publish(&env);
        Ok(())
    }

    /// Add multiple addresses to the allowlist in one call. Admin-only.
    pub fn add_multiple_to_allowlist(env: Env, admin: Address, addresses: Vec<Address>) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        let mut index = 0u32;
        while index < addresses.len() {
            let address = addresses.get(index).unwrap();
            env.storage()
                .persistent()
                .set(&DataKey::Allowed(address.clone()), &true);
            AllowAdd { address: address.clone() }.publish(&env);
            index += 1;
        }
        Ok(())
    }

    /// Remove `address` from the allowlist. Admin-only.
    pub fn remove_from_allowlist(env: Env, admin: Address, address: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().persistent().remove(&DataKey::Allowed(address.clone()));
        AllowRemove { address }.publish(&env);
        Ok(())
    }

    /// Remove many addresses from the allowlist in a single transaction.
    /// Admin-only; authorizes `admin` once and then removes each address via
    /// the same logic as `remove_from_allowlist`.
    pub fn remove_multiple_from_allowlist(
        env: Env,
        admin: Address,
        addresses: Vec<Address>,
    ) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        for address in addresses.iter() {
            env.storage()
                .persistent()
                .remove(&DataKey::Allowed(address.clone()));
            AllowRemove { address }.publish(&env);
        }
        Ok(())
    }

    /// Returns true if `address` is currently allowlisted.
    pub fn is_allowed(env: Env, address: Address) -> bool {
        env.storage().persistent().get(&DataKey::Allowed(address)).unwrap_or(false)
    }

    /// Pause all transfers. Admin-only.
    pub fn pause(env: Env, admin: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &true);
        Paused { by: admin }.publish(&env);
        Ok(())
    }

    /// Unpause transfers. Admin-only.
    pub fn unpause(env: Env, admin: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &false);
        Unpaused { by: admin }.publish(&env);
        Ok(())
    }

    /// Returns true if transfers are paused.
    pub fn is_paused(env: Env) -> bool {
        env.storage().instance().get(&DataKey::Paused).unwrap_or(false)
    }

    /// Transfer `amount` of the underlying token from `from` to `to`.
    ///
    /// Returns `Ok(false)` without forwarding the transfer if either party is
    /// not allowlisted, and emits a `Blocked` event so the attempt is
    /// auditable off-chain. A Soroban invocation that returns a contract
    /// error rolls back everything it did, including events, so a blocked
    /// attempt is reported as `Ok(false)` rather than an `Err` — that's what
    /// lets the audit event actually land. `Err` is reserved for
    /// configuration failures (e.g. the contract was never initialized).
    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) -> Result<bool, Error> {
        from.require_auth();

        if !Self::is_allowed(env.clone(), from.clone())
            || !Self::is_allowed(env.clone(), to.clone())
        {
            Blocked { from, to, amount }.publish(&env);
            return Ok(false);
        }

        let token_address: Address = env.storage().instance().get(&DataKey::Token).ok_or(Error::NotInitialized)?;
        let token_client = token::Client::new(&env, &token_address);
        token_client.transfer(&from, &to, &amount);
        Ok(true)
    }

    fn require_admin(env: &Env, admin: &Address) -> Result<(), Error> {
        admin.require_auth();
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin).ok_or(Error::NotInitialized)?;
        if stored_admin != *admin {
            return Err(Error::NotAuthorized);
        }
        Ok(())
    }
}

#[cfg(test)]
mod test;
