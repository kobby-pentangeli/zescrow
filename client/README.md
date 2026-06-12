# Zescrow Client

`zescrow-client` is the Zescrow command-line interface and the library of on-chain agents behind it. It drives the full escrow lifecycle---create, finish, cancel---against either supported chain through a single `ChainConfig`, and for a conditioned escrow it generates the zero-knowledge receipt and submits it to the on-chain proof gate.

The crate is both a binary (the `zescrow-client` CLI) and a library: the `ZescrowClient`, the `Agent` trait, and the `EthereumAgent`/`SolanaAgent` implementations are public, so other Rust programs can embed the same flow as a Git or path dependency.

## Features

- `prover` (off by default): pulls in `zescrow-prover` so the client can generate a RISC Zero receipt for a conditioned escrow. Without it, conditioned escrows can be created and cancelled, but `finish` is refused rather than attempted against the proof gate without a receipt. Unconditioned escrows need no prover.

## Installation

`zescrow-client` ships with the repository rather than on crates.io. Install the CLI from source:

```bash
# Unconditioned (timelock-only) escrows---no RISC Zero toolchain needed.
cargo install --git https://github.com/kobby-pentangeli/zescrow zescrow-client

# With zero-knowledge proof generation for conditioned escrows.
cargo install --git https://github.com/kobby-pentangeli/zescrow zescrow-client --features prover
```

Or build it from a checkout of the workspace:

```bash
cargo build --release -p zescrow-client                    # timelock-only
cargo build --release -p zescrow-client --features prover  # with ZK proving
```

## Configuration

The CLI reads three JSON files from the `deploy/` directory (paths are defined by `zescrow-core::interface`):

| File                     | Role                                                           |
| ------------------------ | -------------------------------------------------------------- |
| `escrow_params.json`     | Input to `create`: chain config, asset, time-locks, flags.     |
| `escrow_conditions.json` | The condition for a conditioned escrow (output of `generate`). |
| `escrow_metadata.json`   | Written by `create`; read by `finish` and `cancel`.            |

Environment variables referenced as `${VAR}` inside the config are expanded at load time; a missing variable in a secret-bearing field is a hard error rather than a silent empty value. A `.env` file in the working directory is loaded automatically. Set `RUST_LOG=info` to see progress logs.

## Commands

```bash
# Create the escrow described by deploy/escrow_params.json (locks funds).
zescrow-client create

# Release to the recipient. For a conditioned escrow this generates and submits
# the bound receipt; build with --features prover.
zescrow-client finish --recipient <RECIPIENT>

# Refund to the sender after the cancellation deadline.
zescrow-client cancel

# Generate a condition file (writes deploy/escrow_conditions.json by default).
zescrow-client generate hashlock   --preimage ./preimage.txt
zescrow-client generate ed25519    --pubkey <hex> --msg <hex> --sig <hex>
zescrow-client generate secp256k1  --pubkey <hex> --msg <hex> --sig <hex>
zescrow-client generate threshold  --subconditions a.json b.json --threshold 1
```

`<RECIPIENT>` is interpreted against the configured chain: a keypair-file path on Solana, or a hex private key (`0x` prefix optional) on Ethereum.

See the repository [Deployment Guide](../deploy/README.md) for an end-to-end walkthrough on a local node and on devnet/testnet, and [`docs/development.md`](../docs/development.md) for the toolchain and dev-mode proving setup.

## License

Licensed under either of [Apache-2.0](../LICENSE-APACHE) or [MIT](../LICENSE-MIT) at your option.
