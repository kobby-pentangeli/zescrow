//! RISC Zero zkVM prover for Zescrow.
//!
//! This crate generates a zero-knowledge proof that an escrow's release
//! condition was satisfied, and binds that proof to a specific settlement via a
//! [`PublicCommitment`] committed to the receipt journal. The witness that
//! satisfies the condition (hashlock preimage, signatures) stays private inside
//! the guest; only the binding fields and the pass/fail outcome are public.
//!
//! # Workflow
//!
//! 1. Encode a [`ProofInput`] (escrow context plus the on-chain identifiers).
//! 2. Execute the zkVM guest, which verifies the condition and commits the
//!    binding journal.
//! 3. Verify the receipt against the pinned guest image id.
//! 4. Decode the journal into a typed [`PublicCommitment`] and confirm the
//!    escrow's conditions were met.
//!
//! The receipt is requested as a Groth16 proof so it is small enough to verify
//! on-chain. Real Groth16 proving requires an x86 host with Docker, or a remote
//! prover selected by [`default_prover`] via the usual environment variables;
//! set `RISC0_DEV_MODE=1` to produce mock receipts in tests and CI without a
//! prover.
//!
//! # Image-id pinning
//!
//! [`ZESCROW_GUEST_ID`] is the cryptographic identity of the audited guest. The
//! on-chain verifiers hard-pin this value so only receipts produced by this
//! exact guest can authorize a release. It is derived from the guest ELF at
//! build time by [`zescrow_methods`] and changes whenever the guest source or
//! any of its dependencies change; when it does, the pinned on-chain constant
//! must be regenerated (rebuild this workspace, read [`guest_image_id`]) and
//! updated in lockstep with the deployed programs.
//!
//! ```ignore
//! use zescrow_prover::{prove, ProofInput};
//!
//! let proof = prove(&proof_input)?;       // binding receipt + typed commitment
//! assert_eq!(proof.image_id, zescrow_prover::guest_image_id());
//! ```

use anyhow::Context;
use bincode::config::standard;
use risc0_zkvm::{ExecutorEnv, ProverOpts, Receipt, default_prover};
use thiserror::Error;
use tracing::info;
use zescrow_core::error::CommitmentError;
pub use zescrow_core::{ExecutionResult, ExecutionState, ProofInput, PublicCommitment};
use zescrow_methods::{ZESCROW_GUEST_ELF, ZESCROW_GUEST_ID};

/// Errors that can occur during proof generation and verification.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ProverError {
    /// Receipt verification against the guest image id failed.
    #[error("receipt verification failed: {0}")]
    VerifyReceipt(String),

    /// The committed journal could not be decoded into a [`PublicCommitment`].
    #[error("could not decode commitment journal: {0}")]
    Journal(#[from] CommitmentError),

    /// The proof did not attest that the escrow's conditions were met.
    #[error("proof did not attest conditions met: {0:?}")]
    UnexpectedOutcome(ExecutionResult),
}

/// A binding proof artifact: the receipt, the typed public commitment decoded
/// from its journal, and the guest image id the receipt was verified against.
///
/// The receipt carries the seal an on-chain client submits; the EVM and Solana
/// agents encode it into their respective verifier formats at submission time.
#[derive(Debug, Clone)]
pub struct EscrowProof {
    /// The pinned guest image id the receipt verifies under.
    pub image_id: [u32; 8],
    /// The binding commitment decoded from the receipt journal.
    pub commitment: PublicCommitment,
    /// The verifiable receipt (journal + seal).
    pub receipt: Receipt,
}

impl EscrowProof {
    /// The selector-prefixed receipt seal an on-chain client submits.
    ///
    /// The 4-byte selector identifies the proof system and verifier version, so
    /// the same encoding is consumed by both the EVM verifier and the Solana
    /// verifier router; each chain client applies only its own framing (the EVM
    /// agent submits these bytes verbatim, the Solana agent length-prefixes them
    /// for the router CPI).
    ///
    /// # Errors
    ///
    /// Returns an error if the receipt's seal cannot be encoded.
    pub fn encoded_seal(&self) -> anyhow::Result<Vec<u8>> {
        risc0_ethereum_contracts::encode_seal(&self.receipt)
    }

    /// The journal bytes committed by the guest, submitted alongside the seal so
    /// the on-chain verifier can bind the proof to this exact settlement.
    pub fn journal_bytes(&self) -> &[u8] {
        &self.receipt.journal.bytes
    }
}

/// The image id of the audited guest, for pinning in the on-chain verifiers.
pub fn guest_image_id() -> [u32; 8] {
    ZESCROW_GUEST_ID
}

