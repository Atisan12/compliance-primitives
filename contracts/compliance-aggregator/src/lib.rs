//! `compliance-aggregator` is a `#![no_std]` Soroban contract that batches
//! multiple compliance checks into a single cross-contract call surface.
//!
//! ## Motivation
//!
//! A consuming contract running both a `denylist-gate` check and a
//! `jurisdiction-flag` check today pays two separate host-level cross-contract
//! call overheads per transfer. This aggregator reduces that to **one**
//! call from the consumer's perspective: the consumer calls
//! `check_address` (or the batched `check_all`) on this contract and gets
//! back a single aggregated pass/fail result plus a per-check breakdown for
//! audit purposes.
//!
//! ## Relationship to #109 (`PolicyEngine`)
//!
//! Issue #109 proposes a `PolicyEngine` capable of expressing AND/OR policy
//! logic over an arbitrary set of predicates. This contract deliberately does
//! *not* re-implement that: all checks here are AND-composed (an address must
//! pass every registered check to be permitted), and there is no support for
//! OR branches or rule weights. The two concerns are complementary:
//!
//! - **`compliance-aggregator`** (this contract): reduces host-level
//!   cross-contract call overhead for common AND-combinations of the existing
//!   primitives. Simple, predictable, auditable.
//! - **`PolicyEngine`** (#109): expresses richer boolean policy DAGs over
//!   an arbitrary set of check predicates. Should that contract land, its
//!   `evaluate` entrypoint could call `compliance-aggregator.check_address`
//!   as one of its predicates, or the two could be merged — that decision
//!   belongs in #109's PR.
//!
//! ## Contract interfaces used
//!
//! Cross-contract calls are made via `#[contractclient]` traits declared in
//! this file. This follows the same pattern as
//! `/examples/denylist-gate-consumer`: we describe only the shape of the
//! remote call and do **not** import the peer crates as `[dependencies]`
//! (doing so would pull both contracts' wasm exports into this binary and
//! cause linker conflicts). The peer crates appear only in
//! `[dev-dependencies]` so their test utilities are available in `cfg(test)`.
//!
//! ## Configured checks
//!
//! The admin registers up to one address for each of the two supported check
//! types:
//!
//! - `DenylistCheck` — calls `denylist-gate.check(address) -> bool`
//! - `JurisdictionCheck` — calls
//!   `jurisdiction-flag.is_permitted_jurisdiction(address, allowed_codes) -> bool`
//!
//! Both checks are optional; if a check type's contract address has not been
//! registered the check is skipped (treated as passing). Registering at least
//! one check is enforced at call time.
#![no_std]

use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, Address, Env, String, Vec,
};

// ---------------------------------------------------------------------------
// Cross-contract client interfaces
// ---------------------------------------------------------------------------

/// Subset of the `denylist-gate` interface that this aggregator uses.
/// Must match the actual contract's exported function signature exactly.
#[soroban_sdk::contractclient(name = "DenylistGateClient")]
pub trait DenylistGateInterface {
    fn check(env: Env, address: Address) -> bool;
}

/// Subset of the `jurisdiction-flag` interface that this aggregator uses.
/// Must match the actual contract's exported function signature exactly.
#[soroban_sdk::contractclient(name = "JurisdictionFlagClient")]
pub trait JurisdictionFlagInterface {
    fn is_permitted_jurisdiction(env: Env, address: Address, allowed_codes: Vec<String>) -> bool;
}

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone)]
enum DataKey {
    /// The admin address that may reconfigure the registered checks.
    Admin,
    /// Address of the `denylist-gate` contract to call, if any.
    DenylistGate,
    /// Address of the `jurisdiction-flag` contract to call, if any.
    JurisdictionFlag,
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// The result of a single compliance check on a single address.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckResult {
    /// Human-readable label identifying which check produced this result.
    pub check: CheckKind,
    /// `true` if the address passed the check, `false` if it failed.
    pub passed: bool,
}

