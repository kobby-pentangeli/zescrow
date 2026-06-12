# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0] - 2026-06-12

This release makes the zero-knowledge layer load-bearing: a conditioned escrow now releases funds only against a RISC Zero receipt whose public journal cryptographically binds the proof to that specific escrow, verified on-chain before settlement.

### Added

#### Binding proofs and on-chain verification

- `PublicCommitment` journal in `zescrow-core`: the guest commits a versioned, big-endian journal binding the chain, agent/program id, escrow id, sender, recipient, asset, condition commitment, and the pass/fail outcome, so a valid receipt ties to exactly one settlement.
- `Condition::commitment()`: a witness-free, domain-separated 32-byte binding recorded on-chain at create time and re-derived from the proof at finish, without revealing preimages or signatures.
- On-chain receipt verification on both chains. The EVM `finishEscrow(id, seal, journal)` reconstructs the journal in Solidity and verifies it against the pinned image id through `IRiscZeroVerifier` before releasing; the Solana program reconstructs the journal from its own account state and verifies it by CPI into the audited RISC Zero Solana verifier router. A conditioned escrow cannot be finished without a verifying, bound receipt.
- Guaranteed refund path: a conditioned escrow is required to set a cancellation deadline, so funds can never be locked permanently if a valid proof never arrives.
- Typed prover API: `zescrow_prover::prove(&ProofInput) -> EscrowProof`, with `encoded_seal()`/`journal_bytes()` for receipt submission and a `RISC0_DEV_MODE` path so tests and CI need no GPU or Docker.
- End-to-end release-gate suite (`zescrow-e2e`) driving the real client against ephemeral `anvil` and `solana-test-validator` chains across every condition type, the unconditioned and refund paths, and the adversarial revert paths, plus a scripted real-Groth16 run for x86 hosts.

### Changed

- Ethereum agent migrated from Hardhat to Foundry; Solidity dependencies via Soldeer (OpenZeppelin 5.1.0), no git submodules.
- Client EVM stack migrated from `ethers` to `alloy`; recipient interpretation is now tied to the configured chain rather than to string shape.
- Solana agent updated to Anchor 1.0.2 and the Solana 3.x crate line; explicit lamport settlement with checked arithmetic; per-escrow PDA uniqueness via a client-supplied id (multiple concurrent escrows per party pair); deterministic finish/cancel precedence; `#[derive(InitSpace)]` account sizing.
- TypeScript test suites replaced by Rust harnesses: a `litesvm` harness for the Solana program and `forge` unit/revert/fuzz tests for the EVM contract.
- Asset model scoped to what the agents settle (native coin and fungible token); `BigNumber` bincode wire format is now raw little-endian bytes.
- Single, centralized Cargo workspace (`[workspace.package]`/`[workspace.dependencies]`), all members at `0.3.0`, with a tracked `Cargo.lock`.
- CI runs the workspace commands plus dedicated Solana (SBF build + harness), Ethereum (Foundry + Slither + Aderyn), and end-to-end jobs; dependency auditing standardized on `cargo-deny`.

### Removed

- Hardhat, TypeChain, and the TypeScript tests/scripts from the Ethereum agent; the yarn/ts-mocha toolchain from the Solana agent.
- `ethers` and `rustc-hex` from the client.
- The unsettleable `Nft`, `MultiToken`, and `LpShare` asset variants.

### Fixed

- Threshold conditions now reject `threshold == 0` and `threshold > subconditions`, closing a zero-threshold bypass.
- Chain-explicit identity encoding (`ID::for_chain`) removes the ambiguous hex/Base58/Base64 auto-detection from the binding path.
- Missing environment variables in secret-bearing configuration fields now error instead of expanding to an empty string.

## [0.2.0] - 2026-01-11

### Added

#### New Crate: `zescrow-prover`

- Extracted RISC Zero zkVM prover into a standalone crate for build optimization
- ZK proving is now opt-in via `--features prover` flag
- Users without RISC Zero toolchain can build and use timelock-only escrows

