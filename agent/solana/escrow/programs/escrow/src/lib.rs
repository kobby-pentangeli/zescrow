//! Escrow program with XRPL-style time-lock semantics and on-chain proof gating.
//!
//! Funds are locked in a per-escrow PDA and released to the recipient only after
//! its finish window opens and, for a conditioned escrow, a RISC Zero receipt
//! proves the release condition was met. Verification is delegated to the
//! audited RISC Zero Solana Verifier Router via CPI: this program reconstructs
//! the binding journal from its own state, hashes it, and asks the
//! router to check the seal against the pinned guest image id over that digest.
//! A receipt that proves any other escrow, amount, or condition yields a
//! different digest and fails verification, so a valid proof binds to exactly
//! one settlement.

use anchor_lang::prelude::*;
use anchor_lang::solana_program::clock::Clock;
use anchor_lang::solana_program::instruction::{AccountMeta, Instruction};
use anchor_lang::solana_program::program::invoke;
use anchor_lang::system_program;
use sha2::{Digest, Sha256};

declare_id!("J4SfUoLAAsvmAWMQGa8dJHw8vsSvRfUUMXGTxcmSeS8s");

/// Seed prefix for escrow PDA derivation.
pub const ESCROW: &[u8] = b"escrow";

/// The RISC Zero Solana Verifier Router this program delegates proof checking to.
pub const VERIFIER_ROUTER: Pubkey =
    Pubkey::from_str_const("6JvFfBrvCcWgANKh1Eae9xDq4RC6cfJuBcf71rp2k9Y7");

/// The pinned guest image id authorized to release conditioned escrows.
pub const IMAGE_ID: [u8; 32] = [0u8; 32];

/// All-zero sentinel marking an unconditioned escrow (no proof required).
const NO_CONDITION: [u8; 32] = [0u8; 32];

/// Canonical journal version reconstructed for verification.
const JOURNAL_VERSION: u8 = 1;
/// Chain tag for Solana in the journal.
const CHAIN_SOLANA: u8 = 1;
/// Outcome tag: execution succeeded.
const OUTCOME_SUCCESS: u8 = 1;
/// Execution-state tag: the release conditions were met.
const STATE_CONDITIONS_MET: u8 = 2;
/// Asset-kind tag: the native coin (currently the only kind this program settles).
const ASSET_NATIVE: u8 = 0;
/// Width of the big-endian amount field.
const AMOUNT_WIDTH: usize = 32;

#[program]
pub mod escrow {
    use super::*;

    /// Creates and funds a new escrow.
    ///
    /// At least one of `finish_after`/`cancel_after` must be set so an escrow
    /// always has a time-based resolution path and funds can never be locked
    /// indefinitely. When cancellation is enabled it must open strictly after the
    /// finish window so the recipient always holds an exclusive release window
    /// before the sender can reclaim, making settlement deterministic. A non-zero
    /// `condition` records the witness-free release-condition commitment that a
    /// later finish must prove against.
    pub fn create_escrow(ctx: Context<CreateEscrow>, args: CreateEscrowArgs) -> Result<()> {
        require!(
            args.finish_after.is_some() || args.cancel_after.is_some(),
            EscrowError::MustSpecifyPath
        );
        if let Some(cancel) = args.cancel_after {
            require!(
                cancel > args.finish_after.unwrap_or(0),
                EscrowError::InvalidTimeOrder
            );
        }
        require!(args.amount > 0, EscrowError::InvalidAmount);

        let cpi_ctx = CpiContext::new(
            ctx.accounts.system_program.key(),
            system_program::Transfer {
                from: ctx.accounts.sender.to_account_info(),
                to: ctx.accounts.escrow_account.to_account_info(),
            },
        );
        system_program::transfer(cpi_ctx, args.amount)?;

        let escrow = &mut ctx.accounts.escrow_account;
        escrow.sender = ctx.accounts.sender.key();
        escrow.recipient = ctx.accounts.recipient.key();
        escrow.amount = args.amount;
        escrow.id = args.id;
        escrow.finish_after = args.finish_after;
        escrow.cancel_after = args.cancel_after;
        escrow.condition = args.condition;
        escrow.bump = ctx.bumps.escrow_account;

        emit!(EscrowEvent {
            id: escrow.id,
            sender: escrow.sender,
            recipient: escrow.recipient,
            amount: escrow.amount,
            action: EscrowState::Created
        });

        Ok(())
    }

