#!/usr/bin/env bash
#
# Ethereum Contract Deployment Script
#
# Builds and deploys the Zescrow Escrow contract with Foundry, pinning it to a
# RISC Zero verifier router and the audited guest image id, and exports the ABI
# the client binds against.
#
# Usage:
#   ./deploy/ethereum/run.sh [--network local|sepolia]
#
# Required environment:
#   ZESCROW_VERIFIER  RISC Zero verifier router address for the network.
#   ZESCROW_IMAGE_ID  Audited guest image id (bytes32); from the prover build.
#
# Signing (one of, never hard-code a key):
#   ETHEREUM_KEYSTORE_ACCOUNT  A `cast wallet` keystore account name (--account).
#   ETHEREUM_SENDER_PRIVATE_KEY  A raw key for local/testnet use (--private-key).
#
# Optional:
#   ETHEREUM_RPC_URL   Overrides the default RPC endpoint.
#   ETHERSCAN_API_KEY  Enables source verification on Sepolia.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CONTRACTS_DIR="$PROJECT_ROOT/agent/ethereum"
ABI_OUT="$PROJECT_ROOT/client/abi/Escrow.json"

NETWORK="local"
while [[ $# -gt 0 ]]; do
    case $1 in
        --network)
            NETWORK="$2"
            shift 2
            ;;
        *)
            echo "Unknown option: $1"
            echo "Usage: $0 [--network local|sepolia]"
            exit 1
            ;;
    esac
done

case $NETWORK in
    local | localhost)
        RPC_URL="${ETHEREUM_RPC_URL:-http://localhost:8545}"
        ;;
    sepolia)
        RPC_URL="${ETHEREUM_RPC_URL:-https://eth-sepolia.public.blastapi.io}"
        ;;
    *)
        echo "Error: invalid network '$NETWORK'. Use 'local' or 'sepolia'."
        exit 1
        ;;
esac

: "${ZESCROW_VERIFIER:?Set ZESCROW_VERIFIER to the RISC Zero verifier router address}"
: "${ZESCROW_IMAGE_ID:?Set ZESCROW_IMAGE_ID to the audited guest image id (bytes32)}"

if [[ -n "${ETHEREUM_KEYSTORE_ACCOUNT:-}" ]]; then
    SIGNER=(--account "$ETHEREUM_KEYSTORE_ACCOUNT")
elif [[ -n "${ETHEREUM_SENDER_PRIVATE_KEY:-}" ]]; then
    SIGNER=(--private-key "$ETHEREUM_SENDER_PRIVATE_KEY")
else
    echo "Error: no signer configured."
    echo "Set ETHEREUM_KEYSTORE_ACCOUNT (preferred) or ETHEREUM_SENDER_PRIVATE_KEY."
    exit 1
fi

cd "$CONTRACTS_DIR"

echo "Installing Solidity dependencies..."
forge soldeer install

echo "Building contracts..."
forge build

echo "Exporting ABI to $ABI_OUT..."
printf '{\n  "abi": %s\n}\n' "$(forge inspect Escrow abi --json)" >"$ABI_OUT"

VERIFY=()
if [[ "$NETWORK" == "sepolia" && -n "${ETHERSCAN_API_KEY:-}" ]]; then
    VERIFY=(--verify --etherscan-api-key "$ETHERSCAN_API_KEY")
fi

echo "Deploying to $NETWORK ($RPC_URL)..."
ZESCROW_VERIFIER="$ZESCROW_VERIFIER" ZESCROW_IMAGE_ID="$ZESCROW_IMAGE_ID" \
    forge script script/Deploy.s.sol:Deploy \
    --rpc-url "$RPC_URL" \
    --broadcast \
    "${SIGNER[@]}" \
    "${VERIFY[@]}"

CHAIN_ID="$(cast chain-id --rpc-url "$RPC_URL")"
BROADCAST="$CONTRACTS_DIR/broadcast/Deploy.s.sol/$CHAIN_ID/run-latest.json"
DEPLOYED_ADDRESS="$(grep -o '"contractAddress": *"0x[0-9a-fA-F]\{40\}"' "$BROADCAST" | head -1 | grep -o '0x[0-9a-fA-F]\{40\}')"

echo ""
echo "Deployment complete."
echo "Contract address: ${DEPLOYED_ADDRESS:-see $BROADCAST}"
echo ""
echo "Next steps:"
echo "  1. Add to your .env:"
echo "     ETHEREUM_RPC_URL=$RPC_URL"
echo "     ESCROW_CONTRACT_ADDRESS=${DEPLOYED_ADDRESS:-<address>}"
echo "  2. Copy the escrow parameters template:"
echo "     cp deploy/ethereum/escrow_params.json deploy/"
echo "  3. Create an escrow:"
echo "     cargo run --release -p zescrow-client -- create"
