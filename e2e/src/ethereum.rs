//! Ethereum end-to-end harness: an `anvil` instance with the Zescrow `Escrow`
//! contract deployed against a mock RISC Zero verifier and the pinned image id.
//!
//! The happy paths drive the [`zescrow_client`] `EthereumAgent`; the
//! adversarial paths issue direct contract calls (tampered journals, replays)
//! so the on-chain binding and revert behavior are exercised against the
//! deployed contract.

use std::borrow::Cow;

use alloy::network::{EthereumWallet, TransactionBuilder};
use alloy::node_bindings::{Anvil, AnvilInstance};
use alloy::primitives::{Address, B256, Bytes, U256};
use alloy::providers::{DynProvider, Provider, ProviderBuilder};
use alloy::signers::local::PrivateKeySigner;
use alloy::sol;
use alloy::sol_types::SolValue;
use anyhow::Context;
use url::Url;
use zescrow_core::{Chain, ChainConfig, Party};

use crate::pinned_image_id;

sol! {
    #[sol(rpc)]
    Escrow,
    "../client/abi/Escrow.json"
}

sol! {
    #[sol(rpc)]
    contract MockRiscZeroVerifier {
        function setAccept(bool value) external;
    }
}

const ESCROW_ARTIFACT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../agent/ethereum/out/Escrow.sol/Escrow.json"
);
const MOCK_VERIFIER_ARTIFACT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../agent/ethereum/out/MockRiscZeroVerifier.sol/MockRiscZeroVerifier.json"
);

fn provider_for(key_hex: &str, url: Url) -> anyhow::Result<DynProvider> {
    let signer: PrivateKeySigner = key_hex.parse().context("parsing anvil key")?;
    Ok(ProviderBuilder::new()
        .wallet(EthereumWallet::from(signer))
        .connect_http(url)
        .erased())
}

fn artifact_bytecode(path: &str) -> anyhow::Result<Vec<u8>> {
    let source = std::fs::read_to_string(path)
        .with_context(|| format!("reading {path} (run `forge build` in agent/ethereum)"))?;
    let json: serde_json::Value = serde_json::from_str(&source)?;
    let object = json["bytecode"]["object"]
        .as_str()
        .context("artifact missing bytecode.object")?;
    Ok(hex::decode(object.strip_prefix("0x").unwrap_or(object))?)
}

async fn deploy(
    provider: &DynProvider,
    mut code: Vec<u8>,
    ctor_args: Vec<u8>,
) -> anyhow::Result<Address> {
    use alloy::rpc::types::TransactionRequest;

    code.extend_from_slice(&ctor_args);
    let tx = TransactionRequest::default().with_deploy_code(code);
    let receipt = provider.send_transaction(tx).await?.get_receipt().await?;
    receipt
        .contract_address
        .context("deployment produced no address")
}

pub struct EthHarness {
    // Dropped last; keeps the node alive for the harness's lifetime.
    _anvil: AnvilInstance,
    endpoint: String,
    admin: DynProvider,
    escrow: Address,
    verifier: Address,
    accounts: Vec<EthAccount>,
}

pub struct EthAccount {
    pub address: Address,
    pub key_hex: String,
}

impl EthHarness {
    pub async fn launch(accept: bool) -> anyhow::Result<Self> {
        let anvil = Anvil::new().try_spawn().context("spawning anvil")?;
        let endpoint = anvil.endpoint();
        let url: Url = endpoint.parse()?;

        let accounts = anvil
            .addresses()
            .iter()
            .zip(anvil.keys())
            .map(|(address, key)| EthAccount {
                address: *address,
                key_hex: format!("0x{}", hex::encode(key.to_bytes())),
            })
            .collect::<Vec<EthAccount>>();

        let admin = provider_for(&accounts[accounts.len() - 1].key_hex, url)?;

        let verifier = deploy(
            &admin,
            artifact_bytecode(MOCK_VERIFIER_ARTIFACT)?,
            accept.abi_encode(),
        )
        .await?;
        let image = B256::from(pinned_image_id());
        let escrow = deploy(
            &admin,
            artifact_bytecode(ESCROW_ARTIFACT)?,
            (verifier, image).abi_encode_params(),
        )
        .await?;

        Ok(Self {
            _anvil: anvil,
            endpoint,
            admin,
            escrow,
            verifier,
            accounts,
        })
    }

    pub fn account(&self, index: usize) -> &EthAccount {
        &self.accounts[index]
    }

    pub fn escrow_address(&self) -> Address {
        self.escrow
    }

    pub fn chain_config(&self, sender_key_hex: &str) -> ChainConfig {
        ChainConfig {
            chain: Chain::Ethereum,
            rpc_url: self.endpoint.clone(),
            sender_private_id: sender_key_hex.to_owned(),
            agent_id: self.escrow.to_string(),
        }
    }

    pub async fn balance(&self, address: Address) -> anyhow::Result<U256> {
        Ok(self.admin.get_balance(address).await?)
    }

    pub async fn block_number(&self) -> anyhow::Result<u64> {
        Ok(self.admin.get_block_number().await?)
    }

    pub async fn escrow_state(&self, id: u64) -> anyhow::Result<Escrow::EscrowDB> {
        let contract = Escrow::new(self.escrow, self.admin.clone());
        Ok(contract.getEscrow(U256::from(id)).call().await?)
    }

    pub async fn mine(&self, blocks: u64) -> anyhow::Result<()> {
        for _ in 0..blocks {
            self.admin
                .raw_request::<_, String>(Cow::Borrowed("evm_mine"), ())
                .await?;
        }
        Ok(())
    }

    pub async fn set_verifier_accept(&self, accept: bool) -> anyhow::Result<()> {
        let contract = MockRiscZeroVerifier::new(self.verifier, self.admin.clone());
        contract
            .setAccept(accept)
            .send()
            .await?
            .get_receipt()
            .await?;
        Ok(())
    }

    pub async fn raw_finish(
        &self,
        recipient_key_hex: &str,
        id: u64,
        seal: Vec<u8>,
        journal: Vec<u8>,
    ) -> anyhow::Result<()> {
        let provider = provider_for(recipient_key_hex, self.endpoint.parse()?)?;
        let contract = Escrow::new(self.escrow, provider);
        contract
            .finishEscrow(U256::from(id), Bytes::from(seal), Bytes::from(journal))
            .send()
            .await?
            .get_receipt()
            .await?;
        Ok(())
    }

    pub fn party(address: Address) -> anyhow::Result<Party> {
        Ok(Party::for_chain(Chain::Ethereum, address.to_string())?)
    }
}
