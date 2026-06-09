//! Chain-agnostic asset representations and utilities.

#[cfg(feature = "json")]
use std::str::FromStr;

use bincode::{Decode, Encode};
use num_bigint::BigUint;
use num_integer::Integer;
#[cfg(feature = "json")]
use serde::{Deserialize, Serialize};
#[cfg(feature = "json")]
use serde_json;

#[cfg(feature = "json")]
use crate::EscrowError;
use crate::error::AssetError;
use crate::{BigNumber, ID, Result};

/// Represents an on-chain asset.
///
/// Restricted to the asset kinds the on-chain agents actually settle: the native
/// coin and fungible tokens. Non-fungible, multi-token, and pool-share
/// settlement are deferred until an agent can honor them.
#[cfg_attr(feature = "json", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
#[derive(Debug, Clone, Encode, Decode)]
pub struct Asset {
    /// Kind of asset.
    pub kind: AssetKind,

    /// On-chain contract address (ERC-20) or mint (SPL) for a [`AssetKind::Token`];
    /// `None` for the native coin.
    pub agent_id: Option<ID>,

    /// Amount in the smallest unit (e.g., wei, lamports).
    pub amount: BigNumber,

    /// Number of decimals the asset uses, for display formatting.
    pub decimals: Option<u8>,
}

/// Fungible asset kinds the escrow can settle on-chain.
#[cfg_attr(feature = "json", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
#[derive(Debug, Clone, Encode, Decode)]
pub enum AssetKind {
    /// Native chain coin (e.g., ETH, SOL).
    Native,
    /// Fungible token (e.g., ERC-20, SPL).
    Token,
}

impl Asset {
    /// Create a native asset.
    pub fn native(amount: BigNumber) -> Self {
        Self {
            kind: AssetKind::Native,
            agent_id: None,
            amount,
            decimals: None,
        }
    }

    /// Create a fungible token asset held at `contract` (ERC-20) or mint (SPL).
    pub fn token(contract: ID, amount: BigNumber, decimals: u8) -> Self {
        Self {
            kind: AssetKind::Token,
            agent_id: Some(contract),
            amount,
            decimals: Some(decimals),
        }
    }

    /// Ensure asset parameters are semantically valid.
    ///
    /// - **Native**: `amount` must be > 0.
    /// - **Token**: `amount` must be > 0 and `agent_id` must be a valid `ID`.
    pub fn validate(&self) -> Result<()> {
        self.validate_non_zero_amount()
            .and_then(|_| self.validate_by_kind())
    }

    /// Validates that the amount is non-zero.
    fn validate_non_zero_amount(&self) -> Result<()> {
        (self.amount != BigNumber::zero())
            .then_some(())
            .ok_or_else(|| AssetError::ZeroAmount.into())
    }

    /// Validates asset-specific requirements based on kind.
    fn validate_by_kind(&self) -> Result<()> {
        match self.kind {
            AssetKind::Native => Ok(()),
            AssetKind::Token => self.validate_agent_id(),
        }
    }

    /// Validates that the agent ID is present and valid.
    fn validate_agent_id(&self) -> Result<()> {
        self.agent_id
            .as_ref()
            .ok_or_else(|| AssetError::MissingId.into())
            .and_then(ID::validate)
    }

