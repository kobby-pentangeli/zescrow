//! Solana end-to-end tests.
//!
//! Each test is `#[ignore]`d: it requires `solana-test-validator`, the built
//! program artifacts, and `RISC0_DEV_MODE` proving, and runs in the dedicated
//! end-to-end job. The tests drive the real `SolanaAgent` over RPC; the
//! program-level binding and the verifier router are covered by the journal
//! parity test and the real-Groth16 run, so the dev-mode matrix here exercises
//! the paths the program enforces at runtime through the actual client.

use zescrow_client::{Recipient, ReleaseProof, ZescrowClient};
use zescrow_core::{ChainConfig, EscrowParams};
use zescrow_e2e::solana::{ONE_SOL, SolAccount, SolHarness};
use zescrow_e2e::{escrow_conditions, native_asset, satisfied_hashlock, solana_release_proof};

// 0.1 SOL, in lamports.
const AMOUNT: u128 = 100_000_000;

fn fund_pair(h: &SolHarness) -> anyhow::Result<(SolAccount, SolAccount)> {
    let sender = h.fund_account("sender", 5 * ONE_SOL)?;
    let recipient = h.fund_account("recipient", ONE_SOL)?;
    Ok((sender, recipient))
}

fn escrow_params(
    config: &ChainConfig,
    sender: &SolAccount,
    recipient: &SolAccount,
    finish_after: Option<u64>,
    cancel_after: Option<u64>,
    has_conditions: bool,
) -> anyhow::Result<EscrowParams> {
    Ok(EscrowParams {
        chain_config: config.clone(),
        asset: native_asset(AMOUNT),
        sender: sender.party()?,
        recipient: recipient.party()?,
        finish_after,
        cancel_after,
        has_conditions,
    })
}