/// Identifies which compliance primitive produced a `CheckResult`.
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CheckKind {
    Denylist = 0,
    Jurisdiction = 1,
}

/// Per-address aggregated result returned by `check_all`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddressCheckResult {
    pub address: Address,
    /// `true` iff every registered check passed for this address.
    pub all_passed: bool,
    /// Individual results in registration order (denylist first, then jurisdiction).
    pub checks: Vec<CheckResult>,
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

#[contractevent]
pub struct AdminSet {
    #[topic]
    pub admin: Address,
}

#[contractevent]
pub struct DenylistGateSet {
    #[topic]
    pub gate: Address,
}

#[contractevent]
pub struct JurisdictionFlagSet {
    #[topic]
    pub flag: Address,
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
    NoChecksRegistered = 4,
    EmptyAddressList = 5,
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct ComplianceAggregator;

#[contractimpl]
impl ComplianceAggregator {
    // -----------------------------------------------------------------------
    // Lifecycle
    // -----------------------------------------------------------------------

    /// One-time setup. `admin` is the only address that may later call
    /// `set_denylist_gate`, `set_jurisdiction_flag`, or `set_admin`.
    ///
    /// `denylist_gate` and `jurisdiction_flag` are optional at initialization;
    /// pass `None` for either one you don't want to register yet.
    pub fn initialize(
        env: Env,
        admin: Address,
        denylist_gate: Option<Address>,
        jurisdiction_flag: Option<Address>,
    ) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        AdminSet {
            admin: admin.clone(),
        }
        .publish(&env);

        if let Some(gate) = denylist_gate {
            env.storage().instance().set(&DataKey::DenylistGate, &gate);
            DenylistGateSet { gate }.publish(&env);
        }
        if let Some(flag) = jurisdiction_flag {
            env.storage()
                .instance()
                .set(&DataKey::JurisdictionFlag, &flag);
            JurisdictionFlagSet { flag }.publish(&env);
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Admin management
    // -----------------------------------------------------------------------

    /// Replace the admin. Old admin must authorize.
    pub fn set_admin(env: Env, admin: Address, new_admin: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Admin, &new_admin);
        AdminSet { admin: new_admin }.publish(&env);
        Ok(())
    }

    /// Register or replace the `denylist-gate` contract address. Admin-only.
    pub fn set_denylist_gate(env: Env, admin: Address, gate: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::DenylistGate, &gate);
        DenylistGateSet { gate }.publish(&env);
        Ok(())
    }

    /// Register or replace the `jurisdiction-flag` contract address. Admin-only.
    pub fn set_jurisdiction_flag(env: Env, admin: Address, flag: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage()
            .instance()
            .set(&DataKey::JurisdictionFlag, &flag);
        JurisdictionFlagSet { flag }.publish(&env);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Read helpers
    // -----------------------------------------------------------------------

    /// Returns the currently registered `denylist-gate` address, if any.
    pub fn denylist_gate(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::DenylistGate)
    }

