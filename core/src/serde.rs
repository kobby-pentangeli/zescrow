//! Serde helpers for custom (de)serialization.

/// Serde helper to (de)serialize `BigUint` as strings.
#[cfg(feature = "json")]
pub mod biguint_serde {
    use num_bigint::BigUint;
    use serde::{Deserialize, Deserializer, Serializer, de};

    /// Serialize a `BigUint` as a &str.
    pub fn serialize<S>(value: &BigUint, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        s.serialize_str(&value.to_str_radix(10))
    }

    /// Deserialize into a `BigUint`.
    pub fn deserialize<'de, D>(d: D) -> Result<BigUint, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(d)?;
        BigUint::parse_bytes(s.as_bytes(), 10)
            .ok_or_else(|| de::Error::custom(format!("invalid BigUint: {s}")))
    }
}

/// Serde helper to (de)serialize `Vec<u8>` as UTF-8 strings.
#[cfg(feature = "json")]
pub mod utf8_serde {
    use std::str;

    use serde::{Deserialize, Deserializer, Serializer, ser};

    /// Convert a byte slice into a &str and serialize.
    pub fn serialize<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let s =
            str::from_utf8(bytes).map_err(|e| ser::Error::custom(format!("invalid UTF-8: {e}")))?;
        serializer.serialize_str(s)
    }

    /// Deserialize into string and convert into bytes.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(s.into_bytes())
    }
}

#[cfg(all(test, feature = "json"))]
mod tests {
    use num_bigint::BigUint;
    use serde::{Deserialize, Serialize};

    use super::{biguint_serde, utf8_serde};

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct BigUintWrapper(#[serde(with = "biguint_serde")] BigUint);

    #[test]
    fn biguint_valid_serde() {
        let orig = BigUint::parse_bytes(b"123456789012345678901234567890", 10).unwrap();
        let wrapped = BigUintWrapper(orig.clone());
        let ser = serde_json::to_string(&wrapped).unwrap();
        assert_eq!(ser, "\"123456789012345678901234567890\"");
        let de: BigUintWrapper = serde_json::from_str(&ser).unwrap();
        assert_eq!(de, wrapped);
    }

    #[test]
    fn biguint_invalid_serde() {
        let bad = "\"not_a_number\"";
        assert!(serde_json::from_str::<BigUintWrapper>(bad).is_err());
    }

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Utf8Wrapper(#[serde(with = "utf8_serde")] Vec<u8>);

    #[test]
    fn utf8_valid_serde() {
        let orig = b"hello-zescrow".to_vec();
        let wrapped = Utf8Wrapper(orig.clone());
        let ser = serde_json::to_string(&wrapped).unwrap();
        assert_eq!(ser, "\"hello-zescrow\"");
        let de: Utf8Wrapper = serde_json::from_str(&ser).unwrap();
        assert_eq!(de, wrapped);
    }

    #[test]
    fn utf8_invalid_serde() {
        let bad = Utf8Wrapper(vec![0xff, 0xfe]);
        assert!(serde_json::to_string(&bad).is_err());
    }
}
