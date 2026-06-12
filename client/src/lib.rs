//! Zescrow client library for cross-chain escrow operations.
//!
//! This crate provides the [`ZescrowClient`] for creating, finishing, and
//! canceling escrows across multiple blockchains.
//!
//! # Supported Chains
//!
//! - **Ethereum**: Via [`EthereumAgent`]
//! - **Solana**: Via [`SolanaAgent`]
//!
//! # Features
//!
//! - `prover`: Enables RISC Zero zkVM proof generation via `zescrow-prover` (opt-in)
//!
//! # Example
//!
//! ```ignore
//! use zescrow_client::{ZescrowClient, Recipient};
//! use zescrow_core::interface::ChainConfig;
//!
//! async fn create_escrow(config: &ChainConfig) -> anyhow::Result<()> {
//!     let client = ZescrowClient::builder(config).build().await?;
//!     // Use client.create_escrow(), client.finish_escrow(), etc.
//!     Ok(())
//! }
//! ```

use std::path::PathBuf;

pub use error::ClientError;
pub use ethereum::EthereumAgent;
pub use solana::SolanaAgent;
use tracing::{debug, info};
use zescrow_core::interface::ChainConfig;
use zescrow_core::{Chain, EscrowMetadata, EscrowParams};

pub mod error;
pub mod ethereum;
pub mod solana;

/// Re-export of the prover crate when the `prover` feature is enabled.
#[cfg(feature = "prover")]
pub use zescrow_prover as prover;

/// Result type alias using [`ClientError`].
pub type Result<T> = std::result::Result<T, ClientError>;

/// A verifying release proof for a conditioned escrow, carried from the prover
/// to the on-chain settlement call.
#[derive(Debug, Clone, Default)]
pub struct ReleaseProof {
    /// Selector-prefixed receipt seal.
    pub seal: Vec<u8>,
    /// Journal bytes the guest committed.
    pub journal: Vec<u8>,
}

/// Core interface for blockchain-specific escrow operations.
///
/// Implementors must provide chain-specific logic for:
/// - Creating escrow contracts/programs and/or accounts
/// - Releasing funds to beneficiaries
/// - Refunding expired escrows
///
/// # Thread Safety
///
/// All implementations must be `Send + Sync` to support async execution.
#[async_trait::async_trait]
pub trait Agent: Send + Sync {
    /// Creates a new escrow with the specified parameters.
    ///
    /// # Arguments
    ///
    /// * `params` - Escrow creation parameters including assets, parties, and timelocks
    /// * `condition` - The release-condition commitment for a conditioned escrow,
    ///   or `None` for an unconditioned, time-lock-only escrow
    ///
    /// # Returns
    ///
    /// Metadata containing chain-specific identifiers and the escrow state.
    ///
    /// # Errors
    ///
    /// Returns an error if transaction submission or confirmation fails.
    async fn create_escrow(
        &self,
        params: &EscrowParams,
        condition: Option<[u8; 32]>,
    ) -> Result<EscrowMetadata>;

    /// Releases escrowed funds to the beneficiary.
    ///
    /// # Arguments
    ///
    /// * `metadata` - Escrow metadata from creation
    /// * `proof` - The verifying release proof for a conditioned escrow, or
    ///   `None` for an unconditioned escrow
    ///
    /// # Preconditions
    ///
    /// - Escrow must be in funded state
    /// - Current block/slot must be at or after `finish_after` (if set)
    /// - Caller must be the recipient
    /// - A conditioned escrow requires a `proof` that verifies on-chain
    ///
    /// # Errors
    ///
    /// Returns an error if the caller is not authorized, timelocks are not met,
    /// or a conditioned escrow's proof fails on-chain verification.
    async fn finish_escrow(
        &self,
        metadata: &EscrowMetadata,
        proof: Option<ReleaseProof>,
    ) -> Result<()>;

    /// Refunds escrowed funds to the depositor.
    ///
    /// # Arguments
    ///
    /// * `metadata` - Escrow metadata from creation
    ///
    /// # Preconditions
    ///
    /// - `cancel_after` must be set in the escrow
    /// - Current block/slot must be at or after `cancel_after`
    /// - Caller must be the sender
    ///
    /// # Errors
    ///
    /// Returns an error if cancellation is not allowed or timelocks are not met.
    async fn cancel_escrow(&self, metadata: &EscrowMetadata) -> Result<()>;
}

/// Unified client for cross-chain escrow management.
///
/// Wraps a chain-specific [`Agent`] to provide a consistent interface
/// for escrow operations across chains.
pub struct ZescrowClient {
    /// The underlying blockchain agent.
    pub agent: Box<dyn Agent>,
}

