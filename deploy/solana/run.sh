#!/usr/bin/env bash
#
# Solana Program Deployment Script
#
# Deploys the Zescrow escrow program to Solana (localnet or devnet).
#
# Usage:
#   ./deploy/solana/run.sh [--network local|devnet]
#
# Examples:
#   ./deploy/solana/run.sh                      # Defaults to localnet
#   ./deploy/solana/run.sh --network local      # Local test validator
#   ./deploy/solana/run.sh --network devnet     # Devnet
#
# Prerequisites:
#   - Solana CLI installed and configured
#   - Anchor CLI installed (v1.0.2+)
#   - For devnet: sufficient SOL balance for sender/deployer (~3 SOL)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
PROGRAM_DIR="$PROJECT_ROOT/agent/solana/escrow"

# Default to local network
NETWORK="local"

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --network)
            NETWORK="$2"
            shift 2
            ;;
        *)
            echo "Unknown option: $1"
            echo "Usage: $0 [--network local|devnet]"
            exit 1
            ;;
    esac
done

# Set RPC URL based on network
case $NETWORK in
    local)
        RPC_URL="http://localhost:8899"
        CLUSTER="localnet"
        ;;
    devnet)
        RPC_URL="https://api.devnet.solana.com"
        CLUSTER="devnet"
        ;;
    *)
        echo "Error: Invalid network '$NETWORK'. Use 'local' or 'devnet'."
        exit 1
        ;;
esac

echo "Deploying Zescrow to Solana $NETWORK..."
echo "RPC URL: $RPC_URL"

ANCHOR_VERSION=$(anchor --version | grep -oE '[0-9]+\.[0-9]+\.[0-9]+')
echo "Anchor CLI: v$ANCHOR_VERSION"

WALLET=$(solana address)
BALANCE=$(solana balance --url "$RPC_URL" 2>/dev/null || echo "0 SOL")
echo "Deployer: $WALLET"
echo "Balance: $BALANCE"

solana config set --url "$RPC_URL"

cd "$PROGRAM_DIR"

echo ""
echo "Syncing program keys..."
anchor keys sync

echo ""
echo "Building program..."
anchor build

echo ""
echo "Deploying to $NETWORK..."
anchor deploy --provider.cluster "$CLUSTER"

PROGRAM_ID=$(solana-keygen pubkey target/deploy/escrow-keypair.json)

echo ""
echo "Deployment complete!"
echo "Program ID: $PROGRAM_ID"
echo ""
echo "Next steps:"
echo "  1. Add to your .env:"
echo "     SOLANA_RPC_URL=$RPC_URL"
echo "     SOLANA_PROGRAM_ID=$PROGRAM_ID"
echo "  2. Copy the escrow parameters template:"
echo "     cp deploy/solana/escrow_params.json deploy/"
echo "  3. Create an escrow:"
echo "     cargo run --release -p zescrow-client -- create"
