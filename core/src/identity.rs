//! Chain-agnostic identity types for escrow participants.
//!
//! Supports multiple encoding formats:
//! - Hexadecimal (with optional `0x` prefix)
//! - Base58 (used by Solana)
//! - Base64 (standard encoding)
//! - Raw bytes
//!
//! When the chain is known, prefer [`ID::for_chain`] / [`Party::for_chain`],
//! which select the encoding deterministically. [`FromStr`] retains
//! best-effort auto-detection for contexts where the chain is not yet known.

use std::str::FromStr;

use base64::Engine;
use base64::prelude::*;
use bincode::{Decode, Encode};
#[cfg(feature = "json")]
use serde::{Deserialize, Serialize};

use crate::error::IdentityError;
use crate::{Chain, EscrowError, Result};

/// Maximum allowed length of the input string before decoding.
/// Prevents arbitrarily‐long Base58/hex/base64 blobs.
const MAX_ID_LEN: usize = 256;

/// A participant in the escrow protocol, wrapping a chain-agnostic `ID`.
///
/// A `Party` represents an on-chain account or public-key identity.  
/// Internally it holds an `ID`, which may have been encoded as hex, Base58, Base64,
/// or raw bytes.
#[cfg_attr(feature = "json", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Encode, Decode, PartialEq, Eq, Hash)]
pub struct Party {
    /// The participant’s on-chain identity.
    identity: ID,
}

/// Supported encoding formats for on-chain identities.
#[cfg_attr(feature = "json", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "lowercase"))]
#[derive(Debug, Clone, Encode, Decode, PartialEq, Eq, Hash)]
pub enum ID {
    /// Hex-encoded string.
    Hex(String),
    /// Base58-encoded string.
    Base58(String),
    /// Base64-encoded string.
    Base64(String),
    /// Raw bytes.
    #[cfg_attr(feature = "json", serde(with = "serde_bytes"))]
    Bytes(Vec<u8>),
}

impl Party {
    /// Parses a `Party` from a string-encoded identity.
    ///
    /// The input is a string-encoded id in any of the supported formats:
    /// - **Hex** (with or without `0x` prefix),
    /// - **Base58**,
    /// - **Base64**,
    /// - or direct raw bytes (`ID::Bytes(Vec<u8>)`).
    ///
    /// # Errors
    ///
    /// Returns `EscrowError::Identity` if the input is empty or cannot be
    /// decoded into a valid byte sequence.
    ///
    /// # Examples
    ///
    /// ```
    /// # use zescrow_core::Party;
    ///
    /// let party = Party::new("0xdeadbeef").unwrap();
    /// assert_eq!(party.to_string(), "deadbeef");
    /// ```
    pub fn new<S: AsRef<str>>(id_str: S) -> Result<Self> {
        let identity = ID::from_str(id_str.as_ref())?;
        Ok(Self { identity })
    }

    /// Parses a `Party` using the encoding for `chain`, removing the
    /// ambiguity of auto-detection. See [`ID::for_chain`].
    ///
    /// # Errors
    ///
    /// Returns `EscrowError::Identity` if the input is empty, too long, or not
    /// valid under the chain's encoding.
    pub fn for_chain<S: AsRef<str>>(chain: Chain, id_str: S) -> Result<Self> {
        let identity = ID::for_chain(chain, id_str.as_ref())?;
        Ok(Self { identity })
    }

    /// Verifies that the underlying [`ID`] can be decoded into raw bytes.
    ///
    /// # Errors
    ///
    /// - `Err(EscrowError::Identity(_))` if decoding fails.
    pub fn verify_identity(&self) -> Result<()> {
        self.identity.validate()
    }
}

impl FromStr for Party {
    type Err = EscrowError;

    /// Parses an instance of `Self` from a string, alias for [`Self::new`].
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Party::new(s)
    }
}

impl std::fmt::Display for Party {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.identity)
    }
}

impl ID {
    const HEX: &'static str = "hex";
    const BASE58: &'static str = "base58";
    const BASE64: &'static str = "base64";
    const BYTES: &'static str = "bytes";

    /// Verifies that self can be decoded into raw bytes, and that it's not empty.
    ///
    /// # Errors
    ///
    /// - `EscrowError::Identity` if decoding fails, or is empty.
    pub fn validate(&self) -> Result<()> {
        let id_bytes = self.to_bytes()?;
        if id_bytes.is_empty() {
            return Err(IdentityError::EmptyIdentity.into());
        }
        Ok(())
    }

