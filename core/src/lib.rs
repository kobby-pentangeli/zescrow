#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![deny(rustdoc::invalid_html_tags, rustdoc::broken_intra_doc_links)]
#![doc = include_str!("../README.md")]

pub mod asset;
pub mod bignum;
pub mod commitment;
pub mod condition;
pub mod error;
pub mod escrow;
pub mod identity;
pub mod interface;
#[cfg(feature = "json")]
pub mod serde;

pub use asset::{Asset, AssetKind};
pub use bignum::BigNumber;
pub use commitment::{ExecutionResult, ProofInput, PublicCommitment};
pub use condition::Condition;
pub use error::EscrowError;
pub use escrow::Escrow;
pub use identity::{ID, Party};
pub use interface::{Chain, ChainConfig, EscrowMetadata, EscrowParams, ExecutionState};

/// `Result` type for all core operations.
pub type Result<T> = std::result::Result<T, EscrowError>;
