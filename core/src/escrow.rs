//! Off-chain escrow context for zero-knowledge condition verification.
//!
//! This module implements the [`Escrow`] struct which represents the complete escrow context that is
//! passed to the zkVM guest for verification. It contains the asset,
//! parties, conditions, and execution state.

use bincode::{Decode, Encode};
#[cfg(feature = "json")]
use {
    crate::interface::{ESCROW_CONDITIONS_PATH, EscrowMetadata},
    serde::{Deserialize, Serialize},
    serde_json,
};

use crate::{Asset, Condition, EscrowError, ExecutionState, Party, Result};

/// Full escrow context, representing the locked asset, participants, and settlement rules.
#[cfg_attr(feature = "json", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Encode, Decode)]
pub struct Escrow {
    /// Asset locked in escrow.
    pub asset: Asset,
    /// Intended recipient of the asset.
    pub recipient: Party,
    /// The party who funded (deposited) the asset.
    pub sender: Party,
    /// Optional cryptographic condition that must be fulfilled before release.
    pub condition: Option<Condition>,
    /// State of escrow execution in the prover (zkVM).
    pub state: ExecutionState,
}

impl Escrow {
    /// Creates a new `Escrow`.
    pub fn new(
        sender: Party,
        recipient: Party,
        asset: Asset,
        condition: Option<Condition>,
    ) -> Self {
        Self {
            asset,
            recipient,
            sender,
            condition,
            state: ExecutionState::Initialized,
        }
    }

    /// Attempts to execute (release) the escrow, performing all necessary checks.
    ///
    /// - Ensures current `state` is `Funded`.
    /// - Verifies `sender` and `recipient` identities.
    /// - Validates the `asset` parameters.
    /// - If a cryptographic `condition` is present, verifies it.
    ///
    /// On success, transitions to `ExecutionState::ConditionsMet`.
    ///
    /// # Errors
    ///
    /// Returns `EscrowError::InvalidState` if not in `Funded` state, or
    /// propagates identity, asset, or condition errors.
    pub fn execute(&mut self) -> Result<ExecutionState> {
        self.validate_state()
            .and_then(|_| self.validate_parties())
            .and_then(|_| self.asset.validate())
            .and_then(|_| self.verify_conditions())
            .map(|_| {
                self.state = ExecutionState::ConditionsMet;
                self.state
            })
    }

    /// Verifies that the escrow is in the `Funded` state.
    fn validate_state(&self) -> Result<()> {
        (self.state == ExecutionState::Funded)
            .then_some(())
            .ok_or(EscrowError::InvalidState)
    }

    /// Verifies both sender and recipient identities.
    fn validate_parties(&self) -> Result<()> {
        self.sender
            .verify_identity()
            .and_then(|_| self.recipient.verify_identity())
    }

    /// Verifies cryptographic conditions if present.
    fn verify_conditions(&self) -> Result<()> {
        self.condition.as_ref().map_or(Ok(()), |cond| cond.verify())
    }

    /// Constructs an `Escrow` from on-chain metadata and, if required,
    /// loads the cryptographic condition from a JSON file.
    ///
    /// If `metadata.has_conditions` is `true`, reads the file at
    /// `ESCROW_CONDITIONS_PATH` and parses it as a `Condition`.
    ///
    /// # Errors
    ///
    /// - I/O error when reading the condition file.
    /// - JSON parsing error when decoding the condition.
    #[cfg(feature = "json")]
    pub fn from_metadata(metadata: EscrowMetadata) -> Result<Self> {
        let condition = if metadata.params.has_conditions {
            let content = std::fs::read_to_string(ESCROW_CONDITIONS_PATH)?;
            let cond: Condition = serde_json::from_str(&content)?;
            Some(cond)
        } else {
            None
        };

        Ok(Self {
            asset: metadata.params.asset,
            recipient: metadata.params.recipient,
            sender: metadata.params.sender,
            condition,
            state: metadata.state,
        })
    }
}

#[cfg(feature = "json")]
impl std::fmt::Display for Escrow {
    /// Compact JSON representation of the `Escrow` for logging.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let json = serde_json::to_string(self).map_err(|_| std::fmt::Error)?;
        write!(f, "{json}")
    }
}

#[cfg(test)]
mod tests {
    use sha2::{Digest as _, Sha256};

    use super::*;
    use crate::{BigNumber, ID};

    fn valid_sender() -> Party {
        Party::new("0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045").unwrap()
    }

    fn valid_recipient() -> Party {
        Party::new("0xEA674fdDe714fd979de3EdF0F56AA9716B898ec8").unwrap()
    }

    fn valid_asset() -> Asset {
        Asset::token(
            ID::from("0xdeadbeef".as_bytes()),
            BigNumber::from(1_000u64),
            18,
        )
    }

    fn valid_condition() -> Condition {
        let preimage = b"secret".to_vec();
        let hash = Sha256::digest(&preimage);
        Condition::hashlock(hash.into(), preimage)
    }

    #[test]
    fn execute_escrow() {
        let mut escrow = Escrow::new(
            valid_sender(),
            valid_recipient(),
            valid_asset(),
            Some(valid_condition()),
        );
        escrow.state = ExecutionState::Funded;
        assert_eq!(escrow.execute().unwrap(), ExecutionState::ConditionsMet);
        assert_eq!(escrow.state, ExecutionState::ConditionsMet);
        // Re-execution is not allowed
        assert!(escrow.execute().is_err());
    }

    #[test]
    fn execute_without_conditions() {
        let mut escrow = Escrow::new(valid_sender(), valid_recipient(), valid_asset(), None);
        escrow.state = ExecutionState::Funded;
        assert_eq!(escrow.execute().unwrap(), ExecutionState::ConditionsMet);
    }

    #[test]
    fn execute_fails_when_not_funded() {
        let mut escrow = Escrow::new(
            valid_sender(),
            valid_recipient(),
            valid_asset(),
            Some(valid_condition()),
        );
        // State is Initialized (not Funded)
        assert_eq!(escrow.state, ExecutionState::Initialized);
        let err = escrow.execute().unwrap_err();
        assert!(matches!(err, EscrowError::InvalidState));
    }

    #[test]
    fn execute_fails_with_invalid_asset() {
        let invalid_asset = Asset::token(
            ID::from("0xdeadbeef".as_bytes()),
            BigNumber::from(0u64), // zero amount
            18,
        );
        let mut escrow = Escrow::new(valid_sender(), valid_recipient(), invalid_asset, None);
        escrow.state = ExecutionState::Funded;
        assert!(escrow.execute().is_err());
    }

    #[test]
    fn execute_fails_with_wrong_preimage() {
        let preimage = b"secret".to_vec();
        let hash = Sha256::digest(&preimage);
        let wrong_condition = Condition::hashlock(hash.into(), b"wrong".to_vec());

        let mut escrow = Escrow::new(
            valid_sender(),
            valid_recipient(),
            valid_asset(),
            Some(wrong_condition),
        );
        escrow.state = ExecutionState::Funded;
        assert!(escrow.execute().is_err());
    }

    #[test]
    fn new_initializes_state() {
        let escrow = Escrow::new(valid_sender(), valid_recipient(), valid_asset(), None);
        assert_eq!(escrow.state, ExecutionState::Initialized);
    }
}
