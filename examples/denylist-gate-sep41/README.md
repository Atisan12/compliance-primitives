# denylist-gate-sep41

A fully SEP-41-conformant token contract that gates every `transfer` and
`transfer_from` call through `denylist-gate`, demonstrating what the
composition pattern looks like when applied to something closer to production
shape.

## How this differs from `denylist-gate-consumer`

| | `denylist-gate-consumer` | `denylist-gate-sep41` |
|---|---|---|
| SEP-41 interface | Minimal (no `approve`, `allowance`, `burn`, metadata) | Full SEP-41 surface |
| Purpose | Illustrate the cross-contract `GateClient` pattern | Show the pattern on a production-shape token |
| Real-world applicability | Low — missing required entry points | High — drop-in once connected to real balances |
| Gate response on denial | `Err(DeniedByGate)` | `Err(DeniedByGate)` (same — revert is correct here) |

## Cross-contract calling pattern (same as `denylist-gate-consumer`)

We deliberately do **not** import the `denylist-gate` crate as a
`[dependencies]` entry.  Doing so would pull its `#[contractimpl]` WASM
exports into this binary and cause a link-time export collision.  Instead, we
declare a `#[contractclient]` trait that mirrors the gate's interface:

```rust
#[contractclient(name = "GateClient")]
pub trait DenylistGateInterface {
    fn check(env: Env, address: Address) -> bool;
}
```

The `denylist-gate` crate appears only in `[dev-dependencies]` so tests can
register a real gate instance to exercise the cross-contract path.

## Running the tests

```sh
# from the workspace root
cargo test -p denylist-gate-sep41

# or from this directory
cargo test
```

## Running against a local network (CLI walkthrough)

```sh
# Build all contracts
stellar contract build

# Start a local network
stellar network start local

# Deploy denylist-gate
GATE_ID=$(stellar contract deploy \
  --wasm target/wasm32v1-none/release/denylist_gate.wasm \
  --source admin --network local)

# Initialize the gate
stellar contract invoke --id $GATE_ID --source admin --network local \
  -- initialize --admin $(stellar keys address admin)

# Deploy and initialize the SEP-41 gated token
TOKEN_ID=$(stellar contract deploy \
  --wasm target/wasm32v1-none/release/denylist_gate_sep41.wasm \
  --source admin --network local)

stellar contract invoke --id $TOKEN_ID --source admin --network local \
  -- initialize \
  --admin $(stellar keys address admin) \
  --gate $GATE_ID \
  --decimal 7 \
  --name "MyToken" \
  --symbol "MTK"

# Mint to Alice, then attempt transfer to Bob (Bob is on the denylist)
stellar contract invoke --id $TOKEN_ID --source admin --network local \
  -- mint --admin $(stellar keys address admin) \
         --to $(stellar keys address alice) --amount 1000

stellar contract invoke --id $GATE_ID --source admin --network local \
  -- add_to_denylist --admin $(stellar keys address admin) \
                     --address $(stellar keys address bob)

stellar contract invoke --id $TOKEN_ID --source alice --network local \
  -- transfer --from $(stellar keys address alice) \
              --to $(stellar keys address bob) --amount 200
# ^ returns Err(DeniedByGate) — transfer reverted
```