    /// Releases an escrow to its recipient.
    ///
    /// Callable only by the recipient and only once the finish window has opened.
    /// For a conditioned escrow, `seal` must be a RISC Zero receipt seal whose
    /// proof verifies, via the pinned router, against the proof journal
    /// reconstructed from this escrow's state; the router's accounts are passed
    /// as remaining accounts. The escrowed `amount` is transferred explicitly to
    /// the recipient and the PDA is closed, returning its rent reserve to the
    /// original funder.
    pub fn finish_escrow<'info>(
        ctx: Context<'info, FinishEscrow<'info>>,
        seal: Vec<u8>,
    ) -> Result<()> {
        let escrow = &ctx.accounts.escrow_account;
        require!(
            ctx.accounts.recipient.key() == escrow.recipient,
            EscrowError::Unauthorized
        );
        if let Some(finish_after) = escrow.finish_after {
            require!(Clock::get()?.slot >= finish_after, EscrowError::NotReady);
        }

        if escrow.condition != NO_CONDITION {
            verify_release_proof(
                escrow,
                ctx.accounts.verifier_router.to_account_info(),
                ctx.remaining_accounts,
                seal,
            )?;
        }

        let amount = escrow.amount;
        let escrow_info = ctx.accounts.escrow_account.to_account_info();
        let recipient_info = ctx.accounts.recipient.to_account_info();
        let debited = escrow_info
            .lamports()
            .checked_sub(amount)
            .ok_or(EscrowError::ArithmeticOverflow)?;
        let credited = recipient_info
            .lamports()
            .checked_add(amount)
            .ok_or(EscrowError::ArithmeticOverflow)?;
        **escrow_info.try_borrow_mut_lamports()? = debited;
        **recipient_info.try_borrow_mut_lamports()? = credited;

        emit!(EscrowEvent {
            id: escrow.id,
            sender: escrow.sender,
            recipient: escrow.recipient,
            amount,
            action: EscrowState::Finished
        });

        Ok(())
    }

    /// Cancels an escrow and refunds the original sender.
    ///
    /// Callable only by the sender, only when cancellation was enabled, and only
    /// once the cancel window has opened. Closing the PDA returns the full
    /// balance (escrowed amount plus rent reserve) to the sender.
    pub fn cancel_escrow(ctx: Context<CancelEscrow>) -> Result<()> {
        let escrow = &ctx.accounts.escrow_account;
        require!(
            ctx.accounts.sender.key() == escrow.sender,
            EscrowError::Unauthorized
        );
        let cancel_after = escrow.cancel_after.ok_or(EscrowError::CancelNotAllowed)?;
        require!(Clock::get()?.slot >= cancel_after, EscrowError::NotExpired);

        emit!(EscrowEvent {
            id: escrow.id,
            sender: escrow.sender,
            recipient: escrow.recipient,
            amount: escrow.amount,
            action: EscrowState::Cancelled
        });

        Ok(())
    }
}

/// Verifies a conditioned escrow's release proof via the pinned router.
///
/// The journal is reconstructed solely from on-chain state, so its digest binds
/// the seal to this exact escrow; `seal` carries only the opaque, already
/// Borsh-encoded receipt seal. The router CPI reverts on an invalid proof.
fn verify_release_proof<'info>(
    escrow: &Escrow,
    verifier_router: AccountInfo<'info>,
    router_accounts: &[AccountInfo<'info>],
    seal: Vec<u8>,
) -> Result<()> {
    let journal = reconstruct_journal(escrow);
    let journal_digest: [u8; 32] = Sha256::digest(&journal).into();

    // Anchor instruction discriminator for the router's `verify`, derived here so
    // it cannot drift from the upstream interface: sha256("global:verify")[..8],
    // followed by the Borsh-encoded (seal, image_id, journal_digest) arguments.
    let discriminator = Sha256::digest(b"global:verify");
    let data: Vec<u8> = discriminator[..8]
        .iter()
        .chain(seal.iter())
        .chain(IMAGE_ID.iter())
        .chain(journal_digest.iter())
        .copied()
        .collect();

    let metas: Vec<AccountMeta> = router_accounts
        .iter()
        .map(|account| AccountMeta {
            pubkey: *account.key,
            is_signer: account.is_signer,
            is_writable: account.is_writable,
        })
        .collect();

    let instruction = Instruction {
        program_id: VERIFIER_ROUTER,
        accounts: metas,
        data,
    };

    let infos: Vec<AccountInfo<'info>> = router_accounts
        .iter()
        .cloned()
        .chain(core::iter::once(verifier_router))
        .collect();

    invoke(&instruction, &infos).map_err(|_| EscrowError::ProofVerificationFailed)?;
    Ok(())
}

