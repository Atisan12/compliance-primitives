SEP: 0000 (replace with assigned number)
Title: Compliance Check Interface
Author: Stellar Compliance Kit contributors
Status: Draft
Type: Standard
Created: 2025-01-18
Discussion: https://github.com/stellar-compliance-kit/compliance-primitives/issues/125
```

# SEP-0000: Compliance Check Interface

## Abstract

This SEP defines a minimal, standardized interface (`is_compliant`) that any
compliance-gate contract on Stellar may implement, so that token contracts,
policy engines, indexers, and other tooling can call a compliance check
generically — regardless of whether the underlying gate is an allowlist, a
denylist, a jurisdiction flag, or any future compliance primitive — without
having to know that contract's specific function names or argument shapes.

## Motivation

Today, three compliance-primitive contracts exist in the
[`compliance-primitives`](https://github.com/stellar-compliance-kit/compliance-primitives)
workspace:

| Contract | Check function | Signature | Meaning of `true` |
|----------|---------------|-----------|-------------------|
| `denylist-gate` | `check` | `(Env, Address) -> bool` | Address is NOT denied |
| `allowlist-token` | `is_allowed` | `(Env, Address) -> bool` | Address is allowlisted |
| `jurisdiction-flag` | `is_permitted_jurisdiction` | `(Env, Address, Vec<String>) -> bool` | Address's jurisdiction is in the allowed set |

A consuming contract (or tooling such as a policy engine or an indexer) that
wants to verify an address before permitting a transfer must know which of
these three contracts it is calling and use the correct function name and
argument shape. There is no shared trait or interface that all compliance
gates implement — the opposite of how SEP-41 standardizes the token
interface itself.

This forces all consumers to either:
- Hard-code per-gate logic, making composition across different gate types
  impossible without contract-level orchestration.
- Maintain an adapter layer off-chain that maps abstract compliance queries
  to each gate's concrete ABI.

Neither approach scales to an ecosystem where multiple issuers deploy
different gate types, and new gate types may be created in the future.
Standardizing the compliance-check interface — even as a single function —
lets the ecosystem treat any compliance gate as a black box that answers
"is this address permitted?" and compose against it generically.

## Specification

### Core Interface

Every compliance-gate contract that wishes to conform to this SEP **SHOULD**
expose the following function:

```rust
#![no_std]

use soroban_sdk::{Address, Bytes, Env};

/// Returns `true` if `addr` is permitted to transact under the compliance
/// rules of this gate, given the optional `context` blob.
///
/// `context` is an opaque, gate-specific payload. A gate that needs no
/// extra information (e.g. a simple allowlist or denylist) MUST ignore it.
/// A gate that needs additional parameters (e.g. a set of allowed
/// jurisdiction codes) MUST define its own encoding convention and document
/// it. Consumers that do not understand a particular gate's `context`
/// encoding SHOULD pass `None`.
fn is_compliant(env: Env, addr: Address, context: Option<Bytes>) -> bool;
```

The function:
- **MUST** be a read-only call (no storage writes, no authentication
  requirements).
- **MUST** return a `bool`: `true` if the address is compliant (permitted to
  transact), `false` otherwise.
- **MUST NOT** panic or trap; if the gate is misconfigured or the `context`
  cannot be decoded, it **SHOULD** return `false` rather than fail.
- **MUST** accept `context` as `Option<Bytes>` (variable-length). Gates that
  do not need context **MUST** accept it and ignore it.
- **SHOULD** be idempotent — multiple calls with the same arguments and
  same ledger state **SHOULD** return the same result.

### Soroban Trait Definition

For consumers that compile against Rust/Soroban, the interface is expressed
as a Soroban trait suitable for use with `#[contractclient]`:

```rust
use soroban_sdk::{contractclient, Address, Bytes, Env};

#[contractclient(name = "ComplianceCheckClient")]
pub trait ComplianceCheckInterface {
    fn is_compliant(env: Env, addr: Address, context: Option<Bytes>) -> bool;
}
```

A consumer contract can then call any SEP-compliant gate without knowing its
concrete type:

```rust
fn transfer(env: Env, from: Address, to: Address, amount: i128) -> Result<(), Error> {
    from.require_auth();

    let gate_client = ComplianceCheckClient::new(&env, &gate_address);
    if !gate_client.is_compliant(&from, &None) {
        return Err(Error::ComplianceBlocked);
    }
    if !gate_client.is_compliant(&to, &None) {
        return Err(Error::ComplianceBlocked);
    }
    // Proceed with transfer...
    Ok(())
}
```

