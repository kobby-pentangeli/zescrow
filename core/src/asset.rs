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
#[cfg_attr(feature = "json", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
#[derive(Debug, Clone, Encode, Decode)]
pub struct Asset {
    /// Kind of asset.
    pub kind: AssetKind,

    /// Unique identity of the asset on-chain.
    pub id: Option<ID>,

    /// Associated on-chain program ID or contract address of the asset.
    pub agent_id: Option<ID>,

    /// Amount in the smallest unit (e.g., wei, lamports).
    pub amount: BigNumber,

    /// Number of decimals the asset uses.
    pub decimals: Option<u8>,

    /// Total supply of the asset in circulation.
    pub total_supply: Option<BigNumber>,
}

/// Different kinds of assets we might escrow on any chain.
#[cfg_attr(feature = "json", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "snake_case"))]
#[derive(Debug, Clone, Encode, Decode)]
pub enum AssetKind {
    /// Native chain coin (e.g., ETH, SOL).
    Native,
    /// Fungible token (e.g., ERC-20, SPL).
    Token,
    /// Non-fungible token (e.g., ERC-721, SPL NFT)
    Nft,
    /// Multi-token (e.g., ERC-1155) with fractional ownership via `amount`.
    MultiToken,
    /// Liquidity pool share (proportional ownership).
    LpShare,
}

impl Asset {
    /// Create a native asset.
    pub fn native(amount: BigNumber) -> Self {
        Self {
            kind: AssetKind::Native,
            id: None,
            agent_id: None,
            amount,
            decimals: None,
            total_supply: None,
        }
    }

    /// Create a fungible token asset.
    pub fn token(contract: ID, amount: BigNumber, total_supply: BigNumber, decimals: u8) -> Self {
        Self {
            kind: AssetKind::Token,
            id: None,
            agent_id: Some(contract),
            amount,
            decimals: Some(decimals),
            total_supply: Some(total_supply),
        }
    }

    /// Create an NFT asset.
    pub fn nft(contract: ID, token_id: ID) -> Self {
        Self {
            kind: AssetKind::Nft,
            id: Some(token_id),
            agent_id: Some(contract),
            amount: BigNumber::from(1u64),
            decimals: None,
            total_supply: None,
        }
    }

    /// Create a multi-token asset.
    pub fn multi_token(contract: ID, token_id: ID, amount: BigNumber) -> Self {
        Self {
            kind: AssetKind::MultiToken,
            id: Some(token_id),
            agent_id: Some(contract),
            amount,
            decimals: None,
            total_supply: None,
        }
    }

    /// Create a liquidity pool share asset.
    pub fn pool_share(
        pool_id: ID,
        share: BigNumber,
        total_supply: BigNumber,
        decimals: u8,
    ) -> Self {
        Self {
            kind: AssetKind::LpShare,
            id: Some(pool_id),
            agent_id: None,
            amount: share,
            decimals: Some(decimals),
            total_supply: Some(total_supply),
        }
    }