/// Reconstructs the binding journal for a met, native-coin escrow.
///
/// The layout matches the guest's public commitment exactly (version 1, Solana,
/// success, conditions-met, length-prefixed 32-byte identities, big-endian
/// amount), so the digest equals what the guest committed for this settlement.
/// Exposed so the binding can be checked against the guest's encoder off-chain.
pub fn reconstruct_journal(escrow: &Escrow) -> Vec<u8> {
    let mut amount = [0u8; AMOUNT_WIDTH];
    amount[AMOUNT_WIDTH - 8..].copy_from_slice(&escrow.amount.to_be_bytes());

    // The only length-prefixed fields are 32-byte public keys and the empty
    // native-token field, so every `u16` prefix is a compile-time constant
    // rather than a run-time cast.
    const PUBKEY_LEN: [u8; 2] = 32u16.to_be_bytes();
    const EMPTY_LEN: [u8; 2] = 0u16.to_be_bytes();

    [
        JOURNAL_VERSION,
        CHAIN_SOLANA,
        OUTCOME_SUCCESS,
        STATE_CONDITIONS_MET,
    ]
    .into_iter()
    .chain(PUBKEY_LEN)
    .chain(crate::ID.to_bytes())
    .chain(escrow.id.to_be_bytes())
    .chain(PUBKEY_LEN)
    .chain(escrow.sender.to_bytes())
    .chain(PUBKEY_LEN)
    .chain(escrow.recipient.to_bytes())
    .chain([ASSET_NATIVE])
    .chain(EMPTY_LEN)
    .chain(amount)
    .chain(escrow.condition)
    .collect()
}

/// Escrow account data, stored in a per-escrow PDA.
#[account]
#[derive(InitSpace)]
pub struct Escrow {
    /// Account that funded the escrow.
    pub sender: Pubkey,
    /// Intended beneficiary of the escrowed funds.
    pub recipient: Pubkey,
    /// Amount of lamports locked, exclusive of the rent reserve.
    pub amount: u64,
    /// Caller-supplied unique identifier; a PDA seed and the journal binding.
    pub id: u64,
    /// Slot after which release is allowed (`None` allows immediate release).
    pub finish_after: Option<u64>,
    /// Slot after which the sender may reclaim (`None` disables cancellation).
    pub cancel_after: Option<u64>,
    /// Witness-free release-condition commitment (all-zero when unconditioned).
    pub condition: [u8; 32],
    /// PDA bump seed for address validation.
    pub bump: u8,
}

/// Context for `create_escrow`.
#[derive(Accounts)]
#[instruction(args: CreateEscrowArgs)]
pub struct CreateEscrow<'info> {
    /// Sender funding the escrow.
    #[account(mut)]
    pub sender: Signer<'info>,

    /// Recipient of the escrow; must differ from the sender.
    ///
    /// CHECK: only its key is used, enforced via PDA seeds and the constraint.
    #[account(constraint = recipient.key() != sender.key() @ EscrowError::InvalidRecipient)]
    pub recipient: UncheckedAccount<'info>,

    /// Per-escrow PDA, unique to `(sender, recipient, id)`.
    #[account(
        init,
        payer = sender,
        space = 8 + Escrow::INIT_SPACE,
        seeds = [ESCROW, sender.key().as_ref(), recipient.key().as_ref(), &args.id.to_le_bytes()],
        bump
    )]
    pub escrow_account: Account<'info, Escrow>,

    /// System program for the funding transfer.
    pub system_program: Program<'info, System>,
}

