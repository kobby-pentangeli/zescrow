# Security Policy

> Zescrow is not audited and is under active development. Until `v1.0`, do not deploy it in production or escrow funds you are not prepared to lose.

## Reporting a Vulnerability

Please report security issues privately, **not** through public issues or pull requests. Use GitHub's [private vulnerability reporting](https://github.com/kobby-pentangeli/zescrow/security/advisories/new) ("Report a vulnerability" under the repository's *Security* tab). Include a description, affected components, and a reproduction or proof of concept where possible. You will receive an acknowledgement, and we ask that you allow a reasonable window for a fix before any public disclosure.

## Threat Model

Zescrow is a trust-minimized, chain-agnostic escrow. A conditioned escrow releases funds only against a RISC Zero receipt whose public journal binds the proof to that exact escrow, verified on-chain before settlement. The notes below state what that guarantee covers and what it does not.

### What the proof proves and what it hides

The guest program proves that a private witness satisfies the escrow's condition and commits a binding journal. The split between public and private is:

- **Public** (revealed on-chain): the journal---the chain, agent/program id, escrow id, sender, recipient, asset, the 32-byte condition commitment, and the pass/fail outcome--—and the proof seal.
- **Private** (never leaves the prover): the witness---hashlock preimages, signatures, and public keys.

An observer therefore learns that *a* valid witness existed and which escrow settled, but not the secret that satisfied the condition.

### On-chain binding

The on-chain agent reconstructs the journal from its own account/contract state and verifies the receipt against the pinned guest image id. A receipt produced for any other escrow, amount, recipient, or condition yields a different journal and fails verification, so a valid receipt authorizes exactly one settlement:

- The EVM contract reconstructs every binding field in Solidity and compares it before calling the verifier, so the binding holds even where the verifier is a test stub.
- The Solana program reconstructs the journal and verifies it by CPI into the audited RISC Zero Solana verifier router; it forwards only the journal digest, so the binding rests on the on-chain reconstruction plus the real proof.

### Trust assumptions

- **The RISC Zero proving system and its on-chain verifier**---the Groth16 verifier (EVM) and the verifier router (Solana)---are trusted to be sound.
- **The audited guest** is trusted: only a receipt from the exact pinned guest image id authorizes a release.
- **No privileged operator.** The on-chain programs have no owner, no pause, and no upgrade authority. This is a deliberate trust-minimization choice: there is no admin who can seize, freeze, or redirect escrowed funds; and, as a consequence, no one who can intervene if a bug is found in a deployed program.

### Image-id pinning

The on-chain programs pin the guest image id. Rebuilding the guest changes the id, so the pinned value must be updated in lockstep with any guest change; a mismatch fails closed (no release). The Solana program ships a fail-closed, all-zero placeholder image id until the guest is frozen and the audited id is pinned, so it cannot release against the wrong guest.

### Time-lock semantics

`finish_after` and `cancel_after` are block heights (EVM) or slots (Solana), not wall-clock times, so settlement depends on chain ordering rather than on a clock a single validator controls. A conditioned escrow is required to set a cancellation deadline strictly after the finish window, which guarantees a proof-independent refund (funds cannot be locked forever if a valid proof never arrives) and a deterministic finish-before-cancel precedence.

## Out of Scope

The following are the operator's responsibility and are not mitigated by the protocol:

- **Key management.** A leaked sender or recipient key compromises that party's funds. Use a keystore or hardware signer; never hard-code keys.
- **RPC endpoint trust.** Use a trusted node; a malicious RPC can withhold or misreport state.
- **Dev-mode proving.** `RISC0_DEV_MODE` produces mock receipts for tests and CI and must never be used to settle real funds.
- **Mempool ordering / MEV.** Transaction ordering at the mempool level is outside the escrow's control.
- **Absence of an emergency stop.** By design there is no circuit breaker; a bug in a deployed program cannot be hot-patched.
