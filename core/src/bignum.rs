//! Arbitrary-precision integer wrapper for cross-platform serialization.
//!
//! Provides a [`BigNumber`] type that wraps `num_bigint::BigUint` with
//! implementations for `bincode` serialization (and optionally `serde`).

use bincode::de::{BorrowDecoder, Decoder};
use bincode::enc::Encoder;
use bincode::error::{DecodeError, EncodeError};
use bincode::{BorrowDecode, Decode, Encode};
use num_bigint::BigUint;
#[cfg(feature = "json")]
use {
    crate::serde::biguint_serde,
    serde::{Deserialize, Serialize},
};

/// A wrapper around `BigUint` so we can implement `bincode` traits without violating
/// orphan rules, and still support Serde/JSON via `#[cfg(feature="json")]`.
#[cfg_attr(feature = "json", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "json", serde(transparent))]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd)]
pub struct BigNumber(#[cfg_attr(feature = "json", serde(with = "biguint_serde"))] pub BigUint);

impl BigNumber {
    /// `BigNumber` from 0.
    pub fn zero() -> BigNumber {
        BigUint::from(0u64).into()
    }
}

impl<T> From<T> for BigNumber
where
    BigUint: From<T>,
{
    fn from(v: T) -> Self {
        BigNumber(BigUint::from(v))
    }
}

impl Encode for BigNumber {
    fn encode<E: Encoder>(&self, encoder: &mut E) -> std::result::Result<(), EncodeError> {
        self.0.to_bytes_le().encode(encoder)
    }
}

impl<Context> Decode<Context> for BigNumber {
    fn decode<D: Decoder>(decoder: &mut D) -> std::result::Result<Self, DecodeError> {
        let bytes = Vec::<u8>::decode(decoder)?;
        Ok(BigNumber(BigUint::from_bytes_le(&bytes)))
    }
}

impl<'de, Context> BorrowDecode<'de, Context> for BigNumber {
    fn borrow_decode<D: BorrowDecoder<'de>>(
        decoder: &mut D,
    ) -> std::result::Result<Self, DecodeError> {
        Self::decode(decoder)
    }
}

impl std::fmt::Display for BigNumber {
    /// Print the inner `BigUint` as a decimal string.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.to_str_radix(10))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode_decode(value: BigNumber) {
        let bytes = bincode::encode_to_vec(&value, bincode::config::standard()).unwrap();
        let (decoded, _): (BigNumber, _) =
            bincode::decode_from_slice(&bytes, bincode::config::standard()).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn bincode_encode_decode_raw_bytes() {
        encode_decode(BigNumber::zero());
        encode_decode(BigNumber::from(1u64));
        encode_decode(BigNumber::from(u64::MAX));
        // A value wider than u128 to exercise multi-limb magnitudes.
        let wide = BigUint::parse_bytes(b"123456789012345678901234567890123456789", 10).unwrap();
        encode_decode(BigNumber(wide));
    }

    #[test]
    fn bincode_encodes_little_endian_magnitude() {
        // 0x0102 == 258, little-endian bytes [0x02, 0x01]; the length-prefixed
        // bincode payload must contain those raw bytes, not the decimal string.
        let bytes =
            bincode::encode_to_vec(BigNumber::from(258u64), bincode::config::standard()).unwrap();
        assert_eq!(bytes, vec![2, 2, 1]);
    }
}
