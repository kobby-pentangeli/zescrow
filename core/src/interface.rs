//! JSON schemas, I/O utilities, and chain configuration types.
//!
//! This module provides configuration loading with environment variable expansion.
//! JSON templates can reference environment variables using `${VAR_NAME}` syntax,
//! which are expanded at load time.

#[cfg(feature = "json")]
use std::borrow::Cow;
#[cfg(feature = "json")]
use std::fs::File;
#[cfg(feature = "json")]
use std::path::Path;

#[cfg(feature = "json")]
use anyhow::Context;
use bincode::{Decode, Encode};
#[cfg(feature = "json")]
use serde::de::DeserializeOwned;
#[cfg(feature = "json")]
use serde::{Deserialize, Serialize};

use crate::{Asset, EscrowError, Party};

/// Default path to escrow parameters configuration.
#[cfg(feature = "json")]
pub const ESCROW_PARAMS_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../deploy/escrow_params.json");

/// Default path to on-chain escrow metadata (output from create command).
#[cfg(feature = "json")]
pub const ESCROW_METADATA_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../deploy/escrow_metadata.json"
);

/// Default path to escrow conditions.
#[cfg(feature = "json")]
pub const ESCROW_CONDITIONS_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../deploy/escrow_conditions.json"
);

/// Expands environment variable references in a string.
///
/// Replaces all occurrences of `${VAR_NAME}` with the corresponding
/// environment variable value. If the variable is not set, it is
/// replaced with an empty string.
///
/// # Examples
///
/// ```
/// # use zescrow_core::interface::expand_env_vars;
/// // Strings without `${...}` are returned unchanged.
/// assert_eq!(expand_env_vars("prefix-suffix"), "prefix-suffix");
///
/// // Unset variables expand to empty strings.
/// assert_eq!(expand_env_vars("${ZESCROW_DOC_UNSET_VAR}"), "");
/// ```
#[cfg(feature = "json")]
#[must_use]
pub fn expand_env_vars(input: &str) -> Cow<'_, str> {
    expand_with(input, |name| std::env::var(name).ok())
}

/// Expands `${VAR}` references in `input`, resolving each name through `lookup`.
///
/// Names that `lookup` resolves to `None` are replaced with an empty string.
/// Returns `Cow::Borrowed` when the input contains no `${` and needs no expansion.
#[cfg(feature = "json")]
fn expand_with<F>(input: &str, mut lookup: F) -> Cow<'_, str>
where
    F: FnMut(&str) -> Option<String>,
{
    if !input.contains("${") {
        return Cow::Borrowed(input);
    }

    let mut result = String::with_capacity(input.len());
    let mut remaining = input;

    while let Some(start) = remaining.find("${") {
        result.push_str(&remaining[..start]);

        let after_start = &remaining[start + 2..];
        match after_start.find('}') {
            Some(end) => {
                let var_name = &after_start[..end];
                if let Some(value) = lookup(var_name) {
                    result.push_str(&value);
                }
                remaining = &after_start[end + 1..];
            }
            None => {
                result.push_str(&remaining[start..]);
                remaining = "";
            }
        }
    }

    result.push_str(remaining);
    Cow::Owned(result)
}

/// Expands `${VAR}` references for the config-loading path, failing if any
/// referenced variable is unset.
///
/// Unlike [`expand_env_vars`], a missing variable is a hard error here: a
/// secret-bearing field such as `sender_private_id` silently expanding to an
/// empty string would surface only as a confusing downstream failure. The
/// error names every unset variable so the misconfiguration is actionable.
#[cfg(feature = "json")]
fn expand_env_checked(input: &str) -> anyhow::Result<String> {
    let mut missing = Vec::new();
    let expanded = expand_with(input, |name| match std::env::var(name) {
        Ok(value) => Some(value),
        Err(_) => {
            missing.push(name.to_string());
            None
        }
    })
    .into_owned();

    if missing.is_empty() {
        Ok(expanded)
    } else {
        anyhow::bail!(
            "unset environment variable(s) referenced in configuration: {}",
            missing.join(", ")
        )
    }
}

