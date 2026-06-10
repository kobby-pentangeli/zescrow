//! Ethereum blockchain agent implementation.
//!
//! Provides [`EthereumAgent`] for interacting with the Zescrow Ethereum
//! smart contract. Supports creating, finishing, and canceling escrows
//! on Ethereum and EVM-compatible chains.

use alloy::network::EthereumWallet;
use alloy::primitives::{Address, B256, Bytes, U256};
use alloy::providers::{DynProvider, Provider, ProviderBuilder};
use alloy::rpc::types::TransactionReceipt;
use alloy::signers::local::PrivateKeySigner;
use alloy::sol;
use tracing::{debug, info};
use url::Url;
use zescrow_core::{ChainConfig, EscrowMetadata, EscrowParams, ExecutionState};

use crate::error::ClientError;
use crate::{Agent, ReleaseProof, Result};

// Operation labels for logging and error context.
const CREATE: &str = "createEscrow";
const FINISH: &str = "finishEscrow";
const CANCEL: &str = "cancelEscrow";

sol! {
    #[sol(rpc)]
    Escrow,
    "abi/Escrow.json"
}

/// Ethereum blockchain agent for escrow operations.
///
/// Holds a sender-signing provider for create/cancel and an optional
/// recipient-signing provider for finish.
pub struct EthereumAgent {
    /// Deployed escrow contract address.
    escrow_address: Address,
    /// Provider signing as the escrow sender.
    sender: DynProvider,
    /// Provider signing as the recipient, for finish operations.
    recipient: Option<DynProvider>,
}

impl EthereumAgent {
    /// Creates a new Ethereum agent from chain configuration.
    ///
    /// # Arguments
    ///
    /// * `config` - Chain configuration containing the RPC URL, sender key, and
    ///   contract address
    /// * `recipient_key` - Optional recipient private key for finish operations
    ///
    /// # Errors
    ///
    /// Returns an error if the RPC URL, contract address, or a signing key
    /// cannot be parsed, or if the initial chain-id query fails.
    pub async fn new(config: &ChainConfig, recipient_key: Option<String>) -> Result<Self> {
        let ChainConfig {
            rpc_url,
            sender_private_id,
            agent_id,
            ..
        } = config;

        let escrow_address = Self::parse_address(agent_id)?;
        let url: Url = rpc_url.parse()?;

        let sender = Self::provider(sender_private_id, url.clone())?;
        let chain_id = sender
            .get_chain_id()
            .await
            .map_err(|e| ClientError::ethereum("get_chain_id", e))?;
        debug!(chain_id, "Connected to Ethereum");

        let recipient = recipient_key
            .map(|key| Self::provider(&key, url))
            .transpose()?;

        Ok(Self {
            escrow_address,
            sender,
            recipient,
        })
    }

    /// Builds a type-erased provider that signs with `private_key`, applying the
    /// recommended nonce, gas, and chain-id fillers.
    fn provider(private_key: &str, url: Url) -> Result<DynProvider> {
        let signer: PrivateKeySigner = private_key
            .parse()
            .map_err(|e| ClientError::ethereum("parse_key", e))?;
        let provider = ProviderBuilder::new()
            .wallet(EthereumWallet::from(signer))
            .connect_http(url);
        Ok(provider.erased())
    }

    /// Parses a 20-byte EVM address from a hex string, tolerating a `0x` prefix
    /// and the unprefixed lowercase form produced by `Party`'s display.
    fn parse_address(s: &str) -> Result<Address> {
        let hex_str = s
            .strip_prefix("0x")
            .or_else(|| s.strip_prefix("0X"))
            .unwrap_or(s);
        let bytes = hex::decode(hex_str).map_err(|e| ClientError::ethereum("parse_address", e))?;
        let array: [u8; 20] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| ClientError::ethereum("parse_address", "expected a 20-byte address"))?;
        Ok(Address::from(array))
    }

    /// Converts the escrow amount to a `U256`, failing if it is not a
    /// representable unsigned integer.
    fn amount(params: &EscrowParams) -> Result<U256> {
        params
            .asset
            .amount()
            .to_string()
            .parse::<U256>()
            .map_err(|_| ClientError::AssetOverflow)
    }

    /// Reads the escrow id recorded at creation, required to address the escrow.
    fn escrow_id(metadata: &EscrowMetadata, operation: &'static str) -> Result<u64> {
        metadata
            .escrow_id
            .ok_or_else(|| ClientError::ethereum(operation, "missing escrow_id"))
    }

    /// Extracts the new escrow id from the `EscrowCreated` log in a receipt.
    fn created_id(receipt: &TransactionReceipt) -> Result<u64> {
        let id = receipt
            .inner
            .logs()
            .iter()
            .find_map(|log| log.log_decode::<Escrow::EscrowCreated>().ok())
            .map(|log| log.inner.data.escrowId)
            .ok_or_else(|| ClientError::MissingEvent("EscrowCreated event not found".into()))?;

        u64::try_from(id).map_err(|_| ClientError::MissingEvent("escrow_id exceeds u64".into()))
    }

    /// Returns the recipient-signing provider, or an error if not configured.
    fn recipient_provider(&self) -> Result<DynProvider> {
        self.recipient
            .clone()
            .ok_or_else(|| ClientError::ethereum(FINISH, "recipient wallet not configured"))
    }
}

