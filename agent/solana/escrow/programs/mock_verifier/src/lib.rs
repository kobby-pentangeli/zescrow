//! Test-only stand-in for the RISC Zero Solana Verifier Router.
//!
//! It exposes the same `verify(seal, image_id, journal_digest)` instruction as
//! the router, so the escrow program's CPI path is exercised exactly as in
//! production, but decides acceptance from a marker byte in `seal` instead of a
//! pairing check. This lets the test harness drive both the valid-proof and
//! invalid-proof branches without producing a real Groth16 receipt (which needs
//! an x86 + Docker prover). It must never be deployed to a live cluster.

use anchor_lang::prelude::*;

declare_id!("6JvFfBrvCcWgANKh1Eae9xDq4RC6cfJuBcf71rp2k9Y7");

#[program]
pub mod mock_verifier {
    use super::*;

    /// Accepts when `seal` begins with the success marker (`1`); otherwise
    /// rejects, standing in for the router refusing an invalid proof. The image
    /// id and journal digest are accepted unconditionally—--binding correctness
    /// is covered by the escrow program's journal-reconstruction parity test.
    pub fn verify(
        _ctx: Context<Verify>,
        seal: Vec<u8>,
        _image_id: [u8; 32],
        _journal_digest: [u8; 32],
    ) -> Result<()> {
        require!(seal.first() == Some(&1), MockError::Rejected);
        Ok(())
    }
}

/// The router's `verify` needs no accounts; neither does this stub.
#[derive(Accounts)]
pub struct Verify {}

/// Mock verifier error codes.
#[error_code]
pub enum MockError {
    /// The supplied seal did not carry the success marker.
    #[msg("Mock verifier rejected the proof.")]
    Rejected,
}
