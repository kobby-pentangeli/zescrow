//! Deterministic cryptographic conditions and fulfillment verification.
//!
//! This module defines the [`Condition`] enum and its variants for
//! verifying cryptographic proofs within the zkVM:
//!
//! - **Hashlock**: SHA-256 preimage verification
//! - **Ed25519**: EdDSA signature verification
//! - **Secp256k1**: ECDSA signature verification
//! - **Threshold**: N-of-M multi-condition logic

use bincode::{Decode, Encode};
#[cfg(feature = "json")]
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::Result;
use crate::error::ConditionError;

/// Ed25519 signature over an arbitrary message.
pub mod ed25519;
/// XRPL-style hashlock: SHA-256(preimage) == hash.
pub mod hashlock;
/// Secp256k1 ECDSA signature over an arbitrary message.
pub mod secp256k1;
/// Threshold condition: at least `threshold` subconditions must hold.
pub mod threshold;

use ed25519::Ed25519;
use hashlock::Hashlock;
use secp256k1::Secp256k1;
use threshold::Threshold;

/// A cryptographic condition that can be deterministically verified.
#[cfg_attr(feature = "json", derive(Serialize, Deserialize))]
#[cfg_attr(
    feature = "json",
    serde(tag = "condition", content = "fulfillment", rename_all = "lowercase")
)]
#[derive(Debug, Clone, Encode, Decode, PartialEq, Eq)]
pub enum Condition {
    /// XRPL-style hashlock: SHA-256(preimage) == hash.
    Hashlock(Hashlock),
    /// Ed25519 signature over an arbitrary message.
    Ed25519(Ed25519),
    /// Secp256k1 ECDSA signature over an arbitrary message.
    Secp256k1(Secp256k1),
    /// Threshold condition: at least `threshold` subconditions must hold.
    Threshold(Threshold),
}

impl Condition {
    /// Validates the provided witness data against this cryptographic condition.
    ///
    /// # Errors
    ///
    /// Returns `EscrowError::Condition` under any of the following circumstances:
    /// - **Hashlock**: `SHA-256(preimage)` does not match the expected hash.
    /// - **Ed25519**: Public key parsing or signature verification fails.
    /// - **Secp256k1**: Public key parsing or signature verification fails.
    /// - **Threshold**: Fewer than `threshold` subconditions were satisfied.
    #[inline]
    pub fn verify(&self) -> Result<()> {
        match self {
            Self::Hashlock(hashlock) => hashlock.verify().map_err(ConditionError::Hashlock)?,
            Self::Ed25519(ed25519) => ed25519.verify().map_err(ConditionError::Ed25519)?,
            Self::Secp256k1(secp256k1) => secp256k1.verify().map_err(ConditionError::Secp256k1)?,
            Self::Threshold(threshold) => threshold.verify().map_err(ConditionError::Threshold)?,
        }
        Ok(())
    }

    /// Construct a hashlock (preimage) condition.
    pub fn hashlock(hash: [u8; 32], preimage: Vec<u8>) -> Self {
        Self::Hashlock(Hashlock { hash, preimage })
    }

    /// Construct an Ed25519 signature condition.
    pub fn ed25519(public_key: [u8; 32], message: Vec<u8>, signature: Vec<u8>) -> Self {
        Self::Ed25519(Ed25519 {
            public_key,
            signature,
            message,
        })
    }

    /// Construct a Secp256k1 signature condition.
    pub fn secp256k1(public_key: Vec<u8>, message: Vec<u8>, signature: Vec<u8>) -> Self {
        Self::Secp256k1(Secp256k1 {
            public_key,
            signature,
            message,
        })
    }

    /// Construct a threshold condition.
    pub fn threshold(threshold: usize, subconditions: Vec<Self>) -> Self {
        Self::Threshold(Threshold {
            threshold,
            subconditions,
        })
    }

    /// A 32-byte SHA-256 commitment binding to this condition's public
    /// parameters, excluding the secret witness.
    pub fn commitment(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        match self {
            Self::Hashlock(hashlock) => {
                hasher.update(b"ZESCROW_COND_HASHLOCK_V1");
                hasher.update(hashlock.hash);
            }
            Self::Ed25519(ed25519) => {
                hasher.update(b"ZESCROW_COND_ED25519_V1");
                hasher.update(ed25519.public_key);
                hasher.update(&ed25519.message);
            }
            Self::Secp256k1(secp256k1) => {
                hasher.update(b"ZESCROW_COND_SECP256K1_V1");
                hasher.update(&secp256k1.public_key);
                hasher.update(&secp256k1.message);
            }
            Self::Threshold(threshold) => {
                hasher.update(b"ZESCROW_COND_THRESHOLD_V1");
                hasher.update(saturating_u32(threshold.threshold).to_be_bytes());
                hasher.update(saturating_u32(threshold.subconditions.len()).to_be_bytes());
                threshold
                    .subconditions
                    .iter()
                    .for_each(|sub| hasher.update(sub.commitment()));
            }
        }
        hasher.finalize().into()
    }
}

/// Normalizes a `usize` count to a fixed-width `u32` for cross-platform
/// (32-bit guest vs 64-bit host) commitment determinism. Counts this large are
/// unreachable for valid conditions; saturating keeps the function total.
fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(feature = "json")]
impl std::fmt::Display for Condition {
    /// Serialize the condition to compact JSON for logging or write formats.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let json = serde_json::to_string(self).map_err(|_| std::fmt::Error)?;
        write!(f, "{json}")
    }
}