fn as_recipient(keypair_path: &str) -> Recipient {
    keypair_path
        .parse()
        .expect("recipient path is infallible to capture")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires solana-test-validator + RISC0_DEV_MODE; run via the e2e job"]
async fn sol_conditioned_escrow_release() -> anyhow::Result<()> {
    let h = SolHarness::launch().await?;
    let (sender, recipient) = fund_pair(&h)?;
    let config = h.chain_config(&sender.path());
    let program_id = config.agent_id.clone();
    let client = ZescrowClient::builder(&config)
        .recipient(as_recipient(&recipient.path()))
        .build()
        .await?;

    for nc in escrow_conditions() {
        let far = h.slot()? + 1_000_000;
        let params = escrow_params(&config, &sender, &recipient, None, Some(far), true)?;
        let metadata = client
            .create_escrow(&params, Some(nc.condition.commitment()))
            .await?;
        let id = metadata.escrow_id.expect("escrow id");
        let pda = h.escrow_pda(&sender.pubkey, &recipient.pubkey, id);

        let before = h.balance(&recipient.pubkey)?;
        let proof = solana_release_proof(
            &program_id,
            id,
            sender.party()?,
            recipient.party()?,
            native_asset(AMOUNT),
            nc.condition.clone(),
        )?;
        client.finish_escrow(&metadata, Some(proof)).await?;

        assert!(!h.escrow_exists(&pda)?, "{}: escrow not closed", nc.name);
        let gained = h.balance(&recipient.pubkey)? - before;
        assert!(
            gained > AMOUNT as u64 - ONE_SOL / 100,
            "{}: recipient gained {gained}, expected ~{AMOUNT}",
            nc.name
        );
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires solana-test-validator + RISC0_DEV_MODE; run via the e2e job"]
async fn sol_unconditioned_escrow_release() -> anyhow::Result<()> {
    let h = SolHarness::launch().await?;
    let (sender, recipient) = fund_pair(&h)?;
    let config = h.chain_config(&sender.path());
    let far = h.slot()? + 1_000_000;
    let params = escrow_params(&config, &sender, &recipient, None, Some(far), false)?;
    let client = ZescrowClient::builder(&config)
        .recipient(as_recipient(&recipient.path()))
        .build()
        .await?;
    let metadata = client.create_escrow(&params, None).await?;
    let id = metadata.escrow_id.expect("escrow id");
    let pda = h.escrow_pda(&sender.pubkey, &recipient.pubkey, id);

    let before = h.balance(&recipient.pubkey)?;
    client.finish_escrow(&metadata, None).await?;

    assert!(!h.escrow_exists(&pda)?);
    assert!(h.balance(&recipient.pubkey)? > before);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires solana-test-validator + RISC0_DEV_MODE; run via the e2e job"]
async fn sol_conditioned_escrow_refund_on_timeout() -> anyhow::Result<()> {
    let h = SolHarness::launch().await?;
    let (sender, recipient) = fund_pair(&h)?;
    let config = h.chain_config(&sender.path());
    let cancel_at = h.slot()? + 8;
    let params = escrow_params(&config, &sender, &recipient, None, Some(cancel_at), true)?;
    let client = ZescrowClient::builder(&config).build().await?;
    let metadata = client
        .create_escrow(&params, Some(satisfied_hashlock().commitment()))
        .await?;
    let id = metadata.escrow_id.expect("escrow id");
    let pda = h.escrow_pda(&sender.pubkey, &recipient.pubkey, id);

    h.wait_for_slot(cancel_at)?;
    let before = h.balance(&sender.pubkey)?;
    client.cancel_escrow(&metadata).await?;

    assert!(!h.escrow_exists(&pda)?);
    assert!(h.balance(&sender.pubkey)? > before, "sender not refunded");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires solana-test-validator + RISC0_DEV_MODE; run via the e2e job"]
async fn sol_conditioned_escrow_missing_proof_reverts() -> anyhow::Result<()> {
    let h = SolHarness::launch().await?;
    let (sender, recipient) = fund_pair(&h)?;
    let config = h.chain_config(&sender.path());
    let far = h.slot()? + 1_000_000;
    let params = escrow_params(&config, &sender, &recipient, None, Some(far), true)?;
    let client = ZescrowClient::builder(&config)
        .recipient(as_recipient(&recipient.path()))
        .build()
        .await?;
    let metadata = client
        .create_escrow(&params, Some(satisfied_hashlock().commitment()))
        .await?;
    let pda = h.escrow_pda(
        &sender.pubkey,
        &recipient.pubkey,
        metadata.escrow_id.unwrap(),
    );

    assert!(
        client.finish_escrow(&metadata, None).await.is_err(),
        "conditioned finish with no proof must revert"
    );
    assert!(
        h.escrow_exists(&pda)?,
        "escrow must survive a failed finish"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires solana-test-validator + RISC0_DEV_MODE; run via the e2e job"]
async fn sol_verifier_rejection_reverts() -> anyhow::Result<()> {
    let h = SolHarness::launch().await?;
    let (sender, recipient) = fund_pair(&h)?;
    let config = h.chain_config(&sender.path());
    let far = h.slot()? + 1_000_000;
    let params = escrow_params(&config, &sender, &recipient, None, Some(far), true)?;
    let client = ZescrowClient::builder(&config)
        .recipient(as_recipient(&recipient.path()))
        .build()
        .await?;
    let metadata = client
        .create_escrow(&params, Some(satisfied_hashlock().commitment()))
        .await?;
    let pda = h.escrow_pda(
        &sender.pubkey,
        &recipient.pubkey,
        metadata.escrow_id.unwrap(),
    );

    // A seal the mock router refuses (no success marker).
    let rejected = ReleaseProof {
        seal: vec![0],
        journal: Vec::new(),
    };
    assert!(
        client
            .finish_escrow(&metadata, Some(rejected))
            .await
            .is_err(),
        "a rejected seal must not release"
    );
    assert!(h.escrow_exists(&pda)?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires solana-test-validator + RISC0_DEV_MODE; run via the e2e job"]
async fn sol_replay_reverts() -> anyhow::Result<()> {
    let h = SolHarness::launch().await?;
    let (sender, recipient) = fund_pair(&h)?;
    let config = h.chain_config(&sender.path());
    let program_id = config.agent_id.clone();
    let condition = satisfied_hashlock();
    let far = h.slot()? + 1_000_000;
    let params = escrow_params(&config, &sender, &recipient, None, Some(far), true)?;
    let client = ZescrowClient::builder(&config)
        .recipient(as_recipient(&recipient.path()))
        .build()
        .await?;
    let metadata = client
        .create_escrow(&params, Some(condition.commitment()))
        .await?;
    let id = metadata.escrow_id.expect("escrow id");

    let proof = solana_release_proof(
        &program_id,
        id,
        sender.party()?,
        recipient.party()?,
        native_asset(AMOUNT),
        condition,
    )?;
    client.finish_escrow(&metadata, Some(proof.clone())).await?;

    assert!(
        client.finish_escrow(&metadata, Some(proof)).await.is_err(),
        "a settled escrow must not be released twice"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires solana-test-validator + RISC0_DEV_MODE; run via the e2e job"]
async fn sol_too_early_finish_reverts() -> anyhow::Result<()> {
    let h = SolHarness::launch().await?;
    let (sender, recipient) = fund_pair(&h)?;
    let config = h.chain_config(&sender.path());
    let base = h.slot()?;
    let params = escrow_params(
        &config,
        &sender,
        &recipient,
        Some(base + 1_000_000),
        Some(base + 2_000_000),
        false,
    )?;
    let client = ZescrowClient::builder(&config)
        .recipient(as_recipient(&recipient.path()))
        .build()
        .await?;
    let metadata = client.create_escrow(&params, None).await?;
    let pda = h.escrow_pda(
        &sender.pubkey,
        &recipient.pubkey,
        metadata.escrow_id.unwrap(),
    );

    assert!(
        client.finish_escrow(&metadata, None).await.is_err(),
        "finishing before the finish slot must revert"
    );
    assert!(h.escrow_exists(&pda)?);
    Ok(())
}
