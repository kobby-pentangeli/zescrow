# Zescrow Deployment Guide

Deploy and interact with Zescrow escrows on Solana and Ethereum networks. This guide covers both **local development** (test validators) and **devnet/testnet deployment** (devnet/Sepolia).

## Quick Start

```bash
# 1. Set up environment
cp deploy/.env.template .env
# Edit .env with your configuration

# 2. Deploy (choose network)
./deploy/solana/run.sh --network local          # Local test validator
./deploy/solana/run.sh --network devnet         # Solana devnet
./deploy/ethereum/run.sh --network local        # Local node (anvil)
./deploy/ethereum/run.sh --network sepolia      # Ethereum Sepolia

# 3. Copy the escrow parameters template and edit accordingly
cp deploy/solana/escrow_params.json deploy/    # For Solana
cp deploy/ethereum/escrow_params.json deploy/  # For Ethereum

# 4. Build and create an escrow
cargo build -p zescrow-client
./target/debug/zescrow-client create
```

## Prerequisites

### Solana

- [Agave (Solana) CLI](https://docs.anza.xyz/cli/install) (v3.0.13)
- [Anchor CLI](https://www.anchor-lang.com/docs/installation) (v1.0.2+)
- For devnet: ~3 SOL for deployment (use `solana airdrop`)

### Ethereum

- [Foundry](https://book.getfoundry.sh/getting-started/installation) (`forge`, `cast`, `anvil`)
- For Sepolia: ETH from a [faucet](https://sepoliafaucet.com/)
- For Sepolia: RPC endpoint (Alchemy, Infura, etc.)

## Directory Structure

```sh
deploy/
├── .env.template             # Environment variables (copy to project root)
├── create_recipient_sol.sh   # Helper: create Solana recipient keypair
├── README.md                 # This file
├── solana/
│   ├── escrow_params.json    # Solana config template
│   └── run.sh                # Deployment script (--network local|devnet)
└── ethereum/
    ├── escrow_params.json    # Ethereum config template
    └── run.sh                # Deployment script (--network local|sepolia)

# Generated at runtime (git-ignored):
# ├── escrow_params.json      # Active config (copied from solana/ or ethereum/)
# ├── escrow_conditions.json  # Condition file (output from the 'generate' command)
# └── escrow_metadata.json    # Output from 'create' command
#
# The release proof is generated and submitted in-process during 'finish';
# nothing is written to disk.
```

## Environment Setup

1. Copy the template to project root:

```bash
cp deploy/.env.template .env
```

2. Edit `.env` with your values. The same file works for both local and devnet; just change the RPC URLs:

```bash
# For local development
SOLANA_RPC_URL=http://localhost:8899
ETHEREUM_RPC_URL=http://localhost:8545

# For devnet/Sepolia
SOLANA_RPC_URL=https://api.devnet.solana.com
ETHEREUM_RPC_URL=https://eth-sepolia.g.alchemy.com/v2/YOUR_KEY
```

## Solana Deployment

### Localnet

1. Start the test validator:

```bash
solana config set --url localhost
solana-test-validator -r
```

2. In a new terminal, deploy the program:

```bash
./deploy/solana/run.sh --network local
```

3. Create a recipient keypair and fund it:

```bash
./deploy/create_recipient_sol.sh --network local
```

This creates `deploy/recipient_keypair.json` and outputs the public key. Add to your `.env`:

```bash
SOLANA_RECIPIENT_PUBKEY=<pubkey_from_output>
SOLANA_RECIPIENT_KEYPAIR_PATH=/absolute/path/to/deploy/recipient_keypair.json
```

4. Copy and edit the escrow config template:

```bash
cp deploy/solana/escrow_params.json deploy/
```

5. Create and complete the escrow:

```bash
# Build once (faster iteration)
cargo build -p zescrow-client

# Create escrow (funds are locked)
./target/debug/zescrow-client create

# Release to recipient (after finish_after slot)
# For escrows WITHOUT conditions:
./target/debug/zescrow-client finish --recipient deploy/recipient_keypair.json

# For escrows WITH conditions (ZK proof generation):
cargo run --release -p zescrow-client --features prover -- finish \
  --recipient deploy/recipient_keypair.json

# Or cancel/refund (after cancel_after slot)
./target/debug/zescrow-client cancel
```

### Devnet

1. Configure for devnet and fund your deployer wallet:

```bash
solana config set --url https://api.devnet.solana.com
solana airdrop 5 $(solana address)
```

2. Deploy the program:

```bash
./deploy/solana/run.sh --network devnet
```

3. Create a recipient keypair and fund it:

```bash
./deploy/create_recipient_sol.sh --network devnet
```

This creates `deploy/recipient_keypair.json` and airdrops 1 SOL. Add to your `.env`:

```bash
SOLANA_RECIPIENT_PUBKEY=<pubkey_from_output>
```

4. Copy and edit the escrow config template:

```bash
cp deploy/solana/escrow_params.json deploy/
```

5. Create and complete the escrow:

```bash
# Build once (faster iteration)
cargo build -p zescrow-client

# Create escrow (funds are locked)
./target/debug/zescrow-client create

# Release to recipient (after finish_after slot)
# For escrows WITHOUT conditions:
./target/debug/zescrow-client finish --recipient deploy/recipient_keypair.json

# For escrows WITH conditions (ZK proof generation):
cargo run --release -p zescrow-client --features prover -- finish \
  --recipient deploy/recipient_keypair.json

# Or cancel/refund (after cancel_after slot)
./target/debug/zescrow-client cancel
```

## Ethereum Deployment

### Local (anvil)

1. Start a local node:

```bash
anvil
```

Note the pre-funded accounts and their private keys printed to the console. Pick one for the sender and one for the recipient.

2. In a new terminal, deploy the contract:

```bash
./deploy/ethereum/run.sh --network local
```

3. Configure sender and recipient in your `.env`:

```bash
# Use accounts from the anvil node output (0x prefix optional)
ETHEREUM_SENDER_PRIVATE_KEY=<account_0_private_key>
ETHEREUM_SENDER_ADDRESS=<account_0_address>
ETHEREUM_RECIPIENT_ADDRESS=<account_1_address>
```

Keep the recipient's private key handy for step 6.

4. Copy and edit the escrow config template:

```bash
cp deploy/ethereum/escrow_params.json deploy/
```

5. Create and complete the escrow:

```bash
# Build once (faster iteration)
cargo build -p zescrow-client

# Create escrow (funds are locked)
./target/debug/zescrow-client create

# Release to recipient (after finish_after block)
# For escrows WITHOUT conditions (Ethereum recipient key; 0x optional):
./target/debug/zescrow-client finish --recipient <RECIPIENT_PRIVATE_KEY>

# For escrows WITH conditions (ZK proof generation):
cargo run --release -p zescrow-client --features prover -- finish \
  --recipient <RECIPIENT_PRIVATE_KEY>

# Or cancel/refund (after cancel_after block)
./target/debug/zescrow-client cancel
```

6. (Optional) Mine blocks to advance past `finish_after`:

```bash
# Mine a single block; repeat as needed
cast rpc evm_mine --rpc-url http://localhost:8545
```

### Sepolia (testnet)

1. Set environment variables for deployment:

```bash
export ETHEREUM_SENDER_PRIVATE_KEY="your_private_key"
export ETHERSCAN_API_KEY="your_api_key"  # Optional, for verification
```

2. Deploy the contract:

```bash
./deploy/ethereum/run.sh --network sepolia
```

3. Configure sender and recipient in your `.env`:

```bash
ETHEREUM_RPC_URL=https://eth-sepolia.g.alchemy.com/v2/YOUR_KEY
ESCROW_CONTRACT_ADDRESS=<from_deploy_output>
ETHEREUM_SENDER_PRIVATE_KEY=<your_private_key>
ETHEREUM_SENDER_ADDRESS=<your_address>
ETHEREUM_RECIPIENT_ADDRESS=<recipient_address>
```

4. Copy and edit the escrow config template:

```bash
cp deploy/ethereum/escrow_params.json deploy/
```

5. Create and complete the escrow:

```bash
# Build once (faster iteration)
cargo build -p zescrow-client

# Create escrow (funds are locked)
./target/debug/zescrow-client create

# Release to recipient (after finish_after block)
# For escrows WITHOUT conditions (Ethereum recipient key; 0x optional):
./target/debug/zescrow-client finish --recipient <RECIPIENT_PRIVATE_KEY>

# For escrows WITH conditions (ZK proof generation):
cargo run --release -p zescrow-client --features prover -- finish \
  --recipient <RECIPIENT_PRIVATE_KEY>

# Or cancel/refund (after cancel_after block)
./target/debug/zescrow-client cancel
```

## Cryptographic Conditions

For escrows with ZK conditions, install the [RISC Zero toolchain](https://dev.risczero.com/api/zkvm/quickstart#1-install-the-risc-zero-toolchain).

### Generate Conditions

```bash
# Build once
cargo build -p zescrow-client

# Hashlock (SHA-256 preimage)
./target/debug/zescrow-client generate hashlock --preimage ./preimage.txt

# Ed25519 signature
./target/debug/zescrow-client generate ed25519 \
  --pubkey <hex> --msg <hex> --sig <hex>

# Threshold (M-of-N)
./target/debug/zescrow-client generate threshold \
  --subconditions cond1.json cond2.json cond3.json \
  --threshold 2
```

### Use Conditions

1. Set `"has_conditions": true` in `escrow_params.json`
2. Create escrow (debug build is fine):

```bash
./target/debug/zescrow-client create
```

3. Finish escrow (release build with prover for ZK proof generation):

```bash
cargo run --release -p zescrow-client --features prover -- finish --recipient <KEY>
```

## Configuration Reference

### Environment Variables

| Variable                      | Description                     |
| ----------------------------- | ------------------------------- |
| `SOLANA_RPC_URL`              | Solana RPC endpoint             |
| `SOLANA_PROGRAM_ID`           | Deployed program ID             |
| `SOLANA_SENDER_KEYPAIR_PATH`  | Path to sender's keypair file   |
| `SOLANA_SENDER_PUBKEY`        | Sender's public key (base58)    |
| `SOLANA_RECIPIENT_PUBKEY`     | Recipient's public key (base58) |
| `ETHEREUM_RPC_URL`            | Ethereum RPC endpoint           |
| `ESCROW_CONTRACT_ADDRESS`     | Deployed contract address       |
| `ETHEREUM_SENDER_PRIVATE_KEY` | Sender's private key (no 0x)    |
| `ETHEREUM_SENDER_ADDRESS`     | Sender's address (0x...)        |
| `ETHEREUM_RECIPIENT_ADDRESS`  | Recipient's address (0x...)     |

### escrow_params.json Fields

| Field                            | Description                                     |
| -------------------------------- | ----------------------------------------------- |
| `chain_config.chain`             | `"solana"` or `"ethereum"`                      |
| `chain_config.rpc_url`           | Network RPC endpoint (uses env var)             |
| `chain_config.sender_private_id` | Keypair path (Solana) or private key (Ethereum) |
| `chain_config.agent_id`          | Program ID or contract address                  |
| `asset.kind`                     | `"native"` for SOL/ETH                          |
| `asset.amount`                   | Amount in smallest unit (lamports/wei)          |
| `finish_after`                   | Slot/block after which release is allowed       |
| `cancel_after`                   | Slot/block after which cancel is allowed        |
| `has_conditions`                 | `true` if ZK conditions apply                   |

## Running Tests

### Solana (litesvm harness)

```bash
cd agent/solana/escrow
cargo build-sbf --tools-version v1.53          # Build the program(s) to SBF
cargo test --manifest-path harness/Cargo.toml  # Run the harness
```

### Ethereum (Foundry)

```bash
cd agent/ethereum
forge test -vvv
```
