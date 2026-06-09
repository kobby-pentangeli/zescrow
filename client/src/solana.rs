//! Solana blockchain agent implementation.
//!
//! Provides [`SolanaAgent`] for interacting with the Zescrow Solana
//! program. Supports creating, finishing, and canceling escrows on Solana.

use core::str::FromStr;
use std::path::{Path, PathBuf};

use anchor_lang::{InstructionData, system_program};
use escrow::{CreateEscrowArgs, ESCROW, instruction as escrow_instruction};
use num_traits::ToPrimitive;
use solana_client::rpc_client::RpcClient;
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, read_keypair_file};
use solana_sdk::signer::Signer;
use solana_sdk::transaction::Transaction;
use tracing::{debug, info, trace};
use zescrow_core::interface::ChainConfig;
use zescrow_core::{EscrowMetadata, EscrowParams, ExecutionState};

use super::Agent;
use crate::Result;
use crate::error::ClientError;

// Instruction names for logging.
const CREATE_ESCROW: &str = "create_escrow";
const FINISH_ESCROW: &str = "finish_escrow";
const CANCEL_ESCROW: &str = "cancel_escrow";

/// Solana blockchain agent for escrow operations.
///
/// Manages interactions with the Zescrow Solana program,
/// including transaction building, signing, and submission.
pub struct SolanaAgent {
    /// JSON-RPC client for the Solana cluster.
    client: RpcClient,
    /// Keypair of the escrow creator (sender).
    sender_keypair: Keypair,
    /// Optional keypair of the escrow beneficiary (recipient).
    recipient_keypair: Option<Keypair>,
    /// Program ID of the deployed escrow program.
    escrow_program_id: Pubkey,
}

impl SolanaAgent {
    /// Creates a new Solana agent from chain configuration.
    ///
    /// # Arguments
    ///
    /// * `config` - Chain configuration containing RPC URL and sender keypair path
    /// * `recipient_keypair_path` - Optional path to recipient keypair for finish operations
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Keypair files cannot be read
    /// - Program ID parsing fails
    pub async fn new(
        config: &ChainConfig,
        recipient_keypair_path: Option<PathBuf>,
    ) -> Result<Self> {
        let ChainConfig {
            rpc_url,
            sender_private_id,
            agent_id,
            ..
        } = config;

        let sender_keypair = Self::load_keypair(sender_private_id, "sender")?;
        debug!(sender = %sender_keypair.pubkey(), "Loaded sender keypair");

        let recipient_keypair = recipient_keypair_path
            .map(|path| Self::load_keypair(&path, "recipient"))
            .transpose()?;

        if let Some(ref kp) = recipient_keypair {
            debug!(recipient = %kp.pubkey(), "Loaded recipient keypair");
        }

        let escrow_program_id =
            Self::parse_pubkey(agent_id).map_err(|e| ClientError::solana("parse_program_id", e))?;
        info!(%escrow_program_id, "Using escrow program");

        Ok(Self {
            client: RpcClient::new(rpc_url),
            sender_keypair,
            recipient_keypair,
            escrow_program_id,
        })
    }

    /// Reads a Solana keypair from a file path.
    fn load_keypair(path: impl AsRef<Path>, name: &str) -> Result<Keypair> {
        read_keypair_file(path.as_ref())
            .map_err(|e| ClientError::Keypair(format!("failed to load {} keypair: {}", name, e)))
    }

    /// Derives the escrow PDA from sender and recipient public keys.
    fn derive_escrow_pda(&self, sender: &Pubkey, recipient: &Pubkey) -> Pubkey {
        let (pda, _bump) = Pubkey::find_program_address(
            &[ESCROW, sender.as_ref(), recipient.as_ref()],
            &self.escrow_program_id,
        );
        pda
    }

    /// Parses a public key from a party's string representation.
    fn parse_pubkey(party: &impl ToString) -> Result<Pubkey> {
        Pubkey::from_str(&party.to_string()).map_err(Into::into)
    }

    /// Builds the create_escrow instruction.
    fn build_create_instruction(
        &self,
        sender: Pubkey,
        recipient: Pubkey,
        escrow_pda: Pubkey,
        args: CreateEscrowArgs,
    ) -> Instruction {
        Instruction {
            program_id: self.escrow_program_id,
            accounts: vec![
                AccountMeta::new(sender, true),
                AccountMeta::new_readonly(recipient, false),
                AccountMeta::new(escrow_pda, false),
                AccountMeta::new_readonly(system_program::ID, false),
            ],
            data: InstructionData::data(&escrow_instruction::CreateEscrow { args }),
        }
    }

    /// Builds the finish_escrow instruction.
    fn build_finish_instruction(&self, recipient: Pubkey, escrow_pda: Pubkey) -> Instruction {
        Instruction {
            program_id: self.escrow_program_id,
            accounts: vec![
                AccountMeta::new(recipient, true),
                AccountMeta::new(escrow_pda, false),
            ],
            data: InstructionData::data(&escrow_instruction::FinishEscrow {}),
        }
    }

