//! Reference example: an RWA-style token that composes all three compliance
//! primitives before mutating balances.
//!
//! Check order in `transfer` (fail-fast, specific error per gate):
//! 1. **Allowlist** — both `from` and `to` must pass `is_allowed`
//! 2. **Denylist** — both parties must pass `denylist-gate.check`
//! 3. **Jurisdiction** — both parties must pass
//!    `is_permitted_jurisdiction` against the token's configured
//!    `allowed_codes`
//!
//! Like `/examples/denylist-gate-consumer`, this crate does **not** depend
//! on the primitive crates' `#[contractimpl]` binaries at link time.
//! Clients are generated from `#[contractclient]` traits that only describe
//! the call shape.
#![no_std]

use soroban_sdk::{
    contract, contractclient, contracterror, contractimpl, contracttype, Address, Env, String, Vec,
};

#[contractclient(name = "AllowlistClient")]
pub trait AllowlistInterface {
    fn is_allowed(env: Env, address: Address) -> bool;
}

#[contractclient(name = "GateClient")]
pub trait DenylistGateInterface {
    fn check(env: Env, address: Address) -> bool;
}

#[contractclient(name = "JurisdictionClient")]
pub trait JurisdictionFlagInterface {
    fn is_permitted_jurisdiction(env: Env, address: Address, allowed_codes: Vec<String>) -> bool;
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Allowlist,
    Gate,
    Jurisdiction,
    AllowedCodes,
    Balance(Address),
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    InsufficientBalance = 3,
    NotAllowlisted = 4,
    DeniedByGate = 5,
    JurisdictionNotPermitted = 6,
}

#[contract]
pub struct RwaToken;

#[contractimpl]
impl RwaToken {
    /// Wire this token to deployed primitive instances and the set of
    /// jurisdiction codes that may hold/transfer it.
    pub fn initialize(
        env: Env,
        allowlist: Address,
        gate: Address,
        jurisdiction: Address,
        allowed_codes: Vec<String>,
    ) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Allowlist) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Allowlist, &allowlist);
        env.storage().instance().set(&DataKey::Gate, &gate);
        env.storage()
            .instance()
            .set(&DataKey::Jurisdiction, &jurisdiction);
        env.storage()
            .instance()
            .set(&DataKey::AllowedCodes, &allowed_codes);
        Ok(())
    }

    /// Test/demo helper to fund an address with an initial balance.
    pub fn mint(env: Env, to: Address, amount: i128) {
        let balance = Self::balance(env.clone(), to.clone());
        env.storage()
            .persistent()
            .set(&DataKey::Balance(to), &(balance + amount));
    }

    pub fn balance(env: Env, address: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Balance(address))
            .unwrap_or(0)
    }

    /// Transfer `amount` from `from` to `to` after all three compliance
    /// checks clear. Errors identify which gate failed.
    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) -> Result<(), Error> {
        from.require_auth();

        let allowlist_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::Allowlist)
            .ok_or(Error::NotInitialized)?;
        let gate_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::Gate)
            .ok_or(Error::NotInitialized)?;
        let jurisdiction_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::Jurisdiction)
            .ok_or(Error::NotInitialized)?;
        let allowed_codes: Vec<String> = env
            .storage()
            .instance()
            .get(&DataKey::AllowedCodes)
            .ok_or(Error::NotInitialized)?;

        let allowlist = AllowlistClient::new(&env, &allowlist_addr);
        if !allowlist.is_allowed(&from) || !allowlist.is_allowed(&to) {
            return Err(Error::NotAllowlisted);
        }

        let gate = GateClient::new(&env, &gate_addr);
        if !gate.check(&from) || !gate.check(&to) {
            return Err(Error::DeniedByGate);
        }

        let jurisdiction = JurisdictionClient::new(&env, &jurisdiction_addr);
        if !jurisdiction.is_permitted_jurisdiction(&from, &allowed_codes)
            || !jurisdiction.is_permitted_jurisdiction(&to, &allowed_codes)
        {
            return Err(Error::JurisdictionNotPermitted);
        }

        let from_balance = Self::balance(env.clone(), from.clone());
        if from_balance < amount {
            return Err(Error::InsufficientBalance);
        }
        let to_balance = Self::balance(env.clone(), to.clone());

        env.storage()
            .persistent()
            .set(&DataKey::Balance(from.clone()), &(from_balance - amount));
        env.storage()
            .persistent()
            .set(&DataKey::Balance(to.clone()), &(to_balance + amount));
        Ok(())
    }
}

#[cfg(test)]
mod test;
