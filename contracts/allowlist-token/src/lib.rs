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
//!
//! **Pausability**: the admin may call `pause` to halt all mutating
//! operations (`add_to_allowlist`, `remove_from_allowlist`, `transfer`).
//! The read-only `is_allowed` method is unaffected by pause state. The
//! shared [`compliance_pausable`] helper crate implements the pause storage
//! logic; this contract only supplies admin-gating and event emission.
#![no_std]

extern crate alloc;

use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, token, Address, Bytes,
    BytesN, Env, Symbol,
};

/// Storage keys for this contract's state.
#[contracttype]
#[derive(Clone)]
enum DataKey {
    /// The admin address, set once in `initialize`. Instance storage.
    Admin,
    ComplianceOfficer,
    Token,
    DelegatedAdminPubKey,
    DelegatedNonce(Address),
    Allowed(Address),
}

#[contracttype]
#[derive(Clone)]
struct DelegatedAction {
    target: Address,
    action: Symbol,
    nonce: u64,
    expiry: u64,
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
pub struct AdminTransferred {
    #[topic]
    pub old_admin: Address,
    #[topic]
    pub new_admin: Address,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAuthorized = 3,
    DelegationNotConfigured = 4,
    InvalidSignature = 5,
    InvalidNonce = 6,
    ExpiredSignature = 7,
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