    /// Builds the cancel_escrow instruction.
    fn build_cancel_instruction(&self, sender: Pubkey, escrow_pda: Pubkey) -> Instruction {
        Instruction {
            program_id: self.escrow_program_id,
            accounts: vec![
                AccountMeta::new(sender, true),
                AccountMeta::new(escrow_pda, false),
            ],
            data: InstructionData::data(&escrow_instruction::CancelEscrow {}),
        }
    }

    /// Signs and submits a transaction.
    fn submit_transaction(
        &self,
        instruction: Instruction,
        payer: &Pubkey,
        signers: &[&Keypair],
        operation: &'static str,
    ) -> Result<()> {
        let recent_hash = self
            .client
            .get_latest_blockhash()
            .map_err(|e| ClientError::solana(operation, e))?;

        debug!(%recent_hash, "Fetched recent blockhash");

        let tx =
            Transaction::new_signed_with_payer(&[instruction], Some(payer), signers, recent_hash);

        self.client
            .send_and_confirm_transaction(&tx)
            .map_err(|e| ClientError::solana(operation, e))?;

        Ok(())
    }

    /// Returns the recipient keypair, or an error if not configured.
    fn recipient_keypair(&self) -> Result<&Keypair> {
        self.recipient_keypair
            .as_ref()
            .ok_or_else(|| ClientError::solana(FINISH_ESCROW, "recipient keypair not configured"))
    }

    /// Verifies that a keypair matches the expected public key.
    fn validate_keypair(keypair: &Keypair, expected: &Pubkey, role: &str) -> Result<()> {
        (keypair.pubkey() == *expected)
            .then_some(())
            .ok_or_else(|| {
                ClientError::Keypair(format!(
                    "{} keypair mismatch: expected {}, got {}",
                    role,
                    expected,
                    keypair.pubkey()
                ))
            })
    }
}

#[async_trait::async_trait]
impl Agent for SolanaAgent {
    async fn create_escrow(&self, params: &EscrowParams) -> Result<EscrowMetadata> {
        let sender = Self::parse_pubkey(&params.sender)?;
        Self::validate_keypair(&self.sender_keypair, &sender, "sender")?;

        let recipient = Self::parse_pubkey(&params.recipient)?;
        let amount = params
            .asset
            .amount()
            .0
            .to_u64()
            .ok_or(ClientError::AssetOverflow)?;
        trace!(%amount, "Computed escrow amount");

        let escrow_pda = self.derive_escrow_pda(&sender, &recipient);
        info!(%escrow_pda, "Derived escrow PDA");

        let args = CreateEscrowArgs {
            amount,
            finish_after: params.finish_after,
            cancel_after: params.cancel_after,
        };

        let instruction = self.build_create_instruction(sender, recipient, escrow_pda, args);
        debug!("{} instruction built", CREATE_ESCROW);

        self.submit_transaction(instruction, &sender, &[&self.sender_keypair], CREATE_ESCROW)?;
        info!("{} transaction confirmed", CREATE_ESCROW);

        Ok(EscrowMetadata {
            params: params.clone(),
            state: ExecutionState::Funded,
            escrow_id: None,
        })
    }

    async fn finish_escrow(&self, metadata: &EscrowMetadata) -> Result<()> {
        let sender = Self::parse_pubkey(&metadata.params.sender)?;
        let recipient = Self::parse_pubkey(&metadata.params.recipient)?;

        let recipient_keypair = self.recipient_keypair()?;
        Self::validate_keypair(recipient_keypair, &recipient, "recipient")?;

        let escrow_pda = self.derive_escrow_pda(&sender, &recipient);
        debug!(%escrow_pda, "Using escrow PDA");

        let instruction = self.build_finish_instruction(recipient, escrow_pda);
        debug!("{} instruction built", FINISH_ESCROW);

        self.submit_transaction(instruction, &recipient, &[recipient_keypair], FINISH_ESCROW)?;
        info!("{} transaction confirmed", FINISH_ESCROW);

        Ok(())
    }

    async fn cancel_escrow(&self, metadata: &EscrowMetadata) -> Result<()> {
        let sender = Self::parse_pubkey(&metadata.params.sender)?;
        let recipient = Self::parse_pubkey(&metadata.params.recipient)?;

        let escrow_pda = self.derive_escrow_pda(&sender, &recipient);
        debug!(%escrow_pda, "Using escrow PDA");

        let instruction = self.build_cancel_instruction(sender, escrow_pda);
        debug!("{} instruction built", CANCEL_ESCROW);

        self.submit_transaction(instruction, &sender, &[&self.sender_keypair], CANCEL_ESCROW)?;
        info!("{} transaction confirmed", CANCEL_ESCROW);

        Ok(())
    }
}
