//! `denylist-gate-sep41` — a SEP-41-conformant token contract that gates every
//! `transfer` call through a deployed `denylist-gate` instance.
//!
//! # Why this example exists
//!
//! [`examples/denylist-gate-consumer`] shows the cross-contract calling pattern
//! using a minimal from-scratch token that does not implement the full SEP-41
//! interface.  That is enough to understand the composition mechanic, but a real
//! issuer needs to see the pattern applied to a token that actually satisfies
//! the SEP-41 interface — so this crate fills that gap.
//!
//! ## SEP-41 interface
//!
//! SEP-41 specifies these entry points (see
//! <https://github.com/stellar/stellar-protocol/blob/master/ecosystem/sep-0041.md>):
//!
//! - `initialize(admin, decimal, name, symbol)`
//! - `allowance(from, spender) -> i128`
//! - `approve(from, spender, amount, expiration_ledger)`
//! - `balance(id) -> i128`
//! - `transfer(from, to, amount)`
//! - `transfer_from(spender, from, to, amount)`
//! - `burn(from, amount)`
//! - `burn_from(spender, from, amount)`
//! - `decimals() -> u32`
//! - `name() -> String`
//! - `symbol() -> String`
//!
//! This implementation provides all required entry points and gates `transfer`
//! (and `transfer_from`) through `denylist-gate`.  Admin-only `mint` is added
//! as a convenience for tests.
//!
//! ## Composition pattern — same as `denylist-gate-consumer`
//!
//! The key difference from that example is that here the token's outer API is
//! fully SEP-41 shaped, so wallets and DEXes see a standard token.  The gate
//! check is invisible to callers — they just call `transfer` as normal, and
//! receive a `DeniedByGate` error if either party is on the denylist.
//!
//! The `#[contractclient]` trait trick is identical: we do **not** add
//! `denylist-gate` as a `[dependencies]` entry (only as `[dev-dependencies]`
//! for tests), so the gate's `#[contractimpl]` exports don't collide with this
//! contract's exports at link time.
#![no_std]

use soroban_sdk::{
    contract, contractclient, contractevent, contracterror, contractimpl, contracttype, Address,
    Env, String,
};

// ---------------------------------------------------------------------------
// Cross-contract client for denylist-gate (trait-only, no binary coupling).
// ---------------------------------------------------------------------------

#[contractclient(name = "GateClient")]
pub trait DenylistGateInterface {
    fn check(env: Env, address: Address) -> bool;
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Emitted when a transfer is denied by the denylist gate.
#[contractevent]
pub struct TransferDenied {
    #[topic]
    pub from: Address,
    #[topic]
    pub to: Address,
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Admin,
    Gate,
    Decimals,
    Name,
    Symbol,
    Balance(Address),
    Allowance(Address, Address), // (owner, spender)
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    NotAuthorized = 3,
    InsufficientBalance = 4,
    InsufficientAllowance = 5,
    DeniedByGate = 6,
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct Sep41GatedToken;

#[contractimpl]
impl Sep41GatedToken {
    // -----------------------------------------------------------------------
    // Admin / setup
    // -----------------------------------------------------------------------

    /// One-time setup. `gate` is the address of a deployed `denylist-gate`
    /// contract instance; this token will call `gate.check(address)` before
    /// every transfer.
    pub fn initialize(
        env: Env,
        admin: Address,
        gate: Address,
        decimal: u32,
        name: String,
        symbol: String,
    ) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Gate, &gate);
        env.storage().instance().set(&DataKey::Decimals, &decimal);
        env.storage().instance().set(&DataKey::Name, &name);
        env.storage().instance().set(&DataKey::Symbol, &symbol);
        Ok(())
    }

