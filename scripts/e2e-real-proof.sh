#!/usr/bin/env bash
#
# Runs the end-to-end harness against real Groth16 proofs.
#
# Continuous integration gates the suite under RISC0_DEV_MODE with mock receipts,
# which needs no GPU or Docker. This script instead produces real receipts, which
# the STARK-to-SNARK wrap requires an x86_64 host with Docker for (or a configured
# remote prover via BONSAI_API_KEY).
#
# The harness deploys a mock verifier so the matrix runs without a deployed RISC
# Zero verifier router; to validate against the live router, deploy the escrow
# contract/program pinned to the real verifier and image id and re-run the suite
# pointed at those deployments.

set -euo pipefail

cd "$(dirname "$0")/.."

arch="$(uname -m)"
if [[ "$arch" != "x86_64" && -z "${BONSAI_API_KEY:-}" ]]; then
  echo "warning: real Groth16 proving needs x86_64 + Docker (got $arch) or BONSAI_API_KEY." >&2
fi
if [[ -z "${BONSAI_API_KEY:-}" ]] && ! command -v docker >/dev/null 2>&1; then
  echo "warning: Docker not found; local Groth16 proving requires it or BONSAI_API_KEY." >&2
fi

echo "Building the Ethereum contract artifacts..."
( cd agent/ethereum && forge soldeer install && forge build )

echo "Building the Solana program artifacts..."
( cd agent/solana/escrow && cargo build-sbf --tools-version v1.53 )

# Real proving: dev mode off, and build the guest (the host crates otherwise
# compile against stub method constants).
unset RISC0_DEV_MODE
export RISC0_SKIP_BUILD=""
: "${RISC0_SERVER_PATH:=$(find "${HOME}/.risc0" -type f -name r0vm 2>/dev/null | head -1)}"
export RISC0_SERVER_PATH

echo "Running the end-to-end matrix with real Groth16 proofs..."
cargo test -p zescrow-e2e -- --ignored --test-threads=1