/// Generates a binding zero-knowledge proof for `input`.
///
/// Executes the guest (which verifies the condition and commits the binding
/// journal), verifies the resulting receipt against [`ZESCROW_GUEST_ID`],
/// decodes the journal into a [`PublicCommitment`], and confirms the escrow's
/// conditions were met.
///
/// # Errors
///
/// Returns an error if encoding the input fails, proving fails, the receipt
/// does not verify, the journal cannot be decoded, or the proof attests an
/// outcome other than conditions-met.
pub fn prove(input: &ProofInput) -> anyhow::Result<EscrowProof> {
    let input_bytes = bincode::encode_to_vec(input, standard())
        .with_context(|| "failed to encode proof input")?;

    let env = ExecutorEnv::builder()
        .write_frame(&input_bytes)
        .build()
        .with_context(|| "failed to build executor environment")?;

    info!("Starting zkVM proof generation");
    let start = std::time::Instant::now();

    let receipt = default_prover()
        .prove_with_opts(env, ZESCROW_GUEST_ELF, &ProverOpts::groth16())
        .with_context(|| "proof generation failed")?
        .receipt;

    info!(
        elapsed_ms = start.elapsed().as_millis(),
        journal_bytes = receipt.journal.bytes.len(),
        "Proof generated"
    );

    receipt
        .verify(ZESCROW_GUEST_ID)
        .map_err(|e| ProverError::VerifyReceipt(e.to_string()))?;

    let commitment = PublicCommitment::from_journal_bytes(&receipt.journal.bytes)
        .map_err(ProverError::Journal)?;

    match commitment.outcome {
        ExecutionResult::Success(ExecutionState::ConditionsMet) => Ok(EscrowProof {
            image_id: ZESCROW_GUEST_ID,
            commitment,
            receipt,
        }),
        other => Err(ProverError::UnexpectedOutcome(other).into()),
    }
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};
    use zescrow_core::{Asset, BigNumber, Chain, Condition, Escrow, ID, Party};

    use super::*;

    fn funded_escrow(condition: Option<Condition>) -> Escrow {
        let mut escrow = Escrow::new(
            Party::for_chain(
                Chain::Ethereum,
                "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
            )
            .unwrap(),
            Party::for_chain(
                Chain::Ethereum,
                "0xEA674fdDe714fd979de3EdF0F56AA9716B898ec8",
            )
            .unwrap(),
            Asset::native(BigNumber::from(1_000_000_000u64)),
            condition,
        );
        escrow.state = ExecutionState::Funded;
        escrow
    }

    fn proof_input(escrow: Escrow) -> ProofInput {
        ProofInput {
            escrow,
            chain: Chain::Ethereum,
            agent_id: ID::for_chain(
                Chain::Ethereum,
                "0x00000000000000000000000000000000DeaDBeef",
            )
            .unwrap(),
            escrow_id: 7,
        }
    }

    /// These exercise the real guest under dev mode. They run only when
    /// `RISC0_DEV_MODE=1` is set (locally or in the dedicated CI job), avoiding
    /// the Docker/x86 requirement of real Groth16 proving.
    fn dev_mode() -> bool {
        if ProverOpts::default().dev_mode() {
            return true;
        }
        eprintln!("skipping prover test: set RISC0_DEV_MODE=1 to run");
        false
    }

    #[test]
    fn public_commitment_journal_encodes_and_decodes() {
        let input = proof_input(funded_escrow(None));
        let outcome = ExecutionResult::Success(ExecutionState::ConditionsMet);
        let commitment = PublicCommitment::new(&input, outcome).expect("build commitment");

        let bytes = commitment.to_journal_bytes().expect("encode journal");
        let decoded = PublicCommitment::from_journal_bytes(&bytes).expect("decode journal");

        assert_eq!(decoded, commitment);
        assert_eq!(decoded.escrow_id, input.escrow_id);
        assert_eq!(decoded.outcome, outcome);
    }

    #[test]
    fn dev_mode_prove_binds_journal() {
        if !dev_mode() {
            return;
        }
        let preimage = b"zkEscrow".to_vec();
        let hash: [u8; 32] = Sha256::digest(&preimage).into();
        let condition = Condition::hashlock(hash, preimage);

        let proof = prove(&proof_input(funded_escrow(Some(condition.clone())))).expect("dev proof");

        assert_eq!(proof.image_id, guest_image_id());
        assert_eq!(
            proof.commitment.outcome,
            ExecutionResult::Success(ExecutionState::ConditionsMet)
        );
        assert_eq!(proof.commitment.escrow_id, 7);
        assert_eq!(proof.commitment.condition, condition.commitment());

        // The committed journal must decode back to the same typed commitment.
        let decoded = PublicCommitment::from_journal_bytes(&proof.receipt.journal.bytes).unwrap();
        assert_eq!(decoded, proof.commitment);
    }

    #[test]
    fn dev_mode_rejects_unsatisfied_condition() {
        if !dev_mode() {
            return;
        }
        // Preimage does not hash to `hash`, so the guest reports failure.
        let condition = Condition::hashlock([0u8; 32], b"wrong-preimage".to_vec());
        assert!(prove(&proof_input(funded_escrow(Some(condition)))).is_err());
    }
}
