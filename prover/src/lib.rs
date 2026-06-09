//! RISC Zero zkVM prover for Zescrow.
//!
//! This crate provides zero-knowledge proof generation and verification
//! for escrow conditions using RISC Zero.
//!
//! # Overview
//!
//! The prover executes escrow condition verification inside a zkVM guest
//! program, producing a cryptographic proof that can be verified on-chain
//! without revealing the underlying condition data.
//!
//! # Usage
//!
//! ```ignore
//! use zescrow_prover::run;
//!
//! // Reads escrow metadata from deploy/escrow_metadata.json
//! // and generates a proof if conditions are met
//! run()?;
//! ```
//!
//! # Workflow
//!
//! 1. Load escrow metadata from JSON file
//! 2. Encode escrow context for the guest program
//! 3. Execute the zkVM to generate a proof
//! 4. Verify the receipt against the guest program ID
//! 5. Decode and validate the execution result

use anyhow::Context;
use bincode::config::standard;
use risc0_zkvm::{ExecutorEnv, Receipt, default_prover};
use thiserror::Error;
use tracing::{info, info_span};
use zescrow_core::interface::{ESCROW_METADATA_PATH, ExecutionResult, load_escrow_data};
use zescrow_core::{Escrow, EscrowMetadata, ExecutionState};
use zescrow_methods::{ZESCROW_GUEST_ELF, ZESCROW_GUEST_ID};

/// Errors that can occur during proof generation and verification.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ProverError {
    /// Receipt verification failed.
    #[error("receipt verification failed: {0}")]
    VerifyReceipt(String),

    /// Escrow execution produced an unexpected state.
    #[error("unexpected escrow state: expected ConditionsMet, got {0:?}")]
    UnexpectedState(ExecutionState),

    /// Escrow condition verification failed inside the zkVM.
    #[error("condition verification failed: {0}")]
    ConditionFailed(String),
}

/// Executes the zero-knowledge proof workflow for an escrow transaction.
///
/// This function:
/// 1. Reads escrow metadata from [`ESCROW_METADATA_PATH`]
/// 2. Constructs an [`Escrow`] from the metadata
/// 3. Executes the zkVM guest program to verify conditions
/// 4. Verifies the generated proof
///
/// # Returns
///
/// Returns `Ok(())` if all escrow conditions are fulfilled and verified.
///
/// # Errors
///
/// Returns an error if:
/// - Metadata file cannot be read or parsed
/// - Escrow construction fails
/// - Proof generation fails
/// - Receipt verification fails
/// - Escrow conditions are not met
pub fn run() -> anyhow::Result<()> {
    let _span = info_span!("zk_prover").entered();

    let escrow = load_escrow_from_metadata()?;
    let receipt = generate_proof(&escrow)?;
    verify_receipt(&receipt)?;
    validate_execution_result(&receipt)
}

/// Loads escrow data from the metadata file.
fn load_escrow_from_metadata() -> anyhow::Result<Escrow> {
    info!(path = ESCROW_METADATA_PATH, "Loading escrow metadata");

    load_escrow_data::<_, EscrowMetadata>(ESCROW_METADATA_PATH).and_then(|metadata| {
        Escrow::from_metadata(metadata).with_context(|| "failed to construct Escrow from metadata")
    })
}

/// Generates a zero-knowledge proof for the escrow.
///
/// # Arguments
///
/// * `escrow` - The escrow to generate a proof for
///
/// # Returns
///
/// The RISC Zero receipt containing the proof.
pub fn generate_proof(escrow: &Escrow) -> anyhow::Result<Receipt> {
    let escrow_bytes =
        bincode::encode_to_vec(escrow, standard()).with_context(|| "failed to encode escrow")?;

    let env = ExecutorEnv::builder()
        .write_frame(&escrow_bytes)
        .build()
        .with_context(|| "failed to build executor environment")?;

    info!("Starting zkVM proof generation");
    let start = std::time::Instant::now();

    let prove_info = default_prover()
        .prove(env, ZESCROW_GUEST_ELF)
        .with_context(|| "proof generation failed")?;

    let elapsed = start.elapsed();
    info!(
        elapsed_ms = elapsed.as_millis(),
        journal_bytes = prove_info.receipt.journal.bytes.len(),
        "Proof generated"
    );

    Ok(prove_info.receipt)
}

/// Verifies the proof receipt against the guest program ID.
///
/// # Arguments
///
/// * `receipt` - The receipt to verify
///
/// # Errors
///
/// Returns [`ProverError::VerifyReceipt`] if verification fails.
pub fn verify_receipt(receipt: &Receipt) -> anyhow::Result<()> {
    info!("Verifying receipt");
    receipt
        .verify(ZESCROW_GUEST_ID)
        .map_err(|e| ProverError::VerifyReceipt(e.to_string()))?;
    info!("Receipt verified successfully");
    Ok(())
}

/// Decodes and validates the execution result from the receipt journal.
fn validate_execution_result(receipt: &Receipt) -> anyhow::Result<()> {
    let (result, _): (ExecutionResult, _) =
        bincode::decode_from_slice(&receipt.journal.bytes, standard())
            .with_context(|| "failed to decode execution result from journal")?;

    match result {
        ExecutionResult::Ok(ExecutionState::ConditionsMet) => {
            info!("Escrow conditions fulfilled");
            Ok(())
        }
        ExecutionResult::Ok(state) => Err(ProverError::UnexpectedState(state).into()),
        ExecutionResult::Err(err) => Err(ProverError::ConditionFailed(err).into()),
    }
}
