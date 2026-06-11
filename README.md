# Zescrow

[![Crates.io](https://img.shields.io/crates/v/zescrow-core.svg)](https://crates.io/crates/zescrow-core)
[![Documentation](https://docs.rs/zescrow-core/badge.svg)](https://docs.rs/zescrow-core)
[![CI](https://github.com/kobby-pentangeli/zescrow/workflows/CI/badge.svg)](https://github.com/kobby-pentangeli/zescrow/actions)
[![License](https://img.shields.io/crates/l/zescrow.svg)](https://github.com/kobby-pentangeli/zescrow#license)

Zescrow (for zero-knowledge escrow) is a trust-minimized, chain-agnostic implementation of an escrow program using the RISC Zero zkVM as the zero-knowledge prover/verifier.

> [!WARNING]
> **This project is not audited and is under active development. Until `v1.0`, do not deploy in production.**

## Features

- **Privacy-Preserving**: Release conditions are proven in zero knowledge---the witness (hashlock preimages, signatures, public keys) never leaves the prover; only a binding commitment and the pass/fail result are revealed on-chain
- **Chain-Agnostic**: Deploy the same escrow logic across L1s/L2s via lightweight on-chain agents (a Solana program or an EVM contract)
- **Proof-Gated Settlement**: A conditioned escrow releases only against a RISC Zero receipt whose journal binds to that specific escrow, verified on-chain before funds move (hashlock, Ed25519, Secp256k1, threshold)

## Project Structure

```text
zescrow/
├── core/         # Chain-agnostic types, escrow state machine, conditions
├── prover/       # RISC Zero zkVM prover/verifier (optional; guest in prover/methods/guest)
├── client/       # CLI and the embeddable library of on-chain agents
├── agent/        # On-chain programs (per chain)
│   ├── solana/   #   Anchor program (+ litesvm harness, mock verifier)
│   └── ethereum/ #   Foundry contract (Escrow.sol)
├── e2e/          # End-to-end test suite (real client vs. ephemeral local chains)
├── deploy/       # Deployment scripts, guides, and configuration templates
├── docs/         # Developer documentation
└── scripts/      # Toolchain setup and end-to-end proof helpers
```

## Quick Start

### Prerequisites

1. Install [Rust](https://rustup.rs/) (stable; the workspace uses the 2024 edition, which requires Rust 1.85 or newer)
2. (Optional) Install the [RISC Zero toolchain](https://dev.risczero.com/api/zkvm/quickstart#1-install-the-risc-zero-toolchain)---only required for ZK conditions

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
./deploy/ethereum/run.sh --network local    # Local node (anvil)
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

1. **Deploy** a chain-specific agent (Solana program or EVM contract), pinned to the RISC Zero verifier and the guest image id
2. **Configure** escrow parameters (parties, amount, timelocks, an optional condition)
3. **Create** the escrow via the CLI; funds are locked and, for a conditioned escrow, the condition commitment is recorded on-chain
4. **Finish** — for a conditioned escrow the client generates a zero-knowledge receipt and submits `(seal, journal)`, which the on-chain agent verifies and binds to this escrow before releasing to the recipient; an unconditioned escrow releases on the time-lock alone
5. **Cancel** — refund to the sender after the cancellation deadline (which a conditioned escrow is always required to set)

```text
OFF-CHAIN  ·  prover host   —--   the witness never leaves this half
──────────────────────────────────────────────────────────────────────────────

   ┌──────────┐
   │  Sender  │
   └──────┬───┘
          │ (1) create  (params, optional condition)
          ▼
   ┌──────┴────────────────────────┐
   │  zescrow-client               │  (2) prove the witness   ┌───────────────────┐
   │  CLI + library; the `prover`  │ ────────────────────────▶│  RISC Zero guest  │
   │  feature runs the zkVM guest  │ ◀───── receipt ──────────│       (zkVM)      │
   └──────┬────────────────────────┘                          └───────────────────┘
          │
          │  (1) create  →  lock funds + record the condition commitment
          │  (3) finish  →  submit (seal, journal)

══════════╪═══════════════════════════════════════════════════════════════════
          │  only (seal, journal) cross —-- never the witness
══════════╪═══════════════════  ON-CHAIN  ·  per chain  ══════════════════════
          ▼
   ┌──────┴──────────────────────────────────┐
   │  On-chain agent                         │
   │  Solana program / EVM contract          │  (3) verify       ┌─────────────────────────┐
   │                                         │ ─────────────────▶│  RISC Zero verifier     │
   │  - holds the escrow vault               │                   │  (Groth16; on Solana,   │
   │  - reconstructs the journal from its    │ ◀────── valid ────│   a pinned router CPI)  │
   │    own state, requires it to bind THIS  │                   └─────────────────────────┘
   │    escrow, then checks the seal         │
   │    against sha256(journal)              │
   └──────┬──────────────────┬───────────────┘
          │ release          │ refund   (after `cancel_after`,
          ▼                  ▼   no valid proof was submitted)
   ┌─────────────┐     ┌──────────┐
   │  Recipient  │     │  Sender  │
   └─────────────┘     └──────────┘
```

| Step            | What happens                                                                                                                                                                                | Path                             |
| --------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------- |
| **(1) create**  | Lock the funds in the agent's vault and, for a conditioned escrow, record the condition commitment on-chain                                                                                 | Sender → client → agent          |
| **(2) prove**   | Run the zkVM guest over the private witness, producing a receipt—--a `seal` and a binding `journal`                                                                                         | client → guest → client          |
| **(3) finish**  | Submit `(seal, journal)`; the agent reconstructs the journal from its own state, requires it to bind this escrow, then verifies the seal against the pinned image id over `sha256(journal)` | client → agent → verifier        |
| **(4) release** | Once the proof is valid and bound and the finish window is open, the vault pays the recipient                                                                                               | agent → Recipient                |
| **cancel**      | After `cancel_after` with no valid proof, the vault refunds the sender in full                                                                                                              | Sender → client → agent → Sender |

A few details about the diagram above are worth calling out:

- **The witness never crosses the boundary.** Only the public receipt---the `seal` and the `journal`---goes on-chain; hashlock preimages, signatures, and public keys stay on the prover host. That OFF-CHAIN / ON-CHAIN divider is the privacy boundary.
- **A valid proof is necessary but not sufficient.** The agent re-derives the journal from its own on-chain state and requires every binding field to match---agent, escrow id, sender, recipient, asset, amount, and condition---so a genuine proof of some other escrow yields a different `sha256(journal)` and is rejected.
- **One shape, many chains.** Only the agent is per-chain---a Solana program or an EVM contract, and on Solana verification is a CPI to a pinned RISC Zero router. The client, the guest, and the receipt format are identical everywhere.
- **Unconditioned escrows skip steps (2) and (3).** With no condition there is nothing to prove: release is gated on the finish time-lock alone, and a conditioned escrow is always required to set `cancel_after` so funds can never be stranded.

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

For the full toolchain setup, the dev-mode proving environment, and how to run the end-to-end suite, see the [Development Guide](docs/development.md).

## Security

Zescrow is **not audited** and is under active development. See [SECURITY.md](SECURITY.md) for the threat model, trust assumptions, and how to report a vulnerability.

## Contributing

Contributions are welcome! Please read our [Contributing Guidelines](CONTRIBUTING.md) and [Code of Conduct](CODE_OF_CONDUCT.md).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