/// Reads a JSON-encoded file from the given `path` and deserializes into type `T`.
///
/// Environment variable references in the format `${VAR_NAME}` are expanded
/// before parsing. This allows configuration templates to reference secrets
/// stored in environment variables or `.env` files.
///
/// # Errors
///
/// Returns an `anyhow::Error` if the file cannot be opened, read, or parsed.
///
/// # Examples
///
/// ```ignore
/// # use zescrow_core::interface::load_escrow_data;
///
/// #[derive(Deserialize)]
/// struct MyParams { /* fields matching JSON */ }
///
/// // JSON file can contain: { "key": "${MY_SECRET}" }
/// let _params: MyParams = load_escrow_data("./my_params.json").unwrap();
/// ```
#[cfg(feature = "json")]
pub fn load_escrow_data<P, T>(path: P) -> anyhow::Result<T>
where
    P: AsRef<Path>,
    T: DeserializeOwned,
{
    let path = path.as_ref();
    let content =
        std::fs::read_to_string(path).with_context(|| format!("loading escrow data: {path:?}"))?;
    let expanded = expand_env_checked(&content)?;
    serde_json::from_str(&expanded).with_context(|| format!("parsing JSON from {path:?}"))
}

/// Writes `data` (serializable) as pretty-printed JSON to the given `path`.
///
/// # Errors
///
/// Returns an `anyhow::Error` if the file cannot be created or data cannot be serialized.
///
/// # Examples
///
/// ```ignore
/// # use zescrow_core::interface::save_escrow_data;
/// # use serde::Serialize;
///
/// #[derive(Serialize)]
/// struct MyMetadata { /* fields */ }
///
/// let metadata = MyMetadata { /* ... */ };
/// save_escrow_data("./metadata.json", &metadata).unwrap();
/// ```
#[cfg(feature = "json")]
pub fn save_escrow_data<P, T>(path: P, data: &T) -> anyhow::Result<()>
where
    P: AsRef<Path>,
    T: Serialize,
{
    let path = path.as_ref();
    let file = File::create(path).with_context(|| format!("creating file {path:?}"))?;
    serde_json::to_writer_pretty(file, data)
        .with_context(|| format!("serializing to JSON to {path:?}"))
}

/// State of escrow execution in the `client`.
#[cfg_attr(feature = "json", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Copy, Encode, Decode, PartialEq, Eq)]
pub enum ExecutionState {
    /// Escrow object created.
    Initialized,

    /// Funds have been deposited; awaiting release or cancellation.
    Funded,

    /// Conditions (if any) have been fulfilled;
    /// funds will be released to the recipient if the proof verifies on-chain.
    ConditionsMet,
}

/// Metadata returned from on-chain escrow creation.
#[cfg_attr(feature = "json", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Encode, Decode)]
pub struct EscrowMetadata {
    /// The parameters that were specified during escrow creation.
    pub params: EscrowParams,
    /// State of escrow execution in the `client`.
    pub state: ExecutionState,
    /// Unique identifier for the created escrow.
    pub escrow_id: Option<u64>,
}

/// Parameters required to create an escrow on-chain.
#[cfg_attr(feature = "json", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Encode, Decode)]
pub struct EscrowParams {
    /// Chain-specific network configuration.
    pub chain_config: ChainConfig,

    /// Exactly which asset to lock (the native coin or a fungible token).
    pub asset: Asset,

    /// Who is funding the escrow.
    pub sender: Party,

    /// Who will receive the funds once conditions pass.
    pub recipient: Party,

    /// Optional block height or slot after which "release" is allowed.
    /// Must be `None` or less than `cancel_after` if both are set.
    pub finish_after: Option<u64>,

    /// Optional block height or slot after which "cancel" is allowed.
    /// Must be `None` or greater than `finish_after` if both are set.
    pub cancel_after: Option<u64>,

    /// Denotes whether this escrow is subject to cryptographic conditions.
    pub has_conditions: bool,
}

/// Chain-specific network configuration for creating or querying escrows.
#[cfg_attr(feature = "json", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Encode, Decode)]
pub struct ChainConfig {
    /// Network identifier.
    pub chain: Chain,
    /// JSON-RPC endpoint URL.
    pub rpc_url: String,
    /// Sender's private key and/or keypair path.
    ///
    /// For Ethereum, a wallet import format (WIF) or hex is expected.
    /// For Solana, a path to a keypair file (e.g., `~/.config/solana/id.json`).
    pub sender_private_id: String,
    /// On-chain escrow program ID (Solana) or smart contract address (Ethereum).
    pub agent_id: String,
}

