//! Ethereum end-to-end tests.
//!
//! Each test is `#[ignore]`d: it requires a provisioned `anvil` plus
//! `RISC0_DEV_MODE` proving and runs in the dedicated end-to-end job. The happy
//! paths drive the real client; the adversarial paths confirm the deployed
//! contract refuses to release without a valid, bound receipt.

use alloy::primitives::{Address, U256};
use zescrow_client::{Recipient, ZescrowClient};
use zescrow_core::{Chain, ChainConfig, EscrowParams};
use zescrow_e2e::ethereum::EthHarness;
use zescrow_e2e::{
    escrow_conditions, native_asset, release_proof, satisfied_hashlock, unsatisfied_hashlock,
};

// 0.1 ETH, in wei.
const AMOUNT: u128 = 100_000_000_000_000_000;

fn escrow_params(
    config: &ChainConfig,
    sender: Address,
    recipient: Address,
    finish_after: Option<u64>,
    cancel_after: Option<u64>,
    has_conditions: bool,
) -> anyhow::Result<EscrowParams> {
    Ok(EscrowParams {
        chain_config: config.clone(),
        asset: native_asset(AMOUNT),
        sender: EthHarness::party(sender)?,
        recipient: EthHarness::party(recipient)?,
        finish_after,
        cancel_after,
        has_conditions,
    })
}

fn recipient(key_hex: &str) -> Recipient {
    key_hex
        .parse()
        .expect("recipient key is infallible to capture")
}