#[cfg(test)]
mod tests {

    use sha2::{Digest, Sha256};

    use super::*;

    #[test]
    fn preimage() {
        let preimage = b"secret".to_vec();
        let hash = Sha256::digest(&preimage).into();
        let cond = Condition::hashlock(hash, preimage);
        assert!(cond.verify().is_ok());

        // invalid preimage
        let cond = Condition::hashlock(hash, b"wrong-secret".to_vec());
        assert!(cond.verify().is_err());
    }

    #[test]
    fn ed25519() {
        use ed25519_dalek::ed25519::signature::rand_core::OsRng;
        use ed25519_dalek::{Signer, SigningKey};

        let mut csprng = OsRng;
        let sk: SigningKey = SigningKey::generate(&mut csprng);

        let message = b"zkEscrow".to_vec();
        let signature = sk.sign(&message).to_bytes().to_vec();
        let public_key = sk.verifying_key().to_bytes();
        let cond = Condition::ed25519(public_key, message.clone(), signature.clone());
        assert!(cond.verify().is_ok());

        // tampered sig
        let mut signature = signature;
        signature[0] ^= 0xFF;
        let cond = Condition::ed25519(public_key, message, signature);
        assert!(cond.verify().is_err());
    }

    #[test]
    fn secp256k1() {
        use k256::ecdsa::signature::Signer;
        use k256::ecdsa::{Signature, SigningKey};
        use k256::elliptic_curve::rand_core::OsRng;

        let sk = SigningKey::random(&mut OsRng);
        let vk = sk.verifying_key();
        let message = b"zkEscrow".to_vec();
        let signature: Signature = sk.sign(&message);

        let sig_bytes = signature.to_der().as_bytes().to_vec();
        let pk_bytes = vk.to_encoded_point(false).as_bytes().to_vec();

        let cond = Condition::secp256k1(pk_bytes.clone(), message, sig_bytes.clone());
        assert!(cond.verify().is_ok());

        // tampered message
        let cond = Condition::secp256k1(pk_bytes, b"tampered".to_vec(), sig_bytes);
        assert!(cond.verify().is_err());
    }

    #[test]
    fn nonzero_threshold() {
        // two trivial subconditions: one succeeds, one fails
        let hash = Sha256::digest(b"zkEscrow").into();
        let correct = Condition::hashlock(hash, b"zkEscrow".to_vec());
        let wrong = Condition::hashlock(hash, b"wrong-preimage".to_vec());

        // threshold == 1 should pass
        let cond = Condition::threshold(1, vec![correct.clone(), wrong.clone()]);
        assert!(cond.verify().is_ok());

        // threshold == 2 should fail
        let cond = Condition::threshold(2, vec![correct, wrong]);
        assert!(cond.verify().is_err());

        // threshold == 1 and no subconditions should fail
        let cond = Condition::threshold(1, vec![]);
        assert!(cond.verify().is_err());
    }

    #[test]
    fn zero_threshold_rejected() {
        // threshold == 0 is malformed and must be rejected, not trivially satisfied
        let cond = Condition::threshold(0, vec![]);
        assert!(cond.verify().is_err());

        let preimage = b"zkEscrow".to_vec();
        let hash = Sha256::digest(&preimage).into();
        let subcond = Condition::hashlock(hash, preimage);
        let cond = Condition::threshold(0, vec![subcond]);
        assert!(cond.verify().is_err());
    }

    #[test]
    fn threshold_exceeding_subconditions_rejected() {
        let preimage = b"zkEscrow".to_vec();
        let hash = Sha256::digest(&preimage).into();
        let subcond = Condition::hashlock(hash, preimage);
        // threshold of 2 over a single subcondition can never be met
        let cond = Condition::threshold(2, vec![subcond]);
        assert!(cond.verify().is_err());
    }

    #[test]
    fn nested_thresholds() {
        let preimage = b"zkEscrow".to_vec();
        let hash = Sha256::digest(&preimage).into();
        let leaf = Condition::hashlock(hash, preimage);

        // inner threshold: need 1 of `leaf`
        let inner = Condition::threshold(1, vec![leaf.clone()]);
        // outer threshold: need 1 of `inner`
        let outer = Condition::threshold(1, vec![inner]);
        assert!(outer.verify().is_ok());

        // if `leaf` wrong, `inner` fails, and so does `outer`
        let wrong_leaf = Condition::hashlock(hash, b"wrong-preimage".to_vec());
        let inner2 = Condition::threshold(1, vec![wrong_leaf]);
        let outer2 = Condition::threshold(1, vec![inner2]);
        assert!(outer2.verify().is_err());
    }

    #[cfg(feature = "json")]
    #[test]
    fn json_roundtrip_hashlock() {
        let preimage = b"secret".to_vec();
        let hash = Sha256::digest(&preimage).into();
        let cond = Condition::hashlock(hash, preimage);
        let json = serde_json::to_string(&cond).unwrap();
        let decoded: Condition = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, cond);
    }

    #[cfg(feature = "json")]
    #[test]
    fn json_roundtrip_threshold() {
        let preimage = b"nested".to_vec();
        let hash = Sha256::digest(&preimage).into();
        let inner = Condition::hashlock(hash, preimage);
        let cond = Condition::threshold(1, vec![inner]);
        let json = serde_json::to_string(&cond).unwrap();
        let decoded: Condition = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, cond);
    }
}
