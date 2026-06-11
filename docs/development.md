# Development Guide

This guide covers the toolchains, commands, and workspace layout needed to build, test, and run Zescrow end to end. For the escrow lifecycle and deployment, see the [Deployment Guide](../deploy/README.md); for the trust model, see [SECURITY.md](../SECURITY.md).

## Workspace layout

Zescrow is several Cargo workspaces plus a Foundry project, kept separate because the Solana SBF and RISC Zero guest toolchains cannot parse each other's manifests:

| Path                                                       | Workspace / project                                                                                       |
| ---------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| `./` (`core`, `prover`, `prover/methods`, `client`, `e2e`) | Root Cargo workspace (host crates).                                                                       |
| `agent/solana/escrow`                                      | Anchor workspace: the `escrow` program and a test-only `mock_verifier`. Excluded from the root workspace. |
| `agent/solana/escrow/harness`                              | `litesvm` test harness for the program. A further crate, excluded from the Anchor workspace.              |
| `agent/ethereum`                                           | Foundry project: the `Escrow` contract, tests, and deploy script.                                         |

The RISC Zero guest lives in `prover/methods/guest` and is built by `prover/methods` via `risc0-build`.

## Toolchains

`scripts/setup.sh` installs and pins the toolchains; `scripts/setup.sh --verify` reports what is present. The components and the few non-obvious details:

### Rust

Stable plus nightly (the project formats with `cargo +nightly fmt`). The host crates use the 2024 edition, which requires Rust 1.85 or newer.

### RISC Zero (zkVM guest + prover)

```bash
curl -L https://risczero.com/install | bash   # installs rzup into ~/.risc0/bin
rzup install                                   # installs the guest toolchain + r0vm
```

`~/.risc0/bin` holds only `rzup`. The prover server (`r0vm`) and `cargo-risczero` are installed under `~/.risc0/extensions/<version>-cargo-risczero-<target>/`. If the prover cannot locate `r0vm` automatically, point it explicitly:

```bash
export RISC0_SERVER_PATH="$(find "$HOME/.risc0" -type f -name r0vm | head -1)"
```

### Agave (Solana) + SBF build tools

The Agave CLI is pinned to v3.0.13. Build the programs to SBF with:

```bash
cargo build-sbf --tools-version v1.53
```

The `--tools-version v1.53` selects a platform-tools whose Rust (≥ 1.85) can parse Anchor 1.0.2's build dependencies; it changes only the compiler, not the runtime crate versions.

### Anchor

Installed via `avm`, pinned to 1.0.2.

### Foundry + Soldeer

Install Foundry (`forge`, `cast`, `anvil`). Solidity dependencies are managed by Soldeer, not git submodules:

```bash
cd agent/ethereum && forge soldeer install
```

## Canonical commands

Run these from the repository root before opening a pull request:

```bash
cargo +nightly fmt
cargo clippy --all-features --all-targets --workspace -- -D warnings
cargo build --release --all-features --all-targets
cargo doc --all-features --no-deps --document-private-items --workspace
cargo test --all-features --all-targets --workspace
```

If you do not have the RISC Zero toolchain installed, prefix each with `RISC0_SKIP_BUILD=1`. That skips compiling the guest; the host crates then build against stub method constants. CI uses the same flag for every job except the end-to-end one.

The Solana program and the Ethereum contract have their own suites:

```bash
# Solana program (litesvm harness)
cd agent/solana/escrow && cargo build-sbf --tools-version v1.53
cargo test --manifest-path agent/solana/escrow/harness/Cargo.toml

# Ethereum contract (Foundry)
cd agent/ethereum && forge test -vvv
```

## Proving environment

Three environment variables control how the guest is built and proved:

| Variable            | Effect                                                                                                                                  |
| ------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| `RISC0_SKIP_BUILD`  | When set, the guest is not compiled and host crates use stub method constants. Leave it **unset** whenever you need to actually prove.  |
| `RISC0_DEV_MODE`    | When `1`, proving produces mock receipts in-process---no GPU or Docker. For tests and local runs only; never settle real funds with it. |
| `RISC0_SERVER_PATH` | Path to the `r0vm` prover server, if it is not auto-discovered.                                                                         |

`RISC0_SKIP_BUILD` and `RISC0_DEV_MODE` are mutually exclusive in practice: dev-mode proving needs the guest, so `RISC0_SKIP_BUILD` must be unset.

Real proofs are Groth16. The final STARK-to-SNARK wrap runs only on an x86_64 host with Docker, or remotely via a configured prover (`BONSAI_API_KEY`); it is not available on Apple Silicon, even through Docker.

## End-to-end suite

The `zescrow-e2e` crate drives the real client against ephemeral local chains. Its live tests are `#[ignore]`d, so `cargo test --workspace` compiles them but skips them. Run them explicitly:

```bash
# Dev-mode (mock receipts; no GPU or Docker). Requires the RISC Zero toolchain
# (so the guest builds), plus anvil and solana-test-validator on PATH.
( cd agent/ethereum && forge soldeer install && forge build )
( cd agent/solana/escrow && cargo build-sbf --tools-version v1.53 )
RISC0_DEV_MODE=1 cargo test -p zescrow-e2e -- --ignored --test-threads=1
```

To run the identical matrix against real Groth16 proofs (x86_64 + Docker, or `BONSAI_API_KEY`):

```bash
./scripts/e2e-real-proof.sh
```