/// Builder for constructing [`ZescrowClient`] instances.
///
/// Use [`ZescrowClient::builder`] to create a new builder.
pub struct ZescrowClientBuilder {
    config: ChainConfig,
    recipient: Option<Recipient>,
}

/// A recipient signing credential, kept as the raw input and interpreted
/// against the target [`Chain`] at build time:
/// - Ethereum: a hex-encoded wallet private key
/// - Solana: a path to a keypair JSON file
#[derive(Debug, Clone)]
pub struct Recipient(String);

impl ZescrowClient {
    /// Creates a new builder for constructing a client.
    ///
    /// # Arguments
    ///
    /// * `config` - Chain configuration specifying the target blockchain
    pub fn builder(config: &ChainConfig) -> ZescrowClientBuilder {
        ZescrowClientBuilder {
            config: config.clone(),
            recipient: None,
        }
    }

    /// Creates an escrow on-chain.
    ///
    /// # Arguments
    ///
    /// * `params` - Parameters defining the escrow terms
    ///
    /// # Returns
    ///
    /// Metadata for the created escrow, including chain-specific identifiers.
    pub async fn create_escrow(
        &self,
        params: &EscrowParams,
        condition: Option<[u8; 32]>,
    ) -> Result<EscrowMetadata> {
        let metadata = self.agent.create_escrow(params, condition).await?;
        debug!(?metadata, "Escrow created");
        Ok(metadata)
    }

    /// Releases an existing escrow to the recipient.
    ///
    /// # Arguments
    ///
    /// * `metadata` - Escrow metadata from creation
    /// * `proof` - The verifying release proof for a conditioned escrow, or
    ///   `None` for an unconditioned escrow
    pub async fn finish_escrow(
        &self,
        metadata: &EscrowMetadata,
        proof: Option<ReleaseProof>,
    ) -> Result<()> {
        self.agent
            .finish_escrow(metadata, proof)
            .await
            .inspect(|_| {
                debug!("Escrow released");
            })
    }

    /// Cancels an existing escrow and refunds the sender.
    ///
    /// # Arguments
    ///
    /// * `metadata` - Escrow metadata from creation
    pub async fn cancel_escrow(&self, metadata: &EscrowMetadata) -> Result<()> {
        self.agent.cancel_escrow(metadata).await.inspect(|_| {
            debug!("Escrow cancelled");
        })
    }
}

impl ZescrowClientBuilder {
    /// Sets the recipient key for finish operations.
    ///
    /// This is required when calling [`ZescrowClient::finish_escrow`].
    pub fn recipient(mut self, recipient: Recipient) -> Self {
        self.recipient = Some(recipient);
        self
    }

    /// Builds the client, instantiating the appropriate chain agent.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The recipient key type doesn't match the chain
    /// - Agent initialization fails
    pub async fn build(self) -> Result<ZescrowClient> {
        debug!("Building ZescrowClient with config: {:?}", self.config);

        let ZescrowClientBuilder { config, recipient } = self;
        let agent: Box<dyn Agent> = match config.chain {
            Chain::Ethereum => {
                let key = recipient.map(|r| r.0);
                debug!(recipient_present = key.is_some(), "Selected EthereumAgent");
                Box::new(EthereumAgent::new(&config, key).await?)
            }
            Chain::Solana => {
                let keypair_path = recipient.map(|r| PathBuf::from(r.0));
                debug!(
                    keypair_present = keypair_path.is_some(),
                    "Selected SolanaAgent"
                );
                Box::new(SolanaAgent::new(&config, keypair_path).await?)
            }
        };

        info!("Agent initialized successfully");
        Ok(ZescrowClient { agent })
    }
}

impl std::str::FromStr for Recipient {
    type Err = std::convert::Infallible;

    /// Captures the raw recipient input; its meaning (Ethereum key vs. Solana
    /// keypair path) is resolved against the chain when the client is built.
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(Self(s.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipient_captures_raw_input() {
        let eth: Recipient = "0xfeedface".parse().unwrap();
        assert_eq!(eth.0, "0xfeedface");
        let solana: Recipient = "/home/user/id.json".parse().unwrap();
        assert_eq!(solana.0, "/home/user/id.json");
    }

    #[test]
    fn ethereum_error_carries_context_and_message() {
        let message = ClientError::ethereum("createEscrow", "boom").to_string();
        assert!(message.contains("createEscrow"));
        assert!(message.contains("boom"));
    }
}
