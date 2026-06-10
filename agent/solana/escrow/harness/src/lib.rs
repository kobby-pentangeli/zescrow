//! Host-side test harness for the Zescrow Solana program.
//!
//! Lives outside the Anchor workspace so the SBF toolchain never parses its
//! edition-2024 dependencies. It loads the compiled escrow program and a mock
//! verifier into `litesvm` to exercise the full instruction surface---including
//! the conditioned-finish CPI into the verifier---without a validator, Docker,
//! or a real Groth16 receipt, and it checks the on-chain journal reconstruction
//! against the guest's commitment. Build the SBF artifacts with
//! `cargo build-sbf` in the Anchor workspace before running.

#![cfg(test)]

use std::path::PathBuf;

use anchor_lang::solana_program::system_program;
use anchor_lang::InstructionData;
use escrow::{CreateEscrowArgs, ESCROW, ID as ESCROW_ID, VERIFIER_ROUTER};
use litesvm::LiteSVM;
use solana_sdk::account::ReadableAccount;
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signer};
use solana_sdk::transaction::Transaction;

const FUND: u64 = 1_000_000_000;
const RENT: u64 = 5_000_000;

/// A non-zero release-condition commitment marking a conditioned escrow.
const CONDITIONED: [u8; 32] = [7u8; 32];

/// The escrow forwards `seal` to the verifier verbatim, so a seal is the mock's
/// `Vec<u8>` argument in Borsh form: a four-byte little-endian length followed by
/// the marker byte (`1` accepts, anything else rejects).
fn accepting_seal() -> Vec<u8> {
    vec![1, 0, 0, 0, 1]
}

fn rejecting_seal() -> Vec<u8> {
    vec![1, 0, 0, 0, 0]
}

fn read_so(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../target/deploy")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|err| {
        panic!(
            "missing {} ({err}); run `cargo build-sbf` before the harness",
            path.display()
        )
    })
}

/// A loaded VM with both programs available, a funded sender, and a separate
/// fee payer so settlement assertions are not perturbed by transaction fees.
struct Harness {
    svm: LiteSVM,
    payer: Keypair,
    sender: Keypair,
    recipient: Keypair,
}

impl Harness {
    fn new() -> Self {
        let mut svm = LiteSVM::new();
        svm.add_program(ESCROW_ID, &read_so("escrow.so")).unwrap();
        svm.add_program(VERIFIER_ROUTER, &read_so("mock_verifier.so"))
            .unwrap();

        let payer = Keypair::new();
        let sender = Keypair::new();
        let recipient = Keypair::new();
        svm.airdrop(&payer.pubkey(), FUND).unwrap();
        svm.airdrop(&sender.pubkey(), FUND * 4 + RENT).unwrap();

        Self {
            svm,
            payer,
            sender,
            recipient,
        }
    }

    fn pda(&self, id: u64) -> Pubkey {
        Pubkey::find_program_address(
            &[
                ESCROW,
                self.sender.pubkey().as_ref(),
                self.recipient.pubkey().as_ref(),
                &id.to_le_bytes(),
            ],
            &ESCROW_ID,
        )
        .0
    }

    fn create(
        &mut self,
        id: u64,
        finish_after: Option<u64>,
        cancel_after: Option<u64>,
        condition: [u8; 32],
    ) -> Result<(), String> {
        let pda = self.pda(id);
        let ix = Instruction {
            program_id: ESCROW_ID,
            accounts: vec![
                AccountMeta::new(self.sender.pubkey(), true),
                AccountMeta::new_readonly(self.recipient.pubkey(), false),
                AccountMeta::new(pda, false),
                AccountMeta::new_readonly(system_program::ID, false),
            ],
            data: escrow::instruction::CreateEscrow {
                args: CreateEscrowArgs {
                    id,
                    amount: FUND,
                    finish_after,
                    cancel_after,
                    condition,
                },
            }
            .data(),
        };
        self.send(ix, &[&self.sender.insecure_clone()])
    }