    /// Decode this `ID` into its raw byte representation.
    ///
    /// Depending on the variant:
    /// - **Hex**: decodes the lowercase hex string (e.g. `"0xdeadbeef"`) into bytes.
    /// - **Base58**: decodes the Base58 string into bytes.
    /// - **Base64**: decodes the Base64 string into bytes.
    /// - **Bytes**: clones and returns the inner `Vec<u8>`.
    ///
    /// # Errors
    ///
    /// An `IdentityError` corresponding to the failing ID type.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let decoded = match self {
            Self::Hex(s) => {
                let stripped = s.strip_prefix("0x").unwrap_or(s);
                hex::decode(stripped).map_err(IdentityError::Hex)
            }
            Self::Base58(s) => bs58::decode(s).into_vec().map_err(IdentityError::Base58),
            Self::Base64(s) => BASE64_STANDARD.decode(s).map_err(IdentityError::Base64),
            Self::Bytes(b) => Ok(b.clone()),
        }?;
        Ok(decoded)
    }

    /// Returns the hex string representation of the identity.
    ///
    /// # Errors
    ///
    /// Returns an `EscrowError::Identity` if the underlying bytes cannot be obtained.
    pub fn to_hex(&self) -> Result<String> {
        let bytes = self.to_bytes()?;
        Ok(hex::encode(bytes))
    }

    /// Returns the Base58 string representation of the identity.
    pub fn to_base58(&self) -> Result<String> {
        let bytes = self.to_bytes()?;
        Ok(bs58::encode(bytes).into_string())
    }

    /// Returns the Base64 string representation of the identity.
    pub fn to_base64(&self) -> Result<String> {
        let bytes = self.to_bytes()?;
        Ok(BASE64_STANDARD.encode(bytes))
    }

    /// Returns the encoding variant as a `&'static str`.
    pub fn encoding(&self) -> &'static str {
        match self {
            Self::Hex(_) => Self::HEX,
            Self::Base58(_) => Self::BASE58,
            Self::Base64(_) => Self::BASE64,
            Self::Bytes(_) => Self::BYTES,
        }
    }
}

impl std::fmt::Display for ID {
    /// Returns the canonical string representation of this `ID`.
    ///
    /// - **Hex**: lowercase hex string without prefix.
    /// - **Base58**: canonical Base58 string.
    /// - **Base64**: standard Base64 string.
    /// - **Bytes**: standard Base64 string of bytes.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Hex(s) => write!(f, "{s}"),
            Self::Base58(s) => write!(f, "{s}"),
            Self::Base64(s) => write!(f, "{s}"),
            Self::Bytes(b) => write!(f, "{}", BASE64_STANDARD.encode(b)),
        }
    }
}

impl FromStr for ID {
    type Err = EscrowError;

    /// Best-effort auto-detection by trial decode, in precedence order
    /// hex -> Base58 -> Base64. Because many byte strings decode validly under
    /// more than one scheme, the first match wins and can mislabel the
    /// encoding. When the chain is known, use [`ID::for_chain`] instead, which
    /// is unambiguous.
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Self::validate_length(s)?;
        let raw = Self::strip_hex_prefix(s.trim());
        Self::ensure_non_empty(raw)?;

        Self::try_decode_hex(raw)
            .or_else(|| Self::try_decode_base58(raw))
            .or_else(|| Self::try_decode_base64(raw))
            .ok_or_else(|| IdentityError::UnsupportedFormat.into())
    }
}

impl ID {
    /// Parses `s` into an identity using the encoding for `chain`:
    /// Ethereum identities are hex (with optional `0x` prefix), Solana
    /// identities are Base58. Resolving the encoding from the known chain
    /// removes the ambiguity of [`FromStr`]'s trial-decode precedence.
    ///
    /// # Errors
    ///
    /// Returns `EscrowError::Identity` if the input is empty, exceeds the
    /// maximum length, or is not valid under the chain's encoding.
    pub fn for_chain(chain: Chain, s: &str) -> Result<Self> {
        let trimmed = s.trim();
        Self::validate_length(trimmed)?;

        let id = match chain {
            Chain::Ethereum => {
                let bytes =
                    hex::decode(Self::strip_hex_prefix(trimmed)).map_err(IdentityError::Hex)?;
                Self::Hex(hex::encode(bytes))
            }
            Chain::Solana => {
                let bytes = bs58::decode(trimmed)
                    .into_vec()
                    .map_err(IdentityError::Base58)?;
                Self::Base58(bs58::encode(bytes).into_string())
            }
        };

        id.validate().map(|_| id)
    }

    /// Validates that the input string does not exceed the maximum allowed length.
    fn validate_length(s: &str) -> Result<()> {
        (s.len() <= MAX_ID_LEN).then_some(()).ok_or_else(|| {
            IdentityError::InputTooLong {
                len: s.len(),
                max: MAX_ID_LEN,
            }
            .into()
        })
    }

    /// Strips the `0x` or `0X` prefix from a hex string if present.
    fn strip_hex_prefix(s: &str) -> &str {
        s.strip_prefix("0x")
            .or_else(|| s.strip_prefix("0X"))
            .unwrap_or(s)
    }

    /// Ensures the input string is not empty.
    fn ensure_non_empty(s: &str) -> Result<()> {
        (!s.is_empty())
            .then_some(())
            .ok_or_else(|| IdentityError::EmptyIdentity.into())
    }