    /// Returns the currently registered `jurisdiction-flag` address, if any.
    pub fn jurisdiction_flag(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::JurisdictionFlag)
    }

    // -----------------------------------------------------------------------
    // Compliance checks
    // -----------------------------------------------------------------------

    /// Run all registered compliance checks against `address` in a **single**
    /// call from the consumer's perspective.
    ///
    /// - `allowed_jurisdictions`: the set of permitted jurisdiction codes
    ///   passed through to `jurisdiction-flag.is_permitted_jurisdiction`.
    ///   Ignored (and not forwarded) if no jurisdiction-flag contract is
    ///   registered.
    ///
    /// Returns `(all_passed, checks)` where:
    /// - `all_passed` is `true` iff every registered check returned `true`.
    /// - `checks` is the per-primitive result list in deterministic order
    ///   (denylist first, then jurisdiction) for auditability.
    ///
    /// Panics with `Error::NoChecksRegistered` if neither gate nor flag
    /// has been configured — calling an aggregator with zero checks is almost
    /// certainly a misconfiguration.
    pub fn check_address(
        env: Env,
        address: Address,
        allowed_jurisdictions: Vec<String>,
    ) -> Result<(bool, Vec<CheckResult>), Error> {
        let mut results: Vec<CheckResult> = Vec::new(&env);
        let mut all_passed = true;

        // -- denylist-gate check --
        if let Some(gate_addr) = env
            .storage()
            .instance()
            .get::<DataKey, Address>(&DataKey::DenylistGate)
        {
            let client = DenylistGateClient::new(&env, &gate_addr);
            let passed = client.check(&address);
            all_passed = all_passed && passed;
            results.push_back(CheckResult {
                check: CheckKind::Denylist,
                passed,
            });
        }

        // -- jurisdiction-flag check --
        if let Some(flag_addr) = env
            .storage()
            .instance()
            .get::<DataKey, Address>(&DataKey::JurisdictionFlag)
        {
            let client = JurisdictionFlagClient::new(&env, &flag_addr);
            let passed = client.is_permitted_jurisdiction(&address, &allowed_jurisdictions);
            all_passed = all_passed && passed;
            results.push_back(CheckResult {
                check: CheckKind::Jurisdiction,
                passed,
            });
        }

        if results.is_empty() {
            return Err(Error::NoChecksRegistered);
        }

        Ok((all_passed, results))
    }

    /// Batched variant of `check_address` for multiple addresses in one call.
    ///
    /// Returns one `AddressCheckResult` per input address, in the same order.
    /// `Error::EmptyAddressList` is returned if `addresses` is empty.
    ///
    /// **Overhead note**: each address still requires its own cross-contract
    /// calls to the underlying primitives; Soroban's execution model does not
    /// allow deferring those across addresses. The saving here is the single
    /// invocation from the *consumer* to this aggregator, versus the consumer
    /// having to call each primitive per address itself. For the per-address
    /// primitive calls, Soroban's host caches contract instance reads within
    /// the same transaction so repeated calls to the same gate or flag address
    /// re-use the already-loaded instance.
    pub fn check_all(
        env: Env,
        addresses: Vec<Address>,
        allowed_jurisdictions: Vec<String>,
    ) -> Result<Vec<AddressCheckResult>, Error> {
        if addresses.is_empty() {
            return Err(Error::EmptyAddressList);
        }

        // Resolve contract addresses once, outside the per-address loop, to
        // avoid redundant storage reads.
        let gate_addr: Option<Address> = env
            .storage()
            .instance()
            .get::<DataKey, Address>(&DataKey::DenylistGate);
        let flag_addr: Option<Address> = env
            .storage()
            .instance()
            .get::<DataKey, Address>(&DataKey::JurisdictionFlag);

        if gate_addr.is_none() && flag_addr.is_none() {
            return Err(Error::NoChecksRegistered);
        }

        let mut batch: Vec<AddressCheckResult> = Vec::new(&env);

        for address in addresses.iter() {
            let mut results: Vec<CheckResult> = Vec::new(&env);
            let mut all_passed = true;

            if let Some(ref ga) = gate_addr {
                let client = DenylistGateClient::new(&env, ga);
                let passed = client.check(&address);
                all_passed = all_passed && passed;
                results.push_back(CheckResult {
                    check: CheckKind::Denylist,
                    passed,
                });
            }

            if let Some(ref fa) = flag_addr {
                let client = JurisdictionFlagClient::new(&env, fa);
                let passed =
                    client.is_permitted_jurisdiction(&address, &allowed_jurisdictions);
                all_passed = all_passed && passed;
                results.push_back(CheckResult {
                    check: CheckKind::Jurisdiction,
                    passed,
                });
            }

            batch.push_back(AddressCheckResult {
                address,
                all_passed,
                checks: results,
            });
        }

        Ok(batch)
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

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