    fn finish(&mut self, id: u64, signer: &Keypair, seal: Vec<u8>) -> Result<(), String> {
        let pda = self.pda(id);
        let ix = Instruction {
            program_id: ESCROW_ID,
            accounts: vec![
                AccountMeta::new(signer.pubkey(), true),
                AccountMeta::new(self.sender.pubkey(), false),
                AccountMeta::new(pda, false),
                AccountMeta::new_readonly(VERIFIER_ROUTER, false),
            ],
            data: escrow::instruction::FinishEscrow { seal }.data(),
        };
        self.send(ix, &[&signer.insecure_clone()])
    }

    fn cancel(&mut self, id: u64) -> Result<(), String> {
        let pda = self.pda(id);
        let ix = Instruction {
            program_id: ESCROW_ID,
            accounts: vec![
                AccountMeta::new(self.sender.pubkey(), true),
                AccountMeta::new(pda, false),
            ],
            data: escrow::instruction::CancelEscrow {}.data(),
        };
        self.send(ix, &[&self.sender.insecure_clone()])
    }

    fn send(&mut self, ix: Instruction, signers: &[&Keypair]) -> Result<(), String> {
        // Rotate the blockhash so a re-sent, byte-identical transaction gets a
        // fresh signature, as it would on a live validator; otherwise litesvm
        // drops the retry as already processed instead of running it again.
        self.svm.expire_blockhash();
        let payer = self.payer.insecure_clone();
        let mut all_signers = vec![&payer];
        all_signers.extend_from_slice(signers);
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&payer.pubkey()),
            &all_signers,
            self.svm.latest_blockhash(),
        );
        self.svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|err| format!("{:?}", err.err))
    }

    fn balance(&self, key: &Pubkey) -> u64 {
        self.svm.get_balance(key).unwrap_or_default()
    }

    fn is_closed(&self, id: u64) -> bool {
        self.svm
            .get_account(&self.pda(id))
            .is_none_or(|account| account.lamports() == 0 || account.data().is_empty())
    }
}

#[test]
fn create_then_finish_unconditioned_releases_to_recipient() {
    let mut h = Harness::new();
    h.create(1, None, Some(1_000), [0u8; 32]).unwrap();

    let before = h.balance(&h.recipient.pubkey());
    let recipient = h.recipient.insecure_clone();
    h.finish(1, &recipient, Vec::new()).unwrap();

    assert_eq!(h.balance(&h.recipient.pubkey()) - before, FUND);
    assert!(h.is_closed(1));
}

#[test]
fn create_then_cancel_refunds_sender() {
    let mut h = Harness::new();
    h.create(1, None, Some(10), [0u8; 32]).unwrap();

    h.svm.warp_to_slot(10);
    let before = h.balance(&h.sender.pubkey());
    h.cancel(1).unwrap();

    // The sender recovers the escrowed amount plus the reclaimed rent reserve.
    assert!(h.balance(&h.sender.pubkey()) - before >= FUND);
    assert!(h.is_closed(1));
}

#[test]
fn finish_by_non_recipient_is_rejected() {
    let mut h = Harness::new();
    h.create(1, None, Some(1_000), [0u8; 32]).unwrap();

    let impostor = Keypair::new();
    h.svm.airdrop(&impostor.pubkey(), RENT).unwrap();
    assert!(h.finish(1, &impostor, Vec::new()).is_err());
}

#[test]
fn finish_before_window_is_rejected() {
    let mut h = Harness::new();
    h.create(1, Some(100), None, [0u8; 32]).unwrap();

    let recipient = h.recipient.insecure_clone();
    assert!(h.finish(1, &recipient, Vec::new()).is_err());

    h.svm.warp_to_slot(100);
    assert!(h.finish(1, &recipient, Vec::new()).is_ok());
}