/// Supported blockchain networks.
#[cfg_attr(feature = "json", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "lowercase"))]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Encode, Decode)]
pub enum Chain {
    /// Ethereum and other EVM-compatible chains.
    Ethereum,
    /// Solana
    Solana,
}

impl AsRef<str> for Chain {
    fn as_ref(&self) -> &str {
        match self {
            Chain::Ethereum => "ethereum",
            Chain::Solana => "solana",
        }
    }
}

impl std::str::FromStr for Chain {
    type Err = EscrowError;

    /// Parses a string ID into a `Chain` enum (case-insensitive).
    ///
    /// # Errors
    ///
    /// Returns `EscrowError::UnsupportedChain` on unrecognized input.
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "ethereum" | "eth" => Ok(Self::Ethereum),
            "solana" | "sol" => Ok(Self::Solana),
            _ => Err(EscrowError::UnsupportedChain),
        }
    }
}

#[cfg(all(test, feature = "json"))]
mod tests {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn chain_from_str_ethereum() {
        assert!(matches!(Chain::from_str("ethereum"), Ok(Chain::Ethereum)));
        assert!(matches!(Chain::from_str("ETHEREUM"), Ok(Chain::Ethereum)));
        assert!(matches!(Chain::from_str("eth"), Ok(Chain::Ethereum)));
        assert!(matches!(Chain::from_str("ETH"), Ok(Chain::Ethereum)));
    }

    #[test]
    fn chain_from_str_solana() {
        assert!(matches!(Chain::from_str("solana"), Ok(Chain::Solana)));
        assert!(matches!(Chain::from_str("SOLANA"), Ok(Chain::Solana)));
        assert!(matches!(Chain::from_str("sol"), Ok(Chain::Solana)));
        assert!(matches!(Chain::from_str("SOL"), Ok(Chain::Solana)));
    }

    #[test]
    fn chain_from_str_unsupported() {
        assert!(matches!(
            Chain::from_str("bitcoin"),
            Err(EscrowError::UnsupportedChain)
        ));
        assert!(matches!(
            Chain::from_str(""),
            Err(EscrowError::UnsupportedChain)
        ));
    }

    #[test]
    fn chain_as_ref() {
        assert_eq!(Chain::Ethereum.as_ref(), "ethereum");
        assert_eq!(Chain::Solana.as_ref(), "solana");
    }

    #[test]
    fn expand_env_vars_no_vars() {
        let result = expand_env_vars("no variables here");
        assert_eq!(result, "no variables here");
        // Should return Borrowed when no expansion needed
        assert!(matches!(result, std::borrow::Cow::Borrowed(_)));
    }

    #[test]
    fn expand_with_single_var() {
        let result = expand_with("prefix-${VAR}-suffix", |name| {
            (name == "VAR").then(|| "hello".to_string())
        });
        assert_eq!(result, "prefix-hello-suffix");
    }

    #[test]
    fn expand_with_multiple_vars() {
        let result = expand_with("${A} and ${B}", |name| match name {
            "A" => Some("alpha".to_string()),
            "B" => Some("beta".to_string()),
            _ => None,
        });
        assert_eq!(result, "alpha and beta");
    }

    #[test]
    fn expand_with_unset_becomes_empty() {
        let result = expand_with("before-${UNSET}-after", |_| None);
        assert_eq!(result, "before--after");
    }

    #[test]
    fn expand_with_unclosed_brace() {
        let result = expand_with("prefix-${UNCLOSED", |_| Some("value".to_string()));
        assert_eq!(result, "prefix-${UNCLOSED");
    }

    #[test]
    fn expand_env_checked_passthrough_without_vars() {
        let result = expand_env_checked("no variables here").unwrap();
        assert_eq!(result, "no variables here");
    }

    #[test]
    fn expand_env_checked_errors_on_unset() {
        let err = expand_env_checked("key=${ZESCROW_TEST_DEFINITELY_UNSET_VAR_XYZ}").unwrap_err();
        assert!(
            err.to_string()
                .contains("ZESCROW_TEST_DEFINITELY_UNSET_VAR_XYZ")
        );
    }
}