    /// Attempt to serialize self into a Bincode‐encoded byte vector.
    ///
    /// Uses the standard Bincode configuration to produce a compact,
    /// little‐endian binary representation.
    ///
    /// # Examples
    ///
    /// ```
    /// # use num_bigint::BigUint;
    /// # use zescrow_core::Asset;
    ///
    /// let asset = Asset::native(BigUint::from(1_000u64).into());
    /// let bytes = asset.to_bytes().expect("serialize");
    /// assert!(!bytes.is_empty());
    /// ```
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        bincode::encode_to_vec(self, bincode::config::standard())
            .map_err(|e| AssetError::Serialization(e.to_string()).into())
    }

    /// Attempt to decode Self from a Bincode‐encoded byte slice `src`.
    ///
    /// Expects `src` to match the format produced by [`Self::to_bytes`].
    ///
    /// # Examples
    ///
    /// ```
    /// # use zescrow_core::{Asset, BigNumber};
    /// # use zescrow_core::identity::ID;
    ///
    /// let amount = BigNumber::from(1_000u64);
    ///
    /// let original = Asset::token(ID::from(vec![4, 5, 6]), amount, 18);
    /// let bytes = original.to_bytes().unwrap();
    /// let decoded = Asset::from_bytes(&bytes).unwrap();
    /// assert_eq!(format!("{:?}", decoded), format!("{:?}", original));
    /// ```
    pub fn from_bytes(src: &[u8]) -> Result<Self> {
        bincode::decode_from_slice(src, bincode::config::standard())
            .map_err(|e| AssetError::Parsing(e.to_string()).into())
            .map(|(asset, _)| asset)
    }

    /// Format the raw `amount` as a fixed-point decimal string using the asset's
    /// `decimals` (zero when unset). The widening `u8 -> u32`/`usize` conversions
    /// are infallible and `BigUint` arithmetic is unbounded, so this cannot fail.
    pub fn format_amount(&self) -> String {
        let decimals = self.decimals.unwrap_or(0);
        let factor = BigUint::from(10u8).pow(u32::from(decimals));
        let (whole, rem) = self.amount.0.div_rem(&factor);

        if decimals == 0 {
            return whole.to_str_radix(10);
        }

        format!(
            "{}.{:0>width$}",
            whole.to_str_radix(10),
            rem.to_str_radix(10),
            width = usize::from(decimals)
        )
    }

    /// Returns the underlying raw quantity for this asset.
    pub fn amount(&self) -> &BigNumber {
        &self.amount
    }
}

#[cfg(feature = "json")]
impl std::fmt::Display for Asset {
    /// Compact representation for logging.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let json = serde_json::to_string(self).map_err(|_| std::fmt::Error)?;
        write!(f, "{json}")
    }
}

#[cfg(feature = "json")]
impl FromStr for Asset {
    type Err = EscrowError;

    /// Parse an `Asset` from its JSON representation.
    fn from_str(s: &str) -> Result<Self> {
        serde_json::from_str::<Self>(s).map_err(|e| AssetError::Parsing(e.to_string()).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BigNumber, ID};

    fn to_bignum(num: u64) -> BigNumber {
        BigNumber::from(num)
    }

    #[test]
    fn native() {
        let coin = Asset::native(to_bignum(1));
        assert!(coin.validate().is_ok());

        let zero_coin = Asset::native(to_bignum(0));
        assert!(zero_coin.validate().is_err());
    }

    #[test]
    fn token() {
        // amount = 1000, decimals = 9
        let token = Asset::token(ID::from(vec![4, 5, 6]), to_bignum(1000), 9);
        assert!(token.validate().is_ok());

        // empty program ID
        let empty_agent_id = Asset::token(ID::from(Vec::new()), to_bignum(100), 9);
        assert!(empty_agent_id.validate().is_err());

        // zero amount
        let zero_token = Asset::token(ID::from(vec![1, 2, 3]), to_bignum(0), 6);
        assert!(zero_token.validate().is_err());
    }

    #[test]
    fn bincode_roundtrip_native() {
        let original = Asset::native(to_bignum(1_000_000_000));
        let bytes = original.to_bytes().unwrap();
        let decoded = Asset::from_bytes(&bytes).unwrap();
        assert!(matches!(decoded.kind, AssetKind::Native));
        assert_eq!(decoded.amount, original.amount);
    }

    #[test]
    fn bincode_roundtrip_token() {
        let original = Asset::token(ID::from(vec![0xde, 0xad, 0xbe, 0xef]), to_bignum(1_000), 18);
        let bytes = original.to_bytes().unwrap();
        let decoded = Asset::from_bytes(&bytes).unwrap();
        assert!(matches!(decoded.kind, AssetKind::Token));
        assert_eq!(decoded.amount, original.amount);
        assert_eq!(decoded.decimals, Some(18));
    }

    #[test]
    fn format_amount_with_decimals() {
        let asset = Asset::token(ID::from(vec![1]), to_bignum(1_500_000_000), 9);
        assert_eq!(asset.format_amount(), "1.500000000");
    }

    #[test]
    fn format_amount_without_decimals() {
        let asset = Asset::native(to_bignum(12345));
        assert_eq!(asset.format_amount(), "12345");
    }
}