#[test]
fn conditioned_finish_with_valid_proof_releases() {
    let mut h = Harness::new();
    h.create(1, None, Some(1_000), CONDITIONED).unwrap();

    let before = h.balance(&h.recipient.pubkey());
    let recipient = h.recipient.insecure_clone();
    h.finish(1, &recipient, accepting_seal()).unwrap();

    assert_eq!(h.balance(&h.recipient.pubkey()) - before, FUND);
    assert!(h.is_closed(1));
}

#[test]
fn conditioned_finish_with_invalid_proof_reverts() {
    let mut h = Harness::new();
    h.create(1, None, Some(1_000), CONDITIONED).unwrap();

    let recipient = h.recipient.insecure_clone();
    assert!(h.finish(1, &recipient, rejecting_seal()).is_err());
    // The escrow is untouched and can still be settled with a valid proof.
    assert!(!h.is_closed(1));
    assert!(h.finish(1, &recipient, accepting_seal()).is_ok());
}

#[test]
fn settled_escrow_cannot_be_finished_again() {
    let mut h = Harness::new();
    h.create(1, None, Some(1_000), [0u8; 32]).unwrap();

    let recipient = h.recipient.insecure_clone();
    h.finish(1, &recipient, Vec::new()).unwrap();
    assert!(h.finish(1, &recipient, Vec::new()).is_err());
}

#[test]
fn cancel_window_not_after_finish_is_rejected_at_create() {
    let mut h = Harness::new();
    // cancel_after must open strictly after the finish window; Some(0) does not.
    assert!(h.create(1, None, Some(0), [0u8; 32]).is_err());
    assert!(h.create(2, Some(50), Some(50), [0u8; 32]).is_err());
}

#[test]
fn concurrent_escrows_per_pair_use_distinct_ids() {
    let mut h = Harness::new();
    h.create(1, None, Some(1_000), [0u8; 32]).unwrap();
    h.create(2, None, Some(1_000), [0u8; 32]).unwrap();

    let recipient = h.recipient.insecure_clone();
    let before = h.balance(&h.recipient.pubkey());
    h.finish(1, &recipient, Vec::new()).unwrap();
    h.finish(2, &recipient, Vec::new()).unwrap();

    assert_eq!(h.balance(&h.recipient.pubkey()) - before, FUND * 2);
    assert!(h.is_closed(1));
    assert!(h.is_closed(2));
}

// The journal the program reconstructs on-chain must equal, byte for byte, what
// the guest commits for the same settlement; otherwise the digest the router
// checks could never match a valid receipt. This is the soundness contract
// between the off-chain prover and the on-chain verifier.
#[test]
fn reconstructed_journal_matches_guest_commitment() {
    use zescrow_core::commitment::{ExecutionResult, ProofInput, PublicCommitment};
    use zescrow_core::{Asset, BigNumber, Chain, Condition, ExecutionState, Party, ID};

    let sender = Pubkey::new_unique();
    let recipient = Pubkey::new_unique();
    let amount = 1_000_000_000u64;
    let escrow_id = 7u64;

    let condition = Condition::hashlock([5u8; 32], b"zkEscrow".to_vec());

    let onchain = escrow::Escrow {
        sender,
        recipient,
        amount,
        id: escrow_id,
        finish_after: None,
        cancel_after: None,
        condition: condition.commitment(),
        bump: 0,
    };
    let reconstructed = escrow::reconstruct_journal(&onchain);

    let input = ProofInput {
        escrow: zescrow_core::Escrow::new(
            Party::for_chain(Chain::Solana, sender.to_string()).unwrap(),
            Party::for_chain(Chain::Solana, recipient.to_string()).unwrap(),
            Asset::native(BigNumber::from(amount)),
            Some(condition),
        ),
        chain: Chain::Solana,
        agent_id: ID::for_chain(Chain::Solana, &ESCROW_ID.to_string()).unwrap(),
        escrow_id,
    };
    let commitment = PublicCommitment::new(
        &input,
        ExecutionResult::Success(ExecutionState::ConditionsMet),
    )
    .unwrap();

    assert_eq!(reconstructed, commitment.to_journal_bytes().unwrap());
}
