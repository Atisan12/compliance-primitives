#!/usr/bin/env bash
# Build and deploy the full RWA reference stack to Stellar testnet:
#   allowlist-token, denylist-gate, jurisdiction-flag, rwa-token
#
# Prints a shell-friendly summary of all contract IDs. Redeploy after a
# testnet reset by re-running this script and updating TESTNET.md.
#
# Usage:
#   STELLAR_SOURCE=<identity> ./scripts/deploy-rwa-testnet.sh
#
# Optional:
#   STELLAR_NETWORK=testnet   (default)
#   ALLOWED_CODES='["US","CA"]'  — JSON array passed to rwa-token initialize
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

SOURCE_IDENTITY="${STELLAR_SOURCE:-default}"
NETWORK="${STELLAR_NETWORK:-testnet}"
ALLOWED_CODES="${ALLOWED_CODES:-[\"US\",\"CA\"]}"

need_wasm() {
  local path="$1"
  if [ ! -f "$path" ]; then
    echo "error: expected build artifact not found at $path" >&2
    exit 1
  fi
}

deploy_one() {
  local label="$1"
  local wasm="$2"
  echo "==> Deploying ${label}..." >&2
  stellar contract deploy \
    --wasm "$wasm" \
    --source "$SOURCE_IDENTITY" \
    --network "$NETWORK"
}

echo "==> Building workspace contracts to wasm..."
stellar contract build

ALLOWLIST_WASM="target/wasm32v1-none/release/allowlist_token.wasm"
GATE_WASM="target/wasm32v1-none/release/denylist_gate.wasm"
JURISDICTION_WASM="target/wasm32v1-none/release/jurisdiction_flag.wasm"
RWA_WASM="target/wasm32v1-none/release/rwa_token.wasm"

need_wasm "$ALLOWLIST_WASM"
need_wasm "$GATE_WASM"
need_wasm "$JURISDICTION_WASM"
need_wasm "$RWA_WASM"

ALLOWLIST_ID="$(deploy_one allowlist-token "$ALLOWLIST_WASM")"
GATE_ID="$(deploy_one denylist-gate "$GATE_WASM")"
JURISDICTION_ID="$(deploy_one jurisdiction-flag "$JURISDICTION_WASM")"
RWA_ID="$(deploy_one rwa-token "$RWA_WASM")"

# Placeholder underlying token address for allowlist-token.initialize —
# rwa-token only calls is_allowed(), so any valid address works. We reuse
# the allowlist contract id itself as a stable placeholder.
UNDERLYING_PLACEHOLDER="$ALLOWLIST_ID"

ISSUER="$(stellar keys address "$SOURCE_IDENTITY")"

echo "==> Initializing allowlist-token..."
stellar contract invoke \
  --id "$ALLOWLIST_ID" \
  --source "$SOURCE_IDENTITY" \
  --network "$NETWORK" \
  -- \
  initialize \
  --admin "$ISSUER" \
  --token "$UNDERLYING_PLACEHOLDER"

echo "==> Initializing denylist-gate..."
stellar contract invoke \
  --id "$GATE_ID" \
  --source "$SOURCE_IDENTITY" \
  --network "$NETWORK" \
  -- \
  initialize \
  --admin "$ISSUER"

echo "==> Initializing jurisdiction-flag..."
stellar contract invoke \
  --id "$JURISDICTION_ID" \
  --source "$SOURCE_IDENTITY" \
  --network "$NETWORK" \
  -- \
  initialize \
  --issuer "$ISSUER"

echo "==> Initializing rwa-token..."
stellar contract invoke \
  --id "$RWA_ID" \
  --source "$SOURCE_IDENTITY" \
  --network "$NETWORK" \
  -- \
  initialize \
  --allowlist "$ALLOWLIST_ID" \
  --gate "$GATE_ID" \
  --jurisdiction "$JURISDICTION_ID" \
  --allowed_codes "$ALLOWED_CODES"

cat <<EOF

============================================================
RWA testnet reference deployment ($NETWORK)
============================================================
Issuer / admin:     $ISSUER
allowlist-token:    $ALLOWLIST_ID
denylist-gate:      $GATE_ID
jurisdiction-flag:  $JURISDICTION_ID
rwa-token:          $RWA_ID
allowed_codes:      $ALLOWED_CODES
============================================================
Copy these IDs into examples/rwa-token/TESTNET.md after deploy.
EOF