#[tokio::test]
#[ignore = "requires anvil + RISC0_DEV_MODE; run via the e2e job"]
async fn eth_conditioned_escrow_release() -> anyhow::Result<()> {
    let h = EthHarness::launch(true).await?;
    let (sender, beneficiary) = (h.account(0).address, h.account(1).address);
    let config = h.chain_config(&h.account(0).key_hex);
    let escrow_hex = h.escrow_address().to_string();

    let client = ZescrowClient::builder(&config)
        .recipient(recipient(&h.account(1).key_hex))
        .build()
        .await?;

    for nc in escrow_conditions() {
        let far = h.block_number().await? + 10_000;
        let params = escrow_params(&config, sender, beneficiary, None, Some(far), true)?;
        let metadata = client
            .create_escrow(&params, Some(nc.condition.commitment()))
            .await?;
        let id = metadata.escrow_id.expect("escrow id");

        let before = h.balance(h.escrow_address()).await?;
        let proof = release_proof(
            Chain::Ethereum,
            &escrow_hex,
            id,
            EthHarness::party(sender)?,
            EthHarness::party(beneficiary)?,
            native_asset(AMOUNT),
            nc.condition.clone(),
        )?;
        client.finish_escrow(&metadata, Some(proof)).await?;

        let state = h.escrow_state(id).await?;
        assert!(state.settled, "{}: escrow not settled", nc.name);
        assert_eq!(state.amount, U256::ZERO, "{}: amount not zeroed", nc.name);
        assert_eq!(
            before - h.balance(h.escrow_address()).await?,
            U256::from(AMOUNT),
            "{}: funds not released",
            nc.name
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires anvil + RISC0_DEV_MODE; run via the e2e job"]
async fn eth_unconditioned_escrow_release() -> anyhow::Result<()> {
    let h = EthHarness::launch(true).await?;
    let (sender, beneficiary) = (h.account(0).address, h.account(1).address);
    let config = h.chain_config(&h.account(0).key_hex);

    let far = h.block_number().await? + 10_000;
    let params = escrow_params(&config, sender, beneficiary, None, Some(far), false)?;
    let client = ZescrowClient::builder(&config)
        .recipient(recipient(&h.account(1).key_hex))
        .build()
        .await?;
    let metadata = client.create_escrow(&params, None).await?;
    let id = metadata.escrow_id.expect("escrow id");

    let before = h.balance(h.escrow_address()).await?;
    client.finish_escrow(&metadata, None).await?;

    assert!(h.escrow_state(id).await?.settled);
    assert_eq!(
        before - h.balance(h.escrow_address()).await?,
        U256::from(AMOUNT)
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires anvil + RISC0_DEV_MODE; run via the e2e job"]
async fn eth_conditioned_escrow_refund_on_timeout() -> anyhow::Result<()> {
    let h = EthHarness::launch(true).await?;
    let (sender, beneficiary) = (h.account(0).address, h.account(1).address);
    let config = h.chain_config(&h.account(0).key_hex);

    let cancel_at = h.block_number().await? + 3;
    let params = escrow_params(&config, sender, beneficiary, None, Some(cancel_at), true)?;
    let client = ZescrowClient::builder(&config).build().await?;
    let metadata = client
        .create_escrow(&params, Some(satisfied_hashlock().commitment()))
        .await?;
    let id = metadata.escrow_id.expect("escrow id");

    let now = h.block_number().await?;
    if cancel_at > now {
        h.mine(cancel_at - now).await?;
    }

    let before = h.balance(h.escrow_address()).await?;
    client.cancel_escrow(&metadata).await?;

    assert!(h.escrow_state(id).await?.settled);
    assert_eq!(
        before - h.balance(h.escrow_address()).await?,
        U256::from(AMOUNT)
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires anvil + RISC0_DEV_MODE; run via the e2e job"]
async fn eth_conditioned_escrow_missing_proof_reverts() -> anyhow::Result<()> {
    let h = EthHarness::launch(true).await?;
    let (sender, beneficiary) = (h.account(0).address, h.account(1).address);
    let config = h.chain_config(&h.account(0).key_hex);

    let far = h.block_number().await? + 10_000;
    let params = escrow_params(&config, sender, beneficiary, None, Some(far), true)?;
    let client = ZescrowClient::builder(&config)
        .recipient(recipient(&h.account(1).key_hex))
        .build()
        .await?;
    let metadata = client
        .create_escrow(&params, Some(satisfied_hashlock().commitment()))
        .await?;

    assert!(
        client.finish_escrow(&metadata, None).await.is_err(),
        "conditioned finish with no proof must revert"
    );
    assert!(!h.escrow_state(metadata.escrow_id.unwrap()).await?.settled);
    Ok(())
}

#[tokio::test]
#[ignore = "requires anvil + RISC0_DEV_MODE; run via the e2e job"]
async fn eth_tampered_journal_reverts() -> anyhow::Result<()> {
    let h = EthHarness::launch(true).await?;
    let (sender, beneficiary) = (h.account(0).address, h.account(1).address);
    let config = h.chain_config(&h.account(0).key_hex);
    let escrow_hex = h.escrow_address().to_string();

    let condition = satisfied_hashlock();
    let far = h.block_number().await? + 10_000;
    let params = escrow_params(&config, sender, beneficiary, None, Some(far), true)?;
    let client = ZescrowClient::builder(&config).build().await?;
    let metadata = client
        .create_escrow(&params, Some(condition.commitment()))
        .await?;
    let id = metadata.escrow_id.expect("escrow id");

    let proof = release_proof(
        Chain::Ethereum,
        &escrow_hex,
        id,
        EthHarness::party(sender)?,
        EthHarness::party(beneficiary)?,
        native_asset(AMOUNT),
        condition,
    )?;
    let mut journal = proof.journal.clone();
    // Flip a byte in the committed condition (the journal's final field).
    *journal.last_mut().expect("journal is non-empty") ^= 0xFF;

    assert!(
        h.raw_finish(&h.account(1).key_hex, id, proof.seal, journal)
            .await
            .is_err(),
        "a tampered journal must not release"
    );
    assert!(!h.escrow_state(id).await?.settled);
    Ok(())
}

#[tokio::test]
#[ignore = "requires anvil + RISC0_DEV_MODE; run via the e2e job"]
async fn eth_verifier_rejection_reverts() -> anyhow::Result<()> {
    let h = EthHarness::launch(true).await?;
    let (sender, beneficiary) = (h.account(0).address, h.account(1).address);
    let config = h.chain_config(&h.account(0).key_hex);
    let escrow_hex = h.escrow_address().to_string();

    let condition = satisfied_hashlock();
    let far = h.block_number().await? + 10_000;
    let params = escrow_params(&config, sender, beneficiary, None, Some(far), true)?;
    let client = ZescrowClient::builder(&config)
        .recipient(recipient(&h.account(1).key_hex))
        .build()
        .await?;
    let metadata = client
        .create_escrow(&params, Some(condition.commitment()))
        .await?;
    let id = metadata.escrow_id.expect("escrow id");

    let proof = release_proof(
        Chain::Ethereum,
        &escrow_hex,
        id,
        EthHarness::party(sender)?,
        EthHarness::party(beneficiary)?,
        native_asset(AMOUNT),
        condition,
    )?;

    h.set_verifier_accept(false).await?;
    assert!(
        client.finish_escrow(&metadata, Some(proof)).await.is_err(),
        "a rejected seal must not release"
    );
    assert!(!h.escrow_state(id).await?.settled);
    Ok(())
}

#[tokio::test]
#[ignore = "requires anvil + RISC0_DEV_MODE; run via the e2e job"]
async fn eth_replay_reverts() -> anyhow::Result<()> {
    let h = EthHarness::launch(true).await?;
    let (sender, beneficiary) = (h.account(0).address, h.account(1).address);
    let config = h.chain_config(&h.account(0).key_hex);
    let escrow_hex = h.escrow_address().to_string();

    let condition = satisfied_hashlock();
    let far = h.block_number().await? + 10_000;
    let params = escrow_params(&config, sender, beneficiary, None, Some(far), true)?;
    let client = ZescrowClient::builder(&config)
        .recipient(recipient(&h.account(1).key_hex))
        .build()
        .await?;
    let metadata = client
        .create_escrow(&params, Some(condition.commitment()))
        .await?;
    let id = metadata.escrow_id.expect("escrow id");

    let proof = release_proof(
        Chain::Ethereum,
        &escrow_hex,
        id,
        EthHarness::party(sender)?,
        EthHarness::party(beneficiary)?,
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

#[tokio::test]
#[ignore = "requires anvil + RISC0_DEV_MODE; run via the e2e job"]
async fn eth_unauthorized_finish_reverts() -> anyhow::Result<()> {
    let h = EthHarness::launch(true).await?;
    let (sender, beneficiary) = (h.account(0).address, h.account(1).address);
    let config = h.chain_config(&h.account(0).key_hex);

    let far = h.block_number().await? + 10_000;
    let params = escrow_params(&config, sender, beneficiary, None, Some(far), false)?;
    let client = ZescrowClient::builder(&config).build().await?;
    let metadata = client.create_escrow(&params, None).await?;

    // A third party signs the finish; the contract checks msg.sender.
    let intruder = ZescrowClient::builder(&config)
        .recipient(recipient(&h.account(2).key_hex))
        .build()
        .await?;
    assert!(
        intruder.finish_escrow(&metadata, None).await.is_err(),
        "a non-recipient must not finish"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires anvil + RISC0_DEV_MODE; run via the e2e job"]
async fn eth_too_early_finish_reverts() -> anyhow::Result<()> {
    let h = EthHarness::launch(true).await?;
    let (sender, beneficiary) = (h.account(0).address, h.account(1).address);
    let config = h.chain_config(&h.account(0).key_hex);

    let base = h.block_number().await?;
    let params = escrow_params(
        &config,
        sender,
        beneficiary,
        Some(base + 100),
        Some(base + 200),
        false,
    )?;
    let client = ZescrowClient::builder(&config)
        .recipient(recipient(&h.account(1).key_hex))
        .build()
        .await?;
    let metadata = client.create_escrow(&params, None).await?;

    assert!(
        client.finish_escrow(&metadata, None).await.is_err(),
        "finishing before the finish block must revert"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires anvil + RISC0_DEV_MODE; run via the e2e job"]
async fn eth_unsatisfied_condition_yields_no_receipt() {
    let sender = EthHarness::party(Address::repeat_byte(0x11)).unwrap();
    let beneficiary = EthHarness::party(Address::repeat_byte(0x22)).unwrap();
    let result = release_proof(
        Chain::Ethereum,
        "0x00000000000000000000000000000000DeaDBeef",
        1,
        sender,
        beneficiary,
        native_asset(AMOUNT),
        unsatisfied_hashlock(),
    );
    assert!(
        result.is_err(),
        "an unsatisfied condition must not yield a receipt"
    );
}