### Context Encoding

Gates that require additional parameters **MUST** document their `context`
encoding scheme. The following encoding conventions are **RECOMMENDED** for
common patterns:

| Gate type | `context` encoding | Example |
|-----------|-------------------|---------|
| Simple allowlist/denylist | `None` (ignored) | — |
| Jurisdiction flag | SCVal-encoded `Vec<String>` of allowed jurisdiction codes via `Bytes::from_slice` containing the XDR of the `Vec<String>` | `Bytes` from `env.scval_to_bytes(vec_env_val)` |

Gates **SHOULD NOT** require `context` for their basic operation; if a gate
can reasonably answer a yes/no check on an address alone, it **SHOULD**
accept `None`. The `context` parameter exists only for gates like
`jurisdiction-flag` where the caller must supply additional data (the set of
permitted jurisdictions) to get a meaningful answer.

## Backward Compatibility

The three existing contracts in the `compliance-primitives` workspace each
retain their existing public function signatures unchanged. Conformance to
this SEP is achieved by adding a single new public function —
`is_compliant(Env, Address, Option<Bytes>) -> bool` — that delegates to the
existing check function.

### `denylist-gate`

```rust
// New function — does not alter existing `check()`.
pub fn is_compliant(env: Env, addr: Address, _context: Option<Bytes>) -> bool {
    Self::check(env, addr)
}
```

The `context` parameter is ignored because the check is binary: an address
is either denied or it is not.

### `allowlist-token`

```rust
// New function — does not alter existing `is_allowed()`.
pub fn is_compliant(env: Env, addr: Address, _context: Option<Bytes>) -> bool {
    Self::is_allowed(env, addr)
}
```

As with the denylist gate, the check is binary (address is either on the
allowlist or not), so `context` is ignored.

### `jurisdiction-flag`

```rust
// New function — does not alter existing `is_permitted_jurisdiction()`.
// Callers must encode the allowed jurisdiction codes into `context`.
pub fn is_compliant(env: Env, addr: Address, context: Option<Bytes>) -> bool {
    match context {
        Some(bytes) => {
            // Decode the Bytes into a Vec<String> of allowed codes.
            // The exact decoding depends on the encoding convention chosen
            // (see "Context Encoding" above). Pseudocode:
            let allowed_codes: Vec<String> = decode_allowed_codes(&env, &bytes);
            Self::is_permitted_jurisdiction(env, addr, allowed_codes)
        }
        None => false, // No jurisdiction info provided — cannot approve.
    }
}
```

This gate is the primary reason `context` exists: unlike the other two, it
cannot answer a yes/no check on an address alone; it needs the caller to
specify which jurisdictions are permitted.

### Backward Compatibility Guarantee

All three contracts guarantee that:
1. Existing public functions (`check`, `is_allowed`, `is_permitted_jurisdiction`,
   `transfer`, `add_to_allowlist`, `remove_from_allowlist`, `add_to_denylist`,
   `remove_from_denylist`, `set_jurisdiction`, `get_jurisdiction`, etc.) retain
   their exact signatures and semantics.
2. Existing deployed instances will continue to work without re-deployment;
   the `is_compliant` function is added in a new contract version.
3. Storage layouts are unchanged.

## Security Considerations

### Context Injection

The `context` parameter is caller-supplied. A gate that interprets `context`
**MUST NOT** trust it as authoritative — it is merely a parameter to the
check. The gate's own storage (e.g., the jurisdiction record for an address)
is the source of truth. A malicious caller cannot bypass a compliance check
by manipulating `context`; they can only cause the gate to answer `false` by
supplying malformed or empty context.

### Reentrancy

`is_compliant` is a read-only function that performs no storage writes and
requires no authentication. It **SHOULD NOT** make cross-contract calls that
could re-enter the caller. Implementations **SHOULD** restrict themselves to
reading their own storage and returning a boolean.

### Front-Running

A compliance gate's answer depends on its current storage state. An admin
that adds or removes an address from an allowlist/denylist between the
moment a transaction is submitted and the moment it is executed can change
the result of `is_compliant`. This is inherent to on-chain state and is not
specific to this interface. Consumers **SHOULD** treat the compliance check
result as valid only for the ledger sequence in which it was executed.