    /// Configure the ed25519 public key that may authorize delegated admin
    /// actions without the admin account itself needing to submit the
    /// transaction. The direct-auth path remains unchanged and still uses
    /// `admin.require_auth()`.
    pub fn set_delegated_admin_key(env: Env, admin: Address, pubkey: BytesN<32>) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::DelegatedAdminPubKey, &pubkey);
        Ok(())
    }

    /// Add `address` to the allowlist. Admin-only.
    pub fn add_to_allowlist(env: Env, admin: Address, address: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage()
            .instance()
            .remove(&DataKey::ComplianceOfficer);
        Ok(())
    }

    /// Add `address` to the allowlist. Admin or compliance-officer.
    pub fn add_to_allowlist(env: Env, admin: Address, address: Address) -> Result<(), Error> {
        Self::require_compliance_authority(&env, &admin)?;
        env.storage()
            .persistent()
            .set(&DataKey::Allowed(address.clone()), &true);
        AllowAdd { address }.publish(&env);
        Ok(())
    }

    /// Add `address` to the allowlist using a signed off-chain authorization
    /// payload. This path verifies a nonce and expiry before applying the
    /// allowlist change, so a relayer can submit it on behalf of the admin.
    pub fn add_to_allowlist_delegated(
        env: Env,
        admin: Address,
        address: Address,
        nonce: u64,
        expiry: u64,
        signature: BytesN<64>,
    ) -> Result<(), Error> {
        Self::require_configured_admin(&env, &admin)?;

        let now = env.ledger().timestamp();
        if expiry <= now {
            return Err(Error::ExpiredSignature);
        }

        let last_nonce: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::DelegatedNonce(admin.clone()))
            .unwrap_or(0);
        if nonce <= last_nonce {
            return Err(Error::InvalidNonce);
        }

        let pubkey: BytesN<32> = env
            .storage()
            .instance()
            .get(&DataKey::DelegatedAdminPubKey)
            .ok_or(Error::DelegationNotConfigured)?;
        let action = Symbol::new(&env, "add_to_allowlist");
        let message = Self::delegated_action_message(&env, &address, &action, nonce, expiry);
        match soroban_sdk::env::internal::Env::verify_sig_ed25519(
            &env,
            pubkey.to_object(),
            message.to_object(),
            signature.to_object(),
        ) {
            Ok(_) => {}
            Err(_) => return Err(Error::NotAuthorized),
        }

        env.storage()
            .persistent()
            .set(&DataKey::DelegatedNonce(admin.clone()), &nonce);
        env.storage()
            .persistent()
            .set(&DataKey::Allowed(address.clone()), &true);
        AllowAdd { address }.publish(&env);
        Ok(())
    }

    /// Remove `address` from the allowlist. Admin-only.
    pub fn remove_from_allowlist(env: Env, admin: Address, address: Address) -> Result<(), Error> {
        Self::require_compliance_authority(&env, &admin)?;
        env.storage()
            .persistent()
            .remove(&DataKey::Allowed(address.clone()));
        AllowRemove { address }.publish(&env);
        Ok(())
    }

    /// Propose a new admin. The current admin remains active until the
    /// proposed admin calls `accept_admin`.
    pub fn propose_admin(env: Env, current_admin: Address, new_admin: Address) -> Result<(), Error> {
        Self::require_admin(&env, &current_admin)?;
        env.storage().instance().set(&DataKey::PendingAdmin, &new_admin);
        Ok(())
    }

    /// Accept a pending admin transfer. Must be called by the proposed admin.
    pub fn accept_admin(env: Env, new_admin: Address) -> Result<(), Error> {
        new_admin.require_auth();

        let pending_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::PendingAdmin)
            .ok_or(Error::NoPendingAdmin)?;
        if pending_admin != new_admin {
            return Err(Error::PendingAdminMismatch);
        }

        let old_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        env.storage().instance().set(&DataKey::Admin, &new_admin);
        env.storage().instance().remove(&DataKey::PendingAdmin);
        AdminTransferred {
            old_admin,
            new_admin,
        }
        .publish(&env);
        Ok(())
    }

    /// Returns true if `address` is currently allowlisted.
    ///
    /// **Not** affected by pause state — reads always succeed.
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
    /// Blocked while paused — returns `Err(ContractPaused)`.
    ///
    /// Returns `Ok(false)` without forwarding the transfer if either party is
    /// not allowlisted, and emits a `Blocked` event so the attempt is
    /// auditable off-chain. A Soroban invocation that returns a contract
    /// error rolls back everything it did, including events, so a blocked
    /// attempt is reported as `Ok(false)` rather than an `Err` — that's what
    /// lets the audit event actually land. `Err` is reserved for
    /// configuration failures (e.g. the contract was never initialized or is
    /// paused).
    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) -> Result<bool, Error> {
        compliance_pausable::require_not_paused(&env, Error::ContractPaused)?;
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

    /// Pause the contract. Admin-only.
    ///
    /// While paused, `add_to_allowlist`, `remove_from_allowlist`, and
    /// `transfer` return `Error::ContractPaused`. `is_allowed` continues
    /// to work normally.
    pub fn pause(env: Env, admin: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        compliance_pausable::pause(&env);
        Paused { admin }.publish(&env);
        Ok(())
    }

    /// Unpause the contract. Admin-only.
    pub fn unpause(env: Env, admin: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        compliance_pausable::unpause(&env);
        Unpaused { admin }.publish(&env);
        Ok(())
    }

    /// Returns `true` if the contract is currently paused.
    pub fn is_paused(env: Env) -> bool {
        compliance_pausable::is_paused(&env)
    }

    fn require_admin(env: &Env, admin: &Address) -> Result<(), Error> {
        admin.require_auth();
        Self::require_configured_admin(env, admin)
    }

    fn require_configured_admin(env: &Env, admin: &Address) -> Result<(), Error> {
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

    fn delegated_action_message(env: &Env, target: &Address, action: &Symbol, nonce: u64, expiry: u64) -> Bytes {
        let mut message = Bytes::new(env);
        message.append(&Bytes::from_slice(env, b"allowlist-delegated-v1:"));
        let target_str = target.to_string().to_string();
        message.append(&Bytes::from_slice(env, target_str.as_bytes()));
        message.push_back(b':');
        let action_str = action.to_string().to_string();
        message.append(&Bytes::from_slice(env, action_str.as_bytes()));
        message.push_back(b':');
        let nonce_str = alloc::format!("{nonce}");
        message.append(&Bytes::from_slice(env, nonce_str.as_bytes()));
        message.push_back(b':');
        let expiry_str = alloc::format!("{expiry}");
        message.append(&Bytes::from_slice(env, expiry_str.as_bytes()));
        message
    }
}

#[cfg(test)]
mod test;
