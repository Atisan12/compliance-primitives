//! Reference example: a minimal token contract that composes
//! `denylist-gate` via cross-contract call. This crate is not meant to be
//! deployed as-is — it exists to show the calling pattern other issuers'
//! token contracts should follow: call `check()` on the gate for both
//! parties before mutating any balances, and abort the transfer if either
//! check fails.
//!
//! Note this deliberately does NOT depend on the `denylist-gate` crate
//! directly: linking another contract's crate pulls its `#[contractimpl]`
//! wasm exports into this binary too and the two export sets collide at
//! link time. Instead, `GateClient` below is generated from a trait that
//! only describes the shape of the call — the standard pattern for calling
//! a contract you don't own the source of in the same build.
#![no_std]

use soroban_sdk::{
    contract, contractclient, contracterror, contractimpl, contracttype, Address, Env,
};

#[contractclient(name = "GateClient")]
pub trait DenylistGateInterface {
    fn check(env: Env, address: Address) -> bool;
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Gate,
    Balance(Address),
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    InsufficientBalance = 3,
    DeniedByGate = 4,
}

#[contract]
pub struct ExampleToken;

#[contractimpl]
impl ExampleToken {
    /// `gate` is the address of a deployed `denylist-gate` contract instance.
    pub fn initialize(env: Env, gate: Address) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Gate) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Gate, &gate);
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

    /// Transfer `amount` from `from` to `to`, gated by `denylist-gate`.
    /// This is the pattern to copy: resolve the gate's address, build a
    /// client for it, and call `check()` for both parties via a
    /// cross-contract call before touching any state.
    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) -> Result<(), Error> {
        from.require_auth();

        let gate_address: Address = env
            .storage()
            .instance()
            .get(&DataKey::Gate)
            .ok_or(Error::NotInitialized)?;
        let gate = GateClient::new(&env, &gate_address);

        if !gate.check(&from) || !gate.check(&to) {
            return Err(Error::DeniedByGate);
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