### No Authentication

`is_compliant` **MUST NOT** require authentication. It is a public read-only
view into the gate's state. Requiring authentication would prevent
consumers (which may be other contracts) from calling it without the target
address's cooperation.

### Divergent Semantics

Different gates may define "compliant" in subtly different ways:
- A denylist gate says "compliant" = "not on the deny list".
- An allowlist gate says "compliant" = "on the allow list".
- A jurisdiction gate says "compliant" = "has a jurisdiction in the allowed set".

Consumers **MUST** understand the semantics of the specific gate they are
calling; the standardized interface removes the need to know the function
name and signature, but not the need to understand what the gate checks.
The `context` parameter and the gate's documentation are the mechanisms for
communicating gate-specific semantics.

## Examples

### Consumer Contract Using the Standardized Interface

```rust
#![no_std]

use soroban_sdk::{
    contract, contractclient, contractimpl, contracttype, Address, Bytes, Env,
};

#[contractclient(name = "ComplianceCheckClient")]
pub trait ComplianceCheckInterface {
    fn is_compliant(env: Env, addr: Address, context: Option<Bytes>) -> bool;
}

#[contracttype]
#[derive(Clone)]
enum DataKey {
    Gate,
    Token,
}

#[contract]
pub struct CompliantToken;

#[contractimpl]
impl CompliantToken {
    pub fn initialize(env: Env, gate: Address, token: Address) {
        env.storage().instance().set(&DataKey::Gate, &gate);
        env.storage().instance().set(&DataKey::Token, &token);
    }

    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) -> Result<bool, ()> {
        from.require_auth();

        let gate: Address = env.storage().instance().get(&DataKey::Gate).unwrap();
        let gate_client = ComplianceCheckClient::new(&env, &gate);

        if !gate_client.is_compliant(&from, &None) {
            return Ok(false);
        }
        if !gate_client.is_compliant(&to, &None) {
            return Ok(false);
        }

        let token: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        let token_client = soroban_sdk::token::Client::new(&env, &token);
        token_client.transfer(&from, &to, &amount);
        Ok(true)
    }
}
```

### Indexer / Off-Chain Tooling

An off-chain indexer that monitors compliance events can use a single
function signature to query any compliant gate:

```
GET /contract/{gate_id}/is_compliant?addr={address}
```

The indexer does not need to know whether `gate_id` is an allowlist,
denylist, or jurisdiction flag — it simply calls `is_compliant` and
interprets the boolean result. For jurisdiction gates, the off-chain client
would need to encode the allowed jurisdiction codes into the `context`
parameter using the gate's documented encoding scheme.

## Design Rationale

### Why `is_compliant` instead of `check`?

The name `check` is already used by `denylist-gate` with slightly different
semantics (returns `true` when the address is *not* denied — i.e., clear to
transact). `is_compliant` is more descriptive of the abstraction and less
likely to collide with existing function names across the ecosystem.

### Why `Option<Bytes>` for context instead of separate parameters?

A separate parameter for each possible gate type (e.g., `allowed_jurisdictions:
Vec<String>`) would defeat the purpose of standardization: consumers would
need to know which parameters to pass for which gate. An opaque blob keeps
the interface minimal and extensible. Gates that need structured data encode
it themselves and document the encoding.

### Why not a Result type?

A compliance check is fundamentally a yes/no question. Returning `Result` would
imply that the gate can fail in a way that is not simply "not compliant,"
which would force consumers to handle error cases that should never occur in
normal operation. If a gate is misconfigured, returning `false` (not compliant)
is the safer default — it causes the consumer to deny the operation rather than
proceed with a potentially incorrect approval.

### Why not include the address being checked in the context?

The `addr` parameter is mandatory and always present. The `context` is for
information that the caller supplies about the *operation* (e.g., which
jurisdictions are permitted), not about the *subject* of the check.

## Reference Implementation

The `compliance-primitives` workspace will add `is_compliant` to each of the
three contracts in a future PR, once (and if) this SEP gains community
consensus. The reference implementation will follow the adapter pattern
described in the Backward Compatibility section above.

## Changelog

| Version | Date | Notes |
|---------|------|-------|
| 1.0.0   | 2025-01-18 | Initial draft |
