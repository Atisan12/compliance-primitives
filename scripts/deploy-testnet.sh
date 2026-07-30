#!/usr/bin/env bash
# Build all workspace contracts and deploy one of them to testnet, printing
# the resulting contract ID.
#
# Usage:
#   scripts/deploy-testnet.sh <allowlist-token|denylist-gate|jurisdiction-flag|rwa-token>
#
# For the full RWA stack (all three primitives + rwa-token + initialize),
# prefer scripts/deploy-rwa-testnet.sh instead.
#
# The source testnet identity is read from $STELLAR_SOURCE (an identity
# already set up via `stellar keys generate`/`stellar keys address`),
# defaulting to "default" if unset.
#
# Requires stellar-cli compatible with soroban-sdk 27 (cli ≥ 23 / ideally 27.x).
set -euo pipefail

usage() {
  echo "Usage: $0 <allowlist-token|denylist-gate|jurisdiction-flag|rwa-token>" >&2
  exit 1
}

if [ "$#" -ne 1 ]; then
  usage
fi

CONTRACT_NAME="$1"

case "$CONTRACT_NAME" in
  allowlist-token | denylist-gate | jurisdiction-flag | rwa-token) ;;
  *)
    echo "error: unknown contract '$CONTRACT_NAME'" >&2
    usage
    ;;
esac

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

WASM_NAME="${CONTRACT_NAME//-/_}.wasm"
WASM_PATH="target/wasm32v1-none/release/${WASM_NAME}"
SOURCE_IDENTITY="${STELLAR_SOURCE:-default}"

echo "==> Building all contracts to wasm..."
stellar contract build

if [ ! -f "$WASM_PATH" ]; then
  echo "error: expected build artifact not found at $WASM_PATH" >&2
  exit 1
fi

echo "==> Deploying ${CONTRACT_NAME} (${WASM_PATH}) to testnet with source '${SOURCE_IDENTITY}'..."
CONTRACT_ID="$(stellar contract deploy \
  --wasm "$WASM_PATH" \
  --source "$SOURCE_IDENTITY" \
  --network testnet)"

echo "${CONTRACT_ID}"