#### Core Library (`zescrow-core`)

- `#[non_exhaustive]` on all public error enums for future extensibility
- `#[must_use]` attributes on verification methods
- Module-level documentation for all public modules
- Security considerations in cryptographic condition documentation
- Environment variable expansion (`${VAR_NAME}`) in JSON configuration files
- New tests: escrow error paths, interface module, serialization roundtrips (24 -> 46 tests)

#### Client (`zescrow-client`)

- `dotenvy` integration for `.env` file loading
- Structured error context via `ClientError::ethereum()` and `ClientError::solana()` helpers
- Comprehensive documentation for `Agent` trait and all public APIs

#### Solana Program (`escrow`)

- Devnet program configuration in `Anchor.toml`
- Package metadata (description, license, repository)

#### Ethereum Contract

- `nextEscrowId()` getter for next escrow ID
- `escrowCount()` getter for total escrows created
- Sepolia network configuration in `hardhat.config.ts`

#### Deployment & CI

- Consolidated `/deploy/` directory with Solana devnet and Ethereum Sepolia scripts
- `.env.template` for environment variable configuration
- `cargo-deny` integration for dependency auditing
- Development commands section in README
- Optimized build workflow: debug builds for `create`, `cancel`, `generate`; release only for `finish` with prover
- `RUST_LOG=info` default in `.env.template` for CLI output visibility

### Changed

#### Core Library (`zescrow-core`)

- Refactored `ID::from_str` to use functional combinators
- Refactored `Escrow::execute` with functional state transitions
- Refactored `Threshold::verify` with iterator combinators
- Refactored `Asset::validate()` with functional match arms
- Improved `format_amount()` error handling

#### Client (`zescrow-client`)

- Unified `ClientError` hierarchy (flattened from separate `AgentError`)
- Extracted helper functions in Ethereum agent: `load_contract_abi()`, `create_contract_instance()`, `extract_escrow_id()`
- Extracted helper functions in Solana agent: `build_create_instruction()`, `build_finish_instruction()`, `derive_escrow_pda()`
- Refactored prover module with structured logging via `tracing` spans

#### Solana Program (`escrow`)

- Updated Anchor from 0.31.1 to 0.32.1

#### CI/CD

- Updated GitHub Actions: `actions/checkout@v4`, `dtolnay/rust-toolchain`, `actions/cache@v4`
- Added `RISC0_SKIP_BUILD=1` for faster CI builds

### Fixed

- Typo in `AssetError::InvalidId` ("inalid" → "invalid")
- Unused import `ed25519_dalek::Verifier` in `secp256k1.rs`
- Rustdoc HTML escaping for `Vec<u8>` in serde module
- Hex identity parsing now handles optional `0x` prefix correctly

### Removed

- Legacy `/templates/` directory (consolidated into `/deploy/`)

## [0.1.0] - 2024-12-15

Initial release.

### Added

- Chain-agnostic escrow core library with cryptographic conditions
- RISC Zero zkVM integration for zero-knowledge proofs
- Solana Anchor program with XRPL-style timelock semantics
- Ethereum Solidity escrow contract with an auto-incrementing escrow id
- CLI client for cross-chain escrow operations
- Support for hashlock, Ed25519, Secp256k1, and threshold conditions

---

## Guidelines for Contributors

When adding entries to this changelog for future releases:

1. **Format**: Follow [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
2. **Categories**: Use Added, Changed, Deprecated, Removed, Fixed, Security
3. **Audience**: Write for users, not developers (focus on impact, not implementation)
4. **Links**: Add comparison links at the bottom: `[0.3.0]: https://github.com/kobby-pentangeli/zescrow/compare/v0.2.0...v0.3.0`

[0.3.0]: https://github.com/kobby-pentangeli/zescrow/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/kobby-pentangeli/zescrow/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/kobby-pentangeli/zescrow/releases/tag/v0.1.0
