//! End-to-end harness for Zescrow.
//!
//! Drives the [`zescrow_client`] agents through the full settlement flow---
//! `create -> fund -> finish` (with a client-generated, journal-bound receipt)
//! and the `cancel` refund path---against ephemeral local chains (`anvil` for
//! Ethereum, `solana-test-validator` for Solana), with the on-chain programs
//! pinned to a RISC Zero verifier and the guest image id.
//!
//! The live tests under [`mod@crate`]'s `tests/` are `#[ignore]`d: they require
//! provisioned local chains plus `RISC0_DEV_MODE` proving, so they run in the
//! dedicated end-to-end job, not the default `cargo test` pass. Under dev mode
//! the cryptographic seal is mocked at the verifier side; the journal binding is
//! checked for real, so the adversarial paths genuinely gate. The identical matrix is
//! run against a real Groth16 seal on x86 via `scripts/e2e-real-proof.sh`.

pub mod ethereum;
pub mod solana;

use sha2::{Digest, Sha256};
use zescrow_client::ReleaseProof;
use zescrow_client::prover::{self, ProofInput};
use zescrow_core::{Asset, BigNumber, Chain, Condition, Escrow, ExecutionState, ID, Party};

const SIGNED_MESSAGE: &[u8] = b"zescrow-e2e-harness";

/// The dev-mode seal marker the Solana mock verifier accepts.
/// The real Groth16 seal is exercised by `scripts/e2e-real-proof.sh`.
pub const SOLANA_ACCEPT_SEAL: u8 = 1;

pub struct NamedCondition {
    pub name: &'static str,
    pub condition: Condition,
}

pub fn satisfied_hashlock() -> Condition {
    let preimage = b"zescrow-e2e-preimage".to_vec();
    let hash: [u8; 32] = Sha256::digest(&preimage).into();
    Condition::hashlock(hash, preimage)
}

pub fn unsatisfied_hashlock() -> Condition {
    Condition::hashlock([0u8; 32], b"wrong-preimage".to_vec())
}

pub fn satisfied_ed25519() -> Condition {
    use ed25519_dalek::ed25519::signature::rand_core::OsRng;
    use ed25519_dalek::{Signer, SigningKey};

    let sk = SigningKey::generate(&mut OsRng);
    let signature = sk.sign(SIGNED_MESSAGE).to_bytes().to_vec();
    Condition::ed25519(
        sk.verifying_key().to_bytes(),
        SIGNED_MESSAGE.to_vec(),
        signature,
    )
}

pub fn satisfied_secp256k1() -> Condition {
    use k256::ecdsa::signature::Signer;
    use k256::ecdsa::{Signature, SigningKey};
    use k256::elliptic_curve::rand_core::OsRng;

    let sk = SigningKey::random(&mut OsRng);
    let signature: Signature = sk.sign(SIGNED_MESSAGE);
    Condition::secp256k1(
        sk.verifying_key()
            .to_encoded_point(false)
            .as_bytes()
            .to_vec(),
        SIGNED_MESSAGE.to_vec(),
        signature.to_der().as_bytes().to_vec(),
    )
}

pub fn satisfied_threshold() -> Condition {
    Condition::threshold(1, vec![satisfied_hashlock(), unsatisfied_hashlock()])
}

pub fn satisfied_nested_threshold() -> Condition {
    Condition::threshold(1, vec![satisfied_threshold()])
}

pub fn escrow_conditions() -> Vec<NamedCondition> {
    vec![
        NamedCondition {
            name: "hashlock",
            condition: satisfied_hashlock(),
        },
        NamedCondition {
            name: "ed25519",
            condition: satisfied_ed25519(),
        },
        NamedCondition {
            name: "secp256k1",
            condition: satisfied_secp256k1(),
        },
        NamedCondition {
            name: "threshold",
            condition: satisfied_threshold(),
        },
        NamedCondition {
            name: "nested-threshold",
            condition: satisfied_nested_threshold(),
        },
    ]
}

pub fn native_asset(amount: u128) -> Asset {
    Asset::native(BigNumber::from(amount))
}

pub fn pinned_image_id() -> [u8; 32] {
    let words = prover::guest_image_id();
    let mut out = [0u8; 32];
    words.iter().enumerate().for_each(|(i, w)| {
        out[i * 4..i * 4 + 4].copy_from_slice(&w.to_be_bytes());
    });
    out
}

#[allow(clippy::too_many_arguments)]
pub fn release_proof(
    chain: Chain,
    agent_id: &str,
    escrow_id: u64,
    sender: Party,
    recipient: Party,
    asset: Asset,
    condition: Condition,
) -> anyhow::Result<ReleaseProof> {
    let mut escrow = Escrow::new(sender, recipient, asset, Some(condition));
    escrow.state = ExecutionState::Funded;

    let input = ProofInput {
        agent_id: ID::for_chain(chain, agent_id)?,
        escrow_id,
        escrow,
        chain,
    };

    let proof = prover::prove(&input)?;
    Ok(ReleaseProof {
        seal: proof.encoded_seal()?,
        journal: proof.journal_bytes().to_vec(),
    })
}

pub fn solana_release_proof(
    program_id: &str,
    escrow_id: u64,
    sender: Party,
    recipient: Party,
    asset: Asset,
    condition: Condition,
) -> anyhow::Result<ReleaseProof> {
    let proven = release_proof(
        Chain::Solana,
        program_id,
        escrow_id,
        sender,
        recipient,
        asset,
        condition,
    )?;
    Ok(ReleaseProof {
        seal: vec![SOLANA_ACCEPT_SEAL],
        journal: proven.journal,
    })
}