/// Arguments for `create_escrow`.
#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct CreateEscrowArgs {
    /// Caller-supplied unique id; reusing an id that still has a live escrow for
    /// the same `(sender, recipient)` pair fails account initialization.
    pub id: u64,
    /// Amount of lamports to lock.
    pub amount: u64,
    /// Slot after which release is allowed (`None` allows immediate release).
    pub finish_after: Option<u64>,
    /// Slot after which the sender may reclaim (`None` disables cancellation).
    pub cancel_after: Option<u64>,
    /// Witness-free release-condition commitment (all-zero for unconditioned).
    pub condition: [u8; 32],
}

/// Context for `finish_escrow`.
#[derive(Accounts)]
pub struct FinishEscrow<'info> {
    /// Recipient claiming the funds.
    #[account(mut)]
    pub recipient: Signer<'info>,

    /// Original funder, refunded the rent reserve when the PDA is closed.
    ///
    /// CHECK: constrained to the escrow's recorded sender.
    #[account(mut, address = escrow_account.sender)]
    pub sender: UncheckedAccount<'info>,

    /// Per-escrow PDA, closed to the original funder on success.
    #[account(
        mut,
        seeds = [ESCROW, escrow_account.sender.as_ref(), recipient.key().as_ref(), &escrow_account.id.to_le_bytes()],
        bump = escrow_account.bump,
        close = sender
    )]
    pub escrow_account: Account<'info, Escrow>,

    /// The pinned RISC Zero verifier router (used only for conditioned escrows).
    ///
    /// CHECK: constrained to the compile-time-pinned router address.
    #[account(address = VERIFIER_ROUTER)]
    pub verifier_router: UncheckedAccount<'info>,
}

/// Context for `cancel_escrow`.
#[derive(Accounts)]
pub struct CancelEscrow<'info> {
    /// Original funder reclaiming the escrow.
    #[account(mut)]
    pub sender: Signer<'info>,

    /// Per-escrow PDA, closed back to the sender on success.
    #[account(
        mut,
        seeds = [ESCROW, sender.key().as_ref(), escrow_account.recipient.as_ref(), &escrow_account.id.to_le_bytes()],
        bump = escrow_account.bump,
        close = sender
    )]
    pub escrow_account: Account<'info, Escrow>,
}

/// Events emitted across the escrow lifecycle.
#[event]
pub struct EscrowEvent {
    /// Caller-supplied escrow id.
    pub id: u64,
    /// Original funder.
    pub sender: Pubkey,
    /// Intended beneficiary.
    pub recipient: Pubkey,
    /// Escrowed amount in lamports.
    pub amount: u64,
    /// Lifecycle stage just executed.
    pub action: EscrowState,
}

/// Escrow lifecycle actions.
#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub enum EscrowState {
    /// Escrow initialized and funded.
    Created,
    /// Escrow released to the recipient.
    Finished,
    /// Escrow cancelled and the sender refunded.
    Cancelled,
}

/// Program-specific error codes.
#[error_code]
pub enum EscrowError {
    /// Specified amount is zero.
    #[msg("Amount must be greater than zero.")]
    InvalidAmount,

    /// Neither `finish_after` nor `cancel_after` was provided.
    #[msg("Must specify at least one of finish_after or cancel_after.")]
    MustSpecifyPath,

    /// Cancellation does not open strictly after the finish window.
    #[msg("cancel_after must be greater than finish_after.")]
    InvalidTimeOrder,

    /// Sender and recipient are the same account.
    #[msg("Sender and recipient must differ.")]
    InvalidRecipient,

    /// Caller is not the account authorized for this action.
    #[msg("Unauthorized caller.")]
    Unauthorized,

    /// The finish window has not yet opened.
    #[msg("Too early to finish.")]
    NotReady,

    /// Cancellation was not enabled for this escrow.
    #[msg("Cancel not allowed (no cancel_after).")]
    CancelNotAllowed,

    /// The cancel window has not yet opened.
    #[msg("Too early to cancel.")]
    NotExpired,

    /// The release proof failed on-chain verification.
    #[msg("Release proof verification failed.")]
    ProofVerificationFailed,

    /// A lamport balance update overflowed.
    #[msg("Lamport arithmetic overflow.")]
    ArithmeticOverflow,
}