    /// Mint `amount` tokens to `to`. Admin-only; not part of SEP-41 but
    /// required for bootstrapping in tests/demos.
    pub fn mint(env: Env, admin: Address, to: Address, amount: i128) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        let bal = Self::balance(env.clone(), to.clone());
        env.storage()
            .persistent()
            .set(&DataKey::Balance(to), &(bal + amount));
        Ok(())
    }

    // -----------------------------------------------------------------------
    // SEP-41 read-only
    // -----------------------------------------------------------------------

    pub fn decimals(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::Decimals)
            .unwrap_or(7)
    }

    pub fn name(env: Env) -> String {
        env.storage()
            .instance()
            .get(&DataKey::Name)
            .unwrap_or_else(|| String::from_str(&env, ""))
    }

    pub fn symbol(env: Env) -> String {
        env.storage()
            .instance()
            .get(&DataKey::Symbol)
            .unwrap_or_else(|| String::from_str(&env, ""))
    }

    pub fn balance(env: Env, id: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Balance(id))
            .unwrap_or(0)
    }

    pub fn allowance(env: Env, from: Address, spender: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Allowance(from, spender))
            .unwrap_or(0)
    }

    // -----------------------------------------------------------------------
    // SEP-41 mutating
    // -----------------------------------------------------------------------

    pub fn approve(
        env: Env,
        from: Address,
        spender: Address,
        amount: i128,
        _expiration_ledger: u32,
    ) -> Result<(), Error> {
        from.require_auth();
        env.storage()
            .persistent()
            .set(&DataKey::Allowance(from, spender), &amount);
        Ok(())
    }

    /// Transfer `amount` from `from` to `to`, gated by `denylist-gate`.
    ///
    /// Returns `Err(DeniedByGate)` if either party is on the denylist.
    /// Unlike `allowlist-token`, this uses an error return (not `Ok(false)`)
    /// because denylist blocks should revert all state — a transfer being
    /// silently soft-blocked here is a worse failure mode than a hard abort.
    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) -> Result<(), Error> {
        from.require_auth();
        Self::gate_check(&env, &from, &to)?;
        Self::do_transfer(&env, &from, &to, amount)
    }

    pub fn transfer_from(
        env: Env,
        spender: Address,
        from: Address,
        to: Address,
        amount: i128,
    ) -> Result<(), Error> {
        spender.require_auth();
        Self::gate_check(&env, &from, &to)?;

        let key = DataKey::Allowance(from.clone(), spender.clone());
        let allowance: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        if allowance < amount {
            return Err(Error::InsufficientAllowance);
        }
        env.storage()
            .persistent()
            .set(&key, &(allowance - amount));

        Self::do_transfer(&env, &from, &to, amount)
    }

    pub fn burn(env: Env, from: Address, amount: i128) -> Result<(), Error> {
        from.require_auth();
        let bal = Self::balance(env.clone(), from.clone());
        if bal < amount {
            return Err(Error::InsufficientBalance);
        }
        env.storage()
            .persistent()
            .set(&DataKey::Balance(from), &(bal - amount));
        Ok(())
    }

    pub fn burn_from(
        env: Env,
        spender: Address,
        from: Address,
        amount: i128,
    ) -> Result<(), Error> {
        spender.require_auth();
        let key = DataKey::Allowance(from.clone(), spender.clone());
        let allowance: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        if allowance < amount {
            return Err(Error::InsufficientAllowance);
        }
        env.storage()
            .persistent()
            .set(&key, &(allowance - amount));

        let bal = Self::balance(env.clone(), from.clone());
        if bal < amount {
            return Err(Error::InsufficientBalance);
        }
        env.storage()
            .persistent()
            .set(&DataKey::Balance(from), &(bal - amount));
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    fn gate_check(env: &Env, from: &Address, to: &Address) -> Result<(), Error> {
        let gate_address: Address = env
            .storage()
            .instance()
            .get(&DataKey::Gate)
            .ok_or(Error::NotInitialized)?;
        let gate = GateClient::new(env, &gate_address);
        if !gate.check(from) || !gate.check(to) {
            // Emit a small diagnostic event before returning the error.
            // Unlike allowlist-token's Ok(false) pattern, we let the error
            // revert the call — the event is emitted here before returning
            // so the caller can observe it in test output even though the
            // invocation reverts.
            TransferDenied {
                from: from.clone(),
                to: to.clone(),
            }
            .publish(env);
            return Err(Error::DeniedByGate);
        }
        Ok(())
    }

    fn do_transfer(env: &Env, from: &Address, to: &Address, amount: i128) -> Result<(), Error> {
        let from_bal = Self::balance(env.clone(), from.clone());
        if from_bal < amount {
            return Err(Error::InsufficientBalance);
        }
        let to_bal = Self::balance(env.clone(), to.clone());
        env.storage()
            .persistent()
            .set(&DataKey::Balance(from.clone()), &(from_bal - amount));
        env.storage()
            .persistent()
            .set(&DataKey::Balance(to.clone()), &(to_bal + amount));
        Ok(())
    }

    fn require_admin(env: &Env, admin: &Address) -> Result<(), Error> {
        admin.require_auth();
        let stored: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        if stored != *admin {
            return Err(Error::NotAuthorized);
        }
        Ok(())
    }
}

#[cfg(test)]
mod test;
