#!/usr/bin/env bash
# =============================================================================
# examples/allowlist-token-usage/run.sh
#
# End-to-end walkthrough of allowlist-token: initialize the contract, manage
# the allowlist, observe a successful transfer and a blocked one (with the
# Blocked event), then clean up.
#
# HOW TO RUN
# ----------
#   Prerequisites:
#     1. The Stellar CLI ("stellar") v22+  — https://developers.stellar.org/docs/tools/stellar-cli
#     2. A local Soroban network running:
#          stellar network start local
#        (or use the README's Quick start docker-compose approach)
#     3. Two funded identities in your local keystore:
#          stellar keys generate --default-seed alice
#          stellar keys generate --default-seed bob
#          stellar keys generate --default-seed admin
#        Fund them via the local friendbot:
#          stellar keys fund alice --network local
#          stellar keys fund bob   --network local
#          stellar keys fund admin --network local
#     4. Build the contracts first:
#          stellar contract build
#
#   Then run:
#          bash examples/allowlist-token-usage/run.sh
#
# WHAT THIS SCRIPT DOES
# ---------------------
#   Step 1 – Deploy a minimal SEP-41 token (the built-in Stellar Asset Contract
#            wrapper or any SEP-41 conformant WASM) as the underlying token.
#   Step 2 – Deploy allowlist-token pointing at the underlying token.
#   Step 3 – Admin adds Alice to the allowlist.
#   Step 4 – Alice tries to transfer to Bob (Bob is NOT yet allowlisted):
#            the transfer returns false and emits a Blocked event.
#   Step 5 – Admin adds Bob to the allowlist.
#   Step 6 – Alice transfers to Bob successfully; the underlying token moves
#            funds and no Blocked event is emitted.
#
# EXPECTED OUTPUT
# ---------------
#   The script prints the contract IDs, CLI return values, and raw event JSON
#   at each step so you can follow along.
# =============================================================================
set -euo pipefail

NETWORK="local"
WASM_DIR="target/wasm32v1-none/release"

echo "=== Step 0: resolve identities ==="
ADMIN=$(stellar keys address admin)
ALICE=$(stellar keys address alice)
BOB=$(stellar keys address bob)
echo "admin : $ADMIN"
echo "alice : $ALICE"
echo "bob   : $BOB"

# ---------------------------------------------------------------------------
echo ""
echo "=== Step 1: deploy underlying SEP-41 token (Stellar Asset Contract) ==="
# We use the built-in SAC for a test asset issued by admin for simplicity.
# Replace this with any SEP-41-conformant WASM if you have one available.
TOKEN_ID=$(stellar contract asset deploy \
  --asset "USDC:$ADMIN" \
  --source admin \
  --network "$NETWORK")
echo "underlying token contract id: $TOKEN_ID"

# Mint some USDC to Alice so she has funds to transfer.
stellar contract invoke \
  --id "$TOKEN_ID" \
  --source admin \
  --network "$NETWORK" \
  -- mint \
  --to "$ALICE" \
  --amount 1000

echo "Minted 1000 USDC to Alice"

# ---------------------------------------------------------------------------
echo ""
echo "=== Step 2: deploy allowlist-token ==="
ALLOWLIST_ID=$(stellar contract deploy \
  --wasm "$WASM_DIR/allowlist_token.wasm" \
  --source admin \
  --network "$NETWORK")
echo "allowlist-token contract id: $ALLOWLIST_ID"

stellar contract invoke \
  --id "$ALLOWLIST_ID" \
  --source admin \
  --network "$NETWORK" \
  -- initialize \
  --admin "$ADMIN" \
  --token "$TOKEN_ID"
echo "allowlist-token initialized"

# ---------------------------------------------------------------------------
echo ""
echo "=== Step 3: admin adds Alice to the allowlist ==="
stellar contract invoke \
  --id "$ALLOWLIST_ID" \
  --source admin \
  --network "$NETWORK" \
  -- add_to_allowlist \
  --admin "$ADMIN" \
  --address "$ALICE"
echo "Alice allowlisted"

# ---------------------------------------------------------------------------
echo ""
echo "=== Step 4: Alice tries to transfer 200 USDC to Bob (Bob NOT allowlisted) ==="
# Expected: returns false; Blocked event emitted; underlying token NOT called.
TRANSFER_RESULT=$(stellar contract invoke \
  --id "$ALLOWLIST_ID" \
  --source alice \
  --network "$NETWORK" \
  -- transfer \
  --from "$ALICE" \
  --to "$BOB" \
  --amount 200)
echo "transfer result (should be false): $TRANSFER_RESULT"

echo "-- events for this transaction (look for 'blocked') --"
stellar contract events \
  --id "$ALLOWLIST_ID" \
  --network "$NETWORK" \
  --limit 5 || true   # non-fatal if event indexing is not enabled locally

# ---------------------------------------------------------------------------
echo ""
echo "=== Step 5: admin adds Bob to the allowlist ==="
stellar contract invoke \
  --id "$ALLOWLIST_ID" \
  --source admin \
  --network "$NETWORK" \
  -- add_to_allowlist \
  --admin "$ADMIN" \
  --address "$BOB"
echo "Bob allowlisted"

# ---------------------------------------------------------------------------
echo ""
echo "=== Step 6: Alice transfers 200 USDC to Bob (both allowlisted — should succeed) ==="
TRANSFER_RESULT=$(stellar contract invoke \
  --id "$ALLOWLIST_ID" \
  --source alice \
  --network "$NETWORK" \
  -- transfer \
  --from "$ALICE" \
  --to "$BOB" \
  --amount 200)
echo "transfer result (should be true): $TRANSFER_RESULT"

echo ""
echo "=== Done! ==="
echo "Verify balances via:"
echo "  stellar contract invoke --id $TOKEN_ID --source alice --network $NETWORK -- balance --id $ALICE"
echo "  stellar contract invoke --id $TOKEN_ID --source alice --network $NETWORK -- balance --id $BOB"
