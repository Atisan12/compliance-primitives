# compliance-primitives

On-chain compliance primitives for Stellar — allowlist/denylist token
gating and jurisdiction checks for RWA and stablecoin issuers, built on
Soroban.

[![CI](https://github.com/stellar-compliance-kit/compliance-primitives/actions/workflows/ci.yml/badge.svg)](https://github.com/stellar-compliance-kit/compliance-primitives/actions/workflows/ci.yml)

## The problem

RWA and stablecoin issuers building on Stellar/Soroban all end up needing
the same handful of compliance gates — allowlists, denylists, jurisdiction
restrictions — before they can legally let a token move. Today every issuer
hand-rolls this logic inside their own token contract, which means more
audit surface, more chances for a subtle bug to become a compliance
incident, and no shared, reviewed reference to build on. `compliance-primitives`
exists to give the ecosystem a small set of standardized, auditable,
independently testable contracts for exactly these checks.

## What's in this repo

This is a Cargo workspace with three focused Soroban contracts, each doing
one job:

- **[`allowlist-token`](./contracts/allowlist-token)** — a token wrapper
  that only permits transfers between two addresses that are both present
  on an on-chain allowlist. Admin-managed; forwards cleared transfers to an
  underlying SEP-41 token contract.
- **[`denylist-gate`](./contracts/denylist-gate)** — a standalone denylist
  other contracts call via cross-contract invocation (`check(address)`)
  before executing a transfer. Not a token itself — meant to be composed.
- **[`jurisdiction-flag`](./contracts/jurisdiction-flag)** — attaches an
  issuer-controlled jurisdiction code (e.g. an ISO country code) to an
  address, with a helper (`is_permitted_jurisdiction`) other contracts can
  call to check an address against a permitted-jurisdictions list.

[`/examples/denylist-gate-consumer`](./examples/denylist-gate-consumer) is a
minimal reference token showing the cross-contract calling pattern for
`denylist-gate`. [`/examples/rwa-token`](./examples/rwa-token) composes all
three primitives in one `transfer` path — see its
[TESTNET.md](./examples/rwa-token/TESTNET.md) for the testnet reference
deployment and walkthrough.

## Quick start

```sh
# Clone
git clone https://github.com/stellar-compliance-kit/compliance-primitives.git
cd compliance-primitives

# Run the full test suite (all three contracts + example)
cargo test --workspace

# Lint
cargo clippy --workspace --all-targets -- -D warnings

# Build a contract to wasm (the wasm32v1-none target is pinned in
# rust-toolchain.toml, so rustup installs it automatically)
stellar contract build

# Equivalent, via the cargo alias in .cargo/config.toml
cargo build-wasm

# Deploy a built contract to testnet, e.g. denylist-gate
stellar contract deploy \
  --wasm target/wasm32v1-none/release/denylist_gate.wasm \
  --source <your-testnet-identity> \
  --network testnet
```

See [`examples/testnet.env.example`](./examples/testnet.env.example) for an
example config covering the identity/network setup needed for the deploy +
invoke round trip above.

Alternatively, use [`scripts/deploy-testnet.sh`](./scripts/deploy-testnet.sh) to
build all contracts and deploy one to testnet in a single step:

```sh
STELLAR_SOURCE=<your-testnet-identity> ./scripts/deploy-testnet.sh denylist-gate
```

### Working with a single contract

You don't need to build or test the whole workspace to work on one
contract. Each crate can be built and tested in isolation with Cargo's
`-p <crate>` flag, using the crate name from its `Cargo.toml`
(`allowlist-token`, `denylist-gate`, or `jurisdiction-flag`):

```sh
# Test just one contract
cargo test -p allowlist-token

# Lint just one contract
cargo clippy -p denylist-gate --all-targets -- -D warnings

# Build just one contract to wasm
cargo build -p jurisdiction-flag --target wasm32v1-none --release
```

## Architecture note

These contracts are building blocks, not standalone products. They're
meant to be composed into a real token or RWA wrapper contract via
cross-contract calls — for example, a token's `transfer` function calling
`denylist-gate.check(address)` before moving funds, or checking
`jurisdiction-flag.is_permitted_jurisdiction(address, allowed_codes)`
before permitting a transaction. See
[`/examples/denylist-gate-consumer`](./examples/denylist-gate-consumer) for
a worked example of that pattern. `allowlist-token` is the one exception —
it's a thin wrapper you can deploy in front of an existing SEP-41 token —
but even it delegates the real transfer to the underlying token contract
rather than reimplementing token logic itself.

## Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md) for the fork → branch → PR flow.
This repo is part of the **Drips Wave Stellar Program**, and issues are
labeled by complexity (`complexity: trivial`, `complexity: medium`,
`complexity: high`) so you can find something that matches how deep you
want to go — issues tagged `good first issue` are a good place to start.

## Security

See [SECURITY.md](./SECURITY.md) for how to report a vulnerability.

## License

[MIT](./LICENSE)
