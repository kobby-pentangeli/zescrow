//! Error types for the Zescrow client.
//!
//! Provides [`ClientError`] for all client-side operations including
//! blockchain interactions, key management, and ZK proof generation.

use thiserror::Error;

/// Errors arising from client operations.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum ClientError {
    /// The specified blockchain is not supported.
    #[error("unsupported chain: {0}")]
    UnsupportedChain(String),

    /// Chain configuration does not match the expected network.
    #[error("chain configuration mismatch: expected {expected}, got {actual}")]
    ConfigMismatch {
        /// Expected chain identifier.
        expected: String,
        /// Actual chain identifier.
        actual: String,
    },

    /// Error loading or parsing a keypair file.
    #[error("keypair error: {0}")]
    Keypair(String),

    /// Generic blockchain interaction error.
    #[error("blockchain error: {0}")]
    Blockchain(String),

    /// Ethereum-specific agent error.
    #[error("ethereum agent: {context} - {message}")]
    Ethereum {
        /// Operation context (e.g., "createEscrow", "finishEscrow").
        context: &'static str,
        /// Underlying error message.
        message: String,
    },

    /// Solana-specific agent error.
    #[error("solana agent: {context} - {message}")]
    Solana {
        /// Operation context (e.g., "create_escrow", "finish_escrow").
        context: &'static str,
        /// Underlying error message.
        message: String,
    },

    /// Error serializing or deserializing data.
    #[error("serialization error: {0}")]
    Serialization(String),

    /// Error parsing a URL.
    #[error("URL parse error: {0}")]
    UrlParse(#[from] url::ParseError),

    /// Invalid operation for the current chain context.
    #[error("invalid chain operation: {0}")]
    InvalidChainOperation(String),

    /// Solana RPC client error.
    #[error("Solana RPC error: {0}")]
    SolanaRpc(#[from] Box<solana_client::client_error::ClientError>),

    /// Solana Anchor program error.
    #[error("Anchor program error: {0}")]
    AnchorProgram(#[from] anchor_lang::prelude::ProgramError),

    /// Transaction was dropped or not confirmed.
    #[error("transaction dropped: {0}")]
    TransactionDropped(String),

    /// Asset amount exceeds representable range.
    #[error("asset amount overflow: value exceeds u64 range")]
    AssetOverflow,

    /// Expected event not found in transaction receipt.
    #[error("missing event: {0}")]
    MissingEvent(String),

    /// Error from zescrow-core library.
    #[error("core library error: {0}")]
    Core(String),
}

impl ClientError {
    /// Creates an Ethereum agent error with context.
    pub fn ethereum(context: &'static str, msg: impl ToString) -> Self {
        Self::Ethereum {
            context,
            message: msg.to_string(),
        }
    }

    /// Creates a Solana agent error with context.
    pub fn solana(context: &'static str, msg: impl ToString) -> Self {
        Self::Solana {
            context,
            message: msg.to_string(),
        }
    }

    /// Creates a transaction dropped error with details.
    pub fn tx_dropped(details: impl ToString) -> Self {
        Self::TransactionDropped(details.to_string())
    }
}

impl From<solana_client::client_error::ClientError> for ClientError {
    fn from(value: solana_client::client_error::ClientError) -> Self {
        Self::SolanaRpc(Box::new(value))
    }
}

impl From<solana_sdk::pubkey::ParsePubkeyError> for ClientError {
    fn from(value: solana_sdk::pubkey::ParsePubkeyError) -> Self {
        Self::Blockchain(value.to_string())
    }
}

impl From<zescrow_core::error::EscrowError> for ClientError {
    fn from(value: zescrow_core::error::EscrowError) -> Self {
        Self::Core(value.to_string())
    }
}
