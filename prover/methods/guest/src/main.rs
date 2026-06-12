//! The RISC Zero guest.
//!
//! Verifies an escrow's release condition and commits a binding
//! [`PublicCommitment`] to the journal, tying the receipt to one settlement.
//! The witness that satisfies the condition stays private inside the guest.

use bincode::config::standard;
use risc0_zkvm::guest::env;
use zescrow_core::{ExecutionResult, ProofInput, PublicCommitment};

fn main() {
    let bytes: Vec<u8> = env::read_frame();
    let (mut input, _): (ProofInput, _) =
        bincode::decode_from_slice(&bytes, standard()).expect("failed to decode proof input");

    let outcome = match input.escrow.execute() {
        Ok(state) => ExecutionResult::Success(state),
        Err(_) => ExecutionResult::Failure,
    };

    let commitment =
        PublicCommitment::new(&input, outcome).expect("failed to build public commitment");
    let journal = commitment
        .to_journal_bytes()
        .expect("failed to encode journal");

    env::commit_slice(&journal);
}