#[async_trait::async_trait]
impl Agent for EthereumAgent {
    async fn create_escrow(
        &self,
        params: &EscrowParams,
        condition: Option<[u8; 32]>,
    ) -> Result<EscrowMetadata> {
        let recipient = Self::parse_address(&params.recipient.to_string())?;
        let finish_after = U256::from(params.finish_after.unwrap_or_default());
        let cancel_after = U256::from(params.cancel_after.unwrap_or_default());
        let amount = Self::amount(params)?;
        let condition_id = B256::from(condition.unwrap_or_default());

        info!(%amount, "Sending {CREATE} transaction");

        let contract = Escrow::new(self.escrow_address, self.sender.clone());
        let receipt = contract
            .createEscrow(recipient, finish_after, cancel_after, condition_id)
            .value(amount)
            .send()
            .await
            .map_err(|e| ClientError::ethereum(CREATE, e))?
            .get_receipt()
            .await
            .map_err(|e| ClientError::ethereum(CREATE, e))?;

        let escrow_id = Self::created_id(&receipt)?;
        info!(escrow_id, "{CREATE} confirmed");

        Ok(EscrowMetadata {
            params: params.clone(),
            state: ExecutionState::Funded,
            escrow_id: Some(escrow_id),
        })
    }

    async fn finish_escrow(
        &self,
        metadata: &EscrowMetadata,
        proof: Option<ReleaseProof>,
    ) -> Result<()> {
        let id = Self::escrow_id(metadata, FINISH)?;
        let contract = Escrow::new(self.escrow_address, self.recipient_provider()?);

        // An unconditioned escrow submits empty seal/journal; the contract
        // ignores them and releases on the time-lock alone.
        let (seal, journal) = proof
            .map(|p| (Bytes::from(p.seal), Bytes::from(p.journal)))
            .unwrap_or_default();

        info!(id, "Sending {FINISH} transaction");
        contract
            .finishEscrow(U256::from(id), seal, journal)
            .send()
            .await
            .map_err(|e| ClientError::ethereum(FINISH, e))?
            .get_receipt()
            .await
            .map_err(|e| ClientError::ethereum(FINISH, e))?;

        info!(id, "{FINISH} confirmed");
        Ok(())
    }

    async fn cancel_escrow(&self, metadata: &EscrowMetadata) -> Result<()> {
        let id = Self::escrow_id(metadata, CANCEL)?;
        let contract = Escrow::new(self.escrow_address, self.sender.clone());

        info!(id, "Sending {CANCEL} transaction");
        contract
            .cancelEscrow(U256::from(id))
            .send()
            .await
            .map_err(|e| ClientError::ethereum(CANCEL, e))?
            .get_receipt()
            .await
            .map_err(|e| ClientError::ethereum(CANCEL, e))?;

        info!(id, "{CANCEL} confirmed");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A checksummed address and the lowercase, unprefixed form `Party`'s display
    // produces must resolve to the same 20-byte address.
    #[test]
    fn parse_address_ignores_case_and_prefix() {
        let checksummed =
            EthereumAgent::parse_address("0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045");
        let bare = EthereumAgent::parse_address("d8da6bf26964af9d7eed9e03e53415d37aa96045");
        assert_eq!(checksummed.unwrap(), bare.unwrap());
    }

    #[test]
    fn parse_address_rejects_wrong_length() {
        assert!(EthereumAgent::parse_address("0xdeadbeef").is_err());
    }

    #[test]
    fn parse_address_rejects_non_hex() {
        assert!(
            EthereumAgent::parse_address("0xnothexnothexnothexnothexnothexnothexnoth").is_err()
        );
    }
}
