# Zescrow

[![Crates.io](https://img.shields.io/crates/v/zescrow-core.svg)](https://crates.io/crates/zescrow-core)
[![Documentation](https://docs.rs/zescrow-core/badge.svg)](https://docs.rs/zescrow-core)
[![CI](https://github.com/kobby-pentangeli/zescrow/workflows/CI/badge.svg)](https://github.com/kobby-pentangeli/zescrow/actions)
[![License](https://img.shields.io/crates/l/zescrow.svg)](https://github.com/kobby-pentangeli/zescrow#license)

Zescrow (for zero-knowledge escrow) is a trust-minimized, chain-agnostic implementation of an escrow program using the RISC Zero zkVM as the zero-knowledge prover/verifier.

> [!WARNING]
> **This project is not audited and is under active development. Until `v1.0`, do not deploy in production.**

## Features

- **Privacy-Preserving**: Reveal only necessary transaction details to counterparties
- **Chain-Agnostic**: Deploy same escrow logic across L1s/L2s via lightweight agents
- **ZK Conditions**: Cryptographic proof of condition fulfillment (hashlock, Ed25519, Secp256k1, threshold)

## Project Structure

```sh
zescrow/
├── core/       # Chain-agnostic types, escrow logic, conditions
├── prover/     # RISC Zero zkVM prover/verifier (optional)
├── client/     # CLI and blockchain agents
├── agent/      # On-chain programs (Solana Anchor, Ethereum Solidity)
└── deploy/     # Deployment scripts, guides, and configuration templates
```

## Quick Start

### Prerequisites

1. Install [Rust](https://rustup.rs/) (the `rust-toolchain.toml` will auto-select the correct version)
2. (Optional) Install the [RISC Zero toolchain](https://dev.risczero.com/api/zkvm/quickstart#1-install-the-risc-zero-toolchain) - only required for ZK conditions

### Deploy

```bash
# Clone and enter the repository
git clone https://github.com/kobby-pentangeli/zescrow.git
cd zescrow

# Set up environment
cp deploy/.env.template .env
# Edit .env with your configuration

# Deploy (choose network)
./deploy/solana/run.sh --network local      # Local test validator
./deploy/solana/run.sh --network devnet     # Solana devnet
./deploy/ethereum/run.sh --network local    # Local Hardhat node
./deploy/ethereum/run.sh --network sepolia  # Ethereum Sepolia

# Create an escrow
cp deploy/solana/escrow_params.json deploy/
cargo run --release -p zescrow-client -- create
```

> **Note**: For escrows with cryptographic conditions (ZK proofs), build with `--features prover`:
>
> ```bash
> cargo run --release -p zescrow-client --features prover -- create
> ```

See the [Deployment Guide](deploy/README.md) for detailed instructions on local development and devnet/testnet deployment.

## How It Works

1. **Deploy** a chain-specific agent (Solana program or EVM contract)
2. **Configure** escrow parameters (parties, amount, timelocks, conditions)
3. **Create** an escrow transaction via the CLI
4. **Finish** (release to recipient) or **Cancel** (refund to sender)

![Zescrow architecture diagram](/assets/zescrow-arch.png)

## Development

```bash
# Format (requires nightly)
cargo +nightly fmt

# Lint
RISC0_SKIP_BUILD=1 cargo clippy --all-features

# Build
RISC0_SKIP_BUILD=1 cargo build --release --all-features

# Test
RISC0_SKIP_BUILD=1 cargo test --all-features

# Documentation
RISC0_SKIP_BUILD=1 cargo doc --all-features --no-deps
```

> **Note**: `RISC0_SKIP_BUILD=1` skips compiling the zkVM guest code, which requires the RISC Zero toolchain. If you have it installed (`rzup install`), you can omit this prefix.

## Contributing

Contributions are welcome! Please read our [Contributing Guidelines](CONTRIBUTING.md) and [Code of Conduct](CODE_OF_CONDUCT.md).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