    /// Ensure asset parameters are semantically valid.
    ///
    /// - **Native**: `amount` must be > 0.
    /// - **Token**: `amount` must be > 0, `contract` must be valid `ID`.
    /// - **MultiToken**: `amount` must be > 0, `contract` must be valid `ID`, `token_id` cannot be empty.
    /// - **Nft**: `contract` must be valid `ID`, `token_id` cannot be empty.
    /// - **PoolShare**: `share` must be > 0, `total_supply` must be > 0, and `share` <= `total_supply`.
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
            AssetKind::Nft | AssetKind::MultiToken => self.validate_agent_and_token_id(),
            AssetKind::LpShare => self.validate_pool_share(),
        }
    }

    /// Validates that the agent ID is present and valid.
    fn validate_agent_id(&self) -> Result<()> {
        self.agent_id
            .as_ref()
            .ok_or_else(|| AssetError::MissingId.into())
            .and_then(ID::validate)
    }

    /// Validates both agent ID and token ID are present and valid.
    fn validate_agent_and_token_id(&self) -> Result<()> {
        self.validate_agent_id().and_then(|_| {
            self.id
                .as_ref()
                .ok_or_else(|| AssetError::MissingId.into())
                .and_then(ID::validate)
        })
    }

    /// Validates pool share constraints: pool ID, total supply, and share ratio.
    fn validate_pool_share(&self) -> Result<()> {
        self.validate_pool_id()
            .and_then(|_| self.validate_total_supply())
            .and_then(|total| self.validate_share_ratio(total))
    }

    /// Validates the pool ID is present and valid.
    fn validate_pool_id(&self) -> Result<()> {
        self.id
            .as_ref()
            .ok_or_else(|| AssetError::MissingId.into())
            .and_then(ID::validate)
    }

    /// Validates total supply is present and non-zero.
    fn validate_total_supply(&self) -> Result<&BigNumber> {
        self.total_supply
            .as_ref()
            .ok_or_else(|| AssetError::MissingTotalSupply.into())
            .and_then(|total| {
                (*total != BigNumber::zero())
                    .then_some(total)
                    .ok_or_else(|| AssetError::ZeroAmount.into())
            })
    }

    /// Validates that share does not exceed total supply.
    fn validate_share_ratio(&self, total_supply: &BigNumber) -> Result<()> {
        (self.amount <= *total_supply).then_some(()).ok_or_else(|| {
            AssetError::InvalidShare(self.amount.clone(), total_supply.clone()).into()
        })
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
    /// let supply = BigNumber::from(2_000u64);
    ///
    /// let original = Asset::token(ID::from(vec![4, 5, 6]), amount, supply, 18);
    /// let bytes = original.to_bytes().unwrap();
    /// let decoded = Asset::from_bytes(&bytes).unwrap();
    /// assert_eq!(format!("{:?}", decoded), format!("{:?}", original));
    /// ```
    pub fn from_bytes(src: &[u8]) -> Result<Self> {
        bincode::decode_from_slice(src, bincode::config::standard())
            .map_err(|e| AssetError::Parsing(e.to_string()).into())
            .map(|(asset, _)| asset)
    }

    /// Format raw `BigNumber` with fixed‐point decimals.
    pub fn format_amount(&self) -> Result<String> {
        let decimals = self.decimals.unwrap_or(0);
        let factor = BigUint::from(10u8).pow(decimals as u32);
        let (whole, rem) = self.amount.0.div_rem(&factor);

        let rem_str = if decimals > 0 {
            let s = rem.to_str_radix(10);
            format!("{:0>width$}", s, width = decimals as usize)
        } else {
            String::new()
        };

        Ok(if decimals > 0 {
            format!("{}.{}", whole.to_str_radix(10), rem_str)
        } else {
            whole.to_str_radix(10)
        })
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
        // supply and amount = 1000, decimals = 9
        let token = Asset::token(ID::from(vec![4, 5, 6]), to_bignum(1000), to_bignum(1000), 9);
        assert!(token.validate().is_ok());

        // empty program ID
        let empty_agent_id = Asset::token(ID::from(Vec::new()), to_bignum(100), to_bignum(100), 9);
        assert!(empty_agent_id.validate().is_err());

        // zero amount
        let zero_token = Asset::token(ID::from(vec![1, 2, 3]), to_bignum(0), to_bignum(100), 6);
        assert!(zero_token.validate().is_err());
    }

    #[test]
    fn nft() {
        // valid NFT: contract and token_id both non-empty
        let nft = Asset::nft(ID::from(vec![7, 8, 9]), ID::from("zescrowNFT".as_bytes()));
        assert!(nft.validate().is_ok());

        // empty token ID
        let empty_token_id = Asset::nft(ID::from(vec![7, 8, 9]), ID::from(Vec::new()));
        assert!(empty_token_id.validate().is_err());

        // empty contract ID
        let empty_contract_id = Asset::nft(ID::from(Vec::new()), ID::from("zescrowNFT".as_bytes()));
        assert!(empty_contract_id.validate().is_err());
    }

    #[test]
    fn multi_token() {
        // valid multi-token
        let asset = Asset::multi_token(
            ID::from(vec![1]),
            ID::from("zescrowToken".as_bytes()),
            to_bignum(500),
        );
        assert!(asset.validate().is_ok());

        // zero amount
        let zero_amt = Asset::multi_token(
            ID::from(vec![1]),
            ID::from("zescrowToken".as_bytes()),
            to_bignum(0),
        );
        assert!(zero_amt.validate().is_err());

        // empty token ID
        let bad_id = Asset::multi_token(ID::from(vec![1]), ID::from(Vec::new()), to_bignum(10));
        assert!(bad_id.validate().is_err());

        // empty contract ID
        let bad_contract = Asset::multi_token(
            ID::from(Vec::new()),
            ID::from("zescrowToken".as_bytes()),
            to_bignum(10),
        );
        assert!(bad_contract.validate().is_err());
    }

    #[test]
    fn pool_share() {
        // valid pool share
        let share = to_bignum(50);
        let total = to_bignum(100);
        let valid = Asset::pool_share(ID::from(vec![1]), share.clone(), total.clone(), 0);
        assert!(valid.validate().is_ok());

        // zero share
        let zero_share = Asset::pool_share(ID::from(vec![1]), to_bignum(0), total.clone(), 0);
        assert!(zero_share.validate().is_err());

        // zero total supply
        let zero_total = Asset::pool_share(ID::from(vec![1]), share.clone(), to_bignum(0), 0);
        assert!(zero_total.validate().is_err());

        // empty pool ID
        let bad_pool = Asset::pool_share(ID::from(Vec::new()), share.clone(), total.clone(), 0);
        assert!(bad_pool.validate().is_err());

        // share exceeds total supply
        let too_many = Asset::pool_share(ID::from(vec![1]), to_bignum(150), to_bignum(100), 0);
        assert!(too_many.validate().is_err());
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
        let original = Asset::token(
            ID::from(vec![0xde, 0xad, 0xbe, 0xef]),
            to_bignum(1_000),
            to_bignum(1_000_000),
            18,
        );
        let bytes = original.to_bytes().unwrap();
        let decoded = Asset::from_bytes(&bytes).unwrap();
        assert!(matches!(decoded.kind, AssetKind::Token));
        assert_eq!(decoded.amount, original.amount);
        assert_eq!(decoded.decimals, Some(18));
    }

    #[test]
    fn format_amount_with_decimals() {
        let asset = Asset::token(ID::from(vec![1]), to_bignum(1_500_000_000), to_bignum(0), 9);
        assert_eq!(asset.format_amount().unwrap(), "1.500000000");
    }

    #[test]
    fn format_amount_without_decimals() {
        let asset = Asset::native(to_bignum(12345));
        assert_eq!(asset.format_amount().unwrap(), "12345");
    }
}
