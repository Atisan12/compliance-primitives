# RWA token — Stellar testnet reference deployment

Live reference stack for [`rwa-token`](./README.md): all three compliance
primitives plus the composing token, deployed so issuers and contributors
can inspect and invoke without building from scratch.

> **CLI requirement**: contracts are built with `soroban-sdk` 27. Use
> **stellar-cli ≥ 23** (ideally **27.x**) to deploy. Older CLIs fail with
> `xdr value invalid` when reading the wasm.

## Current reference addresses

| Contract | Testnet contract ID |
| --- | --- |
| Issuer / admin (deployer) | _fill after deploy_ |
| `allowlist-token` | _fill after deploy_ |
| `denylist-gate` | _fill after deploy_ |
| `jurisdiction-flag` | _fill after deploy_ |
| `rwa-token` | _fill after deploy_ |
| `allowed_codes` | `["US","CA"]` |

Testnet resets wipe contract state. After a reset (or when these IDs go
stale), re-run the deploy script below and replace the table.

## One-shot redeploy

```sh
# 1. Funded testnet identity (once)
stellar keys generate rwa-ref --network testnet --fund
# or: stellar keys fund rwa-ref --network testnet

# 2. Build + deploy + initialize all four contracts
STELLAR_SOURCE=rwa-ref ./scripts/deploy-rwa-testnet.sh

# 3. Paste the printed IDs into the table above
```

Optional: `ALLOWED_CODES='["US","GB"]' STELLAR_SOURCE=rwa-ref ./scripts/deploy-rwa-testnet.sh`

## Walkthrough (stellar-cli)

Set aliases for the IDs from the table:

```sh
export NETWORK=testnet
export SOURCE=rwa-ref   # must be the issuer/admin used at initialize
export ALLOWLIST=<allowlist-token-id>
export GATE=<denylist-gate-id>
export JURISDICTION=<jurisdiction-flag-id>
export RWA=<rwa-token-id>
export ALICE=<alice-g-address>
export BOB=<bob-g-address>
```

Fund `ALICE` / `BOB` via Friendbot if they are new accounts.

### 1. Onboard both parties (allowlist + jurisdiction)

```sh
stellar contract invoke --id "$ALLOWLIST" --source "$SOURCE" --network "$NETWORK" -- \
  add_to_allowlist --admin "$(stellar keys address $SOURCE)" --address "$ALICE"

stellar contract invoke --id "$ALLOWLIST" --source "$SOURCE" --network "$NETWORK" -- \
  add_to_allowlist --admin "$(stellar keys address $SOURCE)" --address "$BOB"

stellar contract invoke --id "$JURISDICTION" --source "$SOURCE" --network "$NETWORK" -- \
  set_jurisdiction --issuer "$(stellar keys address $SOURCE)" --address "$ALICE" --code '"US"'

stellar contract invoke --id "$JURISDICTION" --source "$SOURCE" --network "$NETWORK" -- \
  set_jurisdiction --issuer "$(stellar keys address $SOURCE)" --address "$BOB" --code '"CA"'
```

### 2. Mint and successful transfer

```sh
stellar contract invoke --id "$RWA" --source "$SOURCE" --network "$NETWORK" -- \
  mint --to "$ALICE" --amount 1000

stellar contract invoke --id "$RWA" --source "$ALICE" --network "$NETWORK" -- \
  transfer --from "$ALICE" --to "$BOB" --amount 400
# → Ok; balances 600 / 400
```

### 3. Blocked: allowlist

Remove Bob from the allowlist, then retry a transfer — expect `NotAllowlisted`:

```sh
stellar contract invoke --id "$ALLOWLIST" --source "$SOURCE" --network "$NETWORK" -- \
  remove_from_allowlist --admin "$(stellar keys address $SOURCE)" --address "$BOB"

stellar contract invoke --id "$RWA" --source "$ALICE" --network "$NETWORK" -- \
  transfer --from "$ALICE" --to "$BOB" --amount 10
# → Error::NotAllowlisted (4)

# restore Bob
stellar contract invoke --id "$ALLOWLIST" --source "$SOURCE" --network "$NETWORK" -- \
  add_to_allowlist --admin "$(stellar keys address $SOURCE)" --address "$BOB"
```

### 4. Blocked: denylist

```sh
stellar contract invoke --id "$GATE" --source "$SOURCE" --network "$NETWORK" -- \
  add_to_denylist --admin "$(stellar keys address $SOURCE)" --address "$ALICE"

stellar contract invoke --id "$RWA" --source "$ALICE" --network "$NETWORK" -- \
  transfer --from "$ALICE" --to "$BOB" --amount 10
# → Error::DeniedByGate (5)

stellar contract invoke --id "$GATE" --source "$SOURCE" --network "$NETWORK" -- \
  remove_from_denylist --admin "$(stellar keys address $SOURCE)" --address "$ALICE"
```

### 5. Blocked: jurisdiction

Flag Bob with a code outside `allowed_codes` (e.g. `IR`):

```sh
stellar contract invoke --id "$JURISDICTION" --source "$SOURCE" --network "$NETWORK" -- \
  set_jurisdiction --issuer "$(stellar keys address $SOURCE)" --address "$BOB" --code '"IR"'

stellar contract invoke --id "$RWA" --source "$ALICE" --network "$NETWORK" -- \
  transfer --from "$ALICE" --to "$BOB" --amount 10
# → Error::JurisdictionNotPermitted (6)

# restore
stellar contract invoke --id "$JURISDICTION" --source "$SOURCE" --network "$NETWORK" -- \
  set_jurisdiction --issuer "$(stellar keys address $SOURCE)" --address "$BOB" --code '"CA"'
```

## Keeping this doc fresh

1. After every testnet reset (or when invokes start failing with missing contract), run `STELLAR_SOURCE=… ./scripts/deploy-rwa-testnet.sh`.
2. Replace the address table with the script output.
3. Commit the updated `TESTNET.md` so the reference stays accurate.