    /// Attempts to decode a hex string into an `ID::Hex`.
    /// Handles optional `0x` prefix.
    fn try_decode_hex(s: &str) -> Option<Self> {
        let stripped = s.strip_prefix("0x").unwrap_or(s);
        hex::decode(stripped)
            .ok()
            .map(|bytes| Self::Hex(hex::encode(bytes)))
    }

    /// Attempts to decode a Base58 string into an `ID::Base58`.
    fn try_decode_base58(s: &str) -> Option<Self> {
        bs58::decode(s)
            .into_vec()
            .ok()
            .map(|bytes| Self::Base58(bs58::encode(bytes).into_string()))
    }

    /// Attempts to decode a Base64 string into an `ID::Base64`.
    fn try_decode_base64(s: &str) -> Option<Self> {
        BASE64_STANDARD
            .decode(s)
            .ok()
            .map(|bytes| Self::Base64(BASE64_STANDARD.encode(bytes)))
    }
}

impl From<Vec<u8>> for ID {
    fn from(bytes: Vec<u8>) -> Self {
        ID::Bytes(bytes)
    }
}

impl From<&[u8]> for ID {
    fn from(bytes: &[u8]) -> Self {
        ID::Bytes(bytes.to_vec())
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn hex_identity() {
        let id_str = "deadbeef";
        let id = ID::from_str(id_str).unwrap();
        assert_eq!(id, ID::Hex("deadbeef".into()));
        assert_eq!(id.to_bytes().unwrap(), vec![0xde, 0xad, 0xbe, 0xef]);
        assert_eq!(id.to_hex().unwrap(), id_str);
        assert_eq!(id.encoding(), "hex");
    }

    #[test]
    fn hex_with_prefix() {
        let id_str = "0XDEADBEEF";
        let id = ID::from_str(id_str).unwrap();
        assert_eq!(id, ID::Hex("deadbeef".into()));
    }

    #[test]
    fn base58_identity() {
        let raw = vec![1, 2, 3, 4];
        let b58_str = bs58::encode(&raw).into_string();
        let id = ID::from_str(&b58_str).unwrap();
        assert_eq!(id, ID::Base58(b58_str.clone()));
        assert_eq!(id.to_base58().unwrap(), b58_str);
        assert_eq!(id.encoding(), "base58");
    }

    #[test]
    fn base64_identity() {
        let raw = vec![1, 2, 3, 4];
        let b64 = BASE64_STANDARD.encode(&raw);
        let id = ID::from_str(&b64).unwrap();
        assert_eq!(id, ID::Base64(b64.clone()));
        assert_eq!(id.to_base64().unwrap(), b64);
        assert_eq!(id.encoding(), "base64");
    }

    #[test]
    fn bytes_identity() {
        let raw = vec![9, 8, 7];
        let id: ID = raw.clone().into();
        assert_eq!(id, ID::Bytes(raw.clone()));
        assert_eq!(id.to_bytes().unwrap(), raw);
        assert_eq!(id.to_string(), BASE64_STANDARD.encode(&raw));
        assert_eq!(id.encoding(), "bytes");
    }

    #[test]
    fn verify_identity() {
        let party = Party::new("0xdeadbeef").unwrap();
        assert_eq!(party.to_string(), "deadbeef");
        assert!(party.verify_identity().is_ok());
    }

    #[test]
    fn invalid_identity() {
        assert!(ID::from_str("not a valid ID").is_err());
    }

    #[test]
    fn for_chain_is_unambiguous() {
        // "deadbeef" decodes validly as hex, Base58, and Base64; auto-detect
        // resolves it to hex by precedence, but the chain context is explicit.
        let eth = ID::for_chain(Chain::Ethereum, "0xDEADBEEF").unwrap();
        assert_eq!(eth, ID::Hex("deadbeef".into()));
        assert_eq!(eth.to_bytes().unwrap(), vec![0xde, 0xad, 0xbe, 0xef]);

        let raw = vec![1u8, 2, 3, 4];
        let b58 = bs58::encode(&raw).into_string();
        let sol = ID::for_chain(Chain::Solana, &b58).unwrap();
        assert_eq!(sol, ID::Base58(b58));
        assert_eq!(sol.to_bytes().unwrap(), raw);
    }

    #[test]
    fn for_chain_rejects_invalid() {
        // 'g' is not a hex digit; empty Solana input has no bytes.
        assert!(ID::for_chain(Chain::Ethereum, "0xZZ").is_err());
        assert!(ID::for_chain(Chain::Solana, "").is_err());
    }

    #[test]
    fn id_from_str_input_too_long() {
        let oversized = "x".repeat(MAX_ID_LEN + 1);
        let err = ID::from_str(&oversized).unwrap_err();
        match err {
            EscrowError::Identity(IdentityError::InputTooLong { len, max }) => {
                assert_eq!(len, MAX_ID_LEN + 1);
                assert_eq!(max, MAX_ID_LEN);
            }
            _ => panic!("Expected IdentityError::InputTooLong, got {:?}", err),
        }
    }
}
