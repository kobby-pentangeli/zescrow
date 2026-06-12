//! Solana end-to-end harness: a `solana-test-validator` with the escrow program
//! and a mock verifier deployed at the RISC Zero router address, driving the
//! [`zescrow_client`] `SolanaAgent` over RPC.
//!
//! Unlike the EVM contract, the Solana program forwards only the journal digest
//! to the verifier router, which the dev-mode mock ignores; binding correctness
//! is covered by the program's journal-reconstruction parity test and the
//! real-Groth16 run. So the Solana matrix exercises the paths the program
//! enforces at runtime---authorization, time-locks, the proof-gated finish,
//! replay, and the refund guarantee---through the actual client.

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

use anyhow::{Context, bail};
use solana_client::rpc_client::RpcClient;
use solana_commitment_config::CommitmentConfig;
use solana_sdk::native_token::LAMPORTS_PER_SOL;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, write_keypair_file};
use solana_sdk::signer::Signer;
use tempfile::TempDir;
use zescrow_core::{Chain, ChainConfig, Party};

const ESCROW_PROGRAM_ID: &str = "J4SfUoLAAsvmAWMQGa8dJHw8vsSvRfUUMXGTxcmSeS8s";
const ROUTER_ID: &str = "6JvFfBrvCcWgANKh1Eae9xDq4RC6cfJuBcf71rp2k9Y7";
const ESCROW_SO: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../agent/solana/escrow/target/deploy/escrow.so"
);
const MOCK_SO: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../agent/solana/escrow/target/deploy/mock_verifier.so"
);

pub const ONE_SOL: u64 = LAMPORTS_PER_SOL;

fn free_port() -> anyhow::Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

pub struct SolHarness {
    validator: Child,
    _ledger: TempDir,
    keydir: TempDir,
    rpc_url: String,
    client: RpcClient,
    program_id: Pubkey,
}

pub struct SolAccount {
    pub pubkey: Pubkey,
    pub keypair_path: PathBuf,
}

impl SolAccount {
    pub fn party(&self) -> anyhow::Result<Party> {
        Ok(Party::for_chain(Chain::Solana, self.pubkey.to_string())?)
    }

    pub fn path(&self) -> String {
        self.keypair_path.to_string_lossy().into_owned()
    }
}

impl SolHarness {
    pub async fn launch() -> anyhow::Result<Self> {
        for so in [ESCROW_SO, MOCK_SO] {
            if !std::path::Path::new(so).exists() {
                bail!(
                    "missing program artifact {so} (run `cargo build-sbf` in agent/solana/escrow)"
                );
            }
        }

        let ledger = tempfile::tempdir()?;
        let keydir = tempfile::tempdir()?;
        let rpc_port = free_port()?;
        let faucet_port = free_port()?;
        let rpc_url = format!("http://127.0.0.1:{rpc_port}");

        let validator = Command::new("solana-test-validator")
            .args([
                "--reset",
                "--quiet",
                "--ledger",
                ledger.path().to_str().context("ledger path")?,
                "--rpc-port",
                &rpc_port.to_string(),
                "--faucet-port",
                &faucet_port.to_string(),
                "--bpf-program",
                ESCROW_PROGRAM_ID,
                ESCROW_SO,
                "--bpf-program",
                ROUTER_ID,
                MOCK_SO,
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("spawning solana-test-validator (is it installed?)")?;

        let client = RpcClient::new_with_commitment(rpc_url.clone(), CommitmentConfig::confirmed());

        let harness = Self {
            validator,
            _ledger: ledger,
            keydir,
            rpc_url,
            client,
            program_id: ESCROW_PROGRAM_ID.parse()?,
        };
        harness.wait_until_ready()?;
        Ok(harness)
    }

    pub fn fund_account(&self, name: &str, lamports: u64) -> anyhow::Result<SolAccount> {
        let keypair = Keypair::new();
        let keypair_path = self.keydir.path().join(format!("{name}.json"));
        write_keypair_file(&keypair, &keypair_path)
            .map_err(|e| anyhow::anyhow!("writing {name} keypair: {e}"))?;
        let pubkey = keypair.pubkey();

        self.client
            .request_airdrop(&pubkey, lamports)
            .with_context(|| format!("airdrop to {name}"))?;
        self.poll(
            || Ok(self.client.get_balance(&pubkey)? >= lamports),
            "airdrop",
        )?;

        Ok(SolAccount {
            pubkey,
            keypair_path,
        })
    }

    pub fn chain_config(&self, sender_keypair_path: &str) -> ChainConfig {
        ChainConfig {
            chain: Chain::Solana,
            rpc_url: self.rpc_url.clone(),
            sender_private_id: sender_keypair_path.to_owned(),
            agent_id: self.program_id.to_string(),
        }
    }

    pub fn balance(&self, pubkey: &Pubkey) -> anyhow::Result<u64> {
        Ok(self.client.get_balance(pubkey)?)
    }

    pub fn slot(&self) -> anyhow::Result<u64> {
        Ok(self.client.get_slot()?)
    }

    pub fn escrow_exists(&self, pda: &Pubkey) -> anyhow::Result<bool> {
        Ok(self.client.get_account(pda).is_ok())
    }

    pub fn wait_for_slot(&self, target: u64) -> anyhow::Result<()> {
        self.poll(|| Ok(self.slot()? >= target), "slot")
    }

    pub fn escrow_pda(&self, sender: &Pubkey, recipient: &Pubkey, id: u64) -> Pubkey {
        Pubkey::find_program_address(
            &[
                b"escrow",
                sender.as_ref(),
                recipient.as_ref(),
                &id.to_le_bytes(),
            ],
            &self.program_id,
        )
        .0
    }

    fn wait_until_ready(&self) -> anyhow::Result<()> {
        self.poll(
            || Ok(self.client.get_health().is_ok() && self.client.get_slot()? > 0),
            "validator readiness",
        )
    }

    fn poll(&self, check: impl Fn() -> anyhow::Result<bool>, what: &str) -> anyhow::Result<()> {
        (0..100)
            .find_map(|_| match check() {
                Ok(true) => Some(Ok(())),
                _ => {
                    sleep(Duration::from_millis(400));
                    None
                }
            })
            .unwrap_or_else(|| bail!("timed out waiting for {what}"))
    }
}

impl Drop for SolHarness {
    fn drop(&mut self) {
        let _ = self.validator.kill();
        let _ = self.validator.wait();
    }
}
