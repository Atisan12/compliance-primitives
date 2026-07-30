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
#![no_std]

use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, Address, Env,
};

/// Storage keys for this contract's state.
#[contracttype]
#[derive(Clone)]
enum DataKey {
    /// The admin address, set once in `initialize`. Instance storage.
    Admin,
    /// Whether a given address is on the denylist. Persistent storage,
    /// keyed per address.
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

    /// Add `address` to the denylist and emit a [`DenyAdd`] event.
    ///
    /// # Auth
    /// Admin-only: `admin` must authorize the call and match the stored admin.
    ///
    /// # Returns
    /// `Ok(())` on success. Calling this again for an already-denied address
    /// is a no-op aside from emitting another [`DenyAdd`] event.
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if `initialize` has not been called.
    /// - [`Error::NotAuthorized`] if `admin` is not the stored admin.
    pub fn add_to_denylist(env: Env, admin: Address, address: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().persistent().set(&DataKey::Denied(address.clone()), &true);
        DenyAdd { address }.publish(&env);
        Ok(())
    }

    /// Remove `address` from the denylist and emit a [`DenyRemove`] event.
    ///
    /// # Auth
    /// Admin-only: `admin` must authorize the call and match the stored admin.
    ///
    /// # Returns
    /// `Ok(())` on success. Removing an address that was never denied (or
    /// was already removed) still succeeds and emits [`DenyRemove`].
    ///
    /// # Errors
    /// - [`Error::NotInitialized`] if `initialize` has not been called.
    /// - [`Error::NotAuthorized`] if `admin` is not the stored admin.
    pub fn remove_from_denylist(env: Env, admin: Address, address: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().persistent().remove(&DataKey::Denied(address.clone()));
        DenyRemove { address }.publish(&env);
        Ok(())
    }

    /// Returns the stored admin address.
    pub fn get_admin(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)
    }

    /// Returns `true` if `address` is clear to transact, i.e. it is NOT on
    /// the denylist. This is the function other contracts should call via
    /// cross-contract invocation before proceeding with a transfer.
    ///
    /// # Examples
    ///
    /// ```
    /// use denylist_gate::{DenylistGate, DenylistGateClient};
    /// use soroban_sdk::{testutils::Address as _, Address, Env};
    ///
    /// let env = Env::default();
    /// env.mock_all_auths();
    /// let admin = Address::generate(&env);
    /// let contract_id = env.register(DenylistGate, ());
    /// let client = DenylistGateClient::new(&env, &contract_id);
    /// client.initialize(&admin);
    ///
    /// let alice = Address::generate(&env);
    /// assert!(client.check(&alice));
    ///
    /// client.add_to_denylist(&admin, &alice);
    /// assert!(!client.check(&alice));
    /// ```
    pub fn check(env: Env, address: Address) -> bool {
        !env.storage()
            .persistent()
            .get(&DataKey::Denied(address))
            .unwrap_or(false)
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
