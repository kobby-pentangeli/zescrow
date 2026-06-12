use bincode::{Decode, Encode};
#[cfg(feature = "json")]
use serde::{Deserialize, Serialize};

use super::Condition;

/// N-of-M threshold condition.
///
/// Satisfied when at least `threshold` of the `subconditions` verify
/// successfully. Subconditions can be any [`Condition`] variant,
/// including nested thresholds.
#[cfg_attr(feature = "json", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Encode, Decode, PartialEq, Eq)]
pub struct Threshold {
    /// Minimum number of valid subconditions required.
    pub threshold: usize,

    /// Subconditions to evaluate.
    pub subconditions: Vec<Condition>,
}

impl Threshold {
    /// Verifies that at least `self.threshold` subconditions are satisfied.
    ///
    /// A threshold of zero is rejected as malformed. A threshold exceeding the
    /// number of subconditions is likewise rejected, since it can never be met.
    ///
    /// # Errors
    ///
    /// - [`Error::ZeroThreshold`] if threshold == 0.
    /// - [`Error::ThresholdUnsatisfiable`] if threshold > subconditions.len().
    /// - [`Error::ThresholdNotMet`] if fewer than threshold subconditions verify.
    pub fn verify(&self) -> Result<(), Error> {
        match self.threshold {
            0 => Err(Error::ZeroThreshold),
            required if required > self.subconditions.len() => Err(Error::ThresholdUnsatisfiable {
                required,
                available: self.subconditions.len(),
            }),
            required => {
                let satisfied = self.count_satisfied();
                (satisfied >= required)
                    .then_some(())
                    .ok_or(Error::ThresholdNotMet {
                        required,
                        satisfied,
                    })
            }
        }
    }

    /// Counts the number of subconditions that verify successfully. A
    /// subcondition that fails to verify simply does not count toward the
    /// threshold.
    fn count_satisfied(&self) -> usize {
        self.subconditions
            .iter()
            .filter_map(|c| c.verify().ok())
            .count()
    }
}

/// Threshold conditions verification errors.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The threshold was zero.
    #[error("threshold must be greater than zero")]
    ZeroThreshold,

    /// The threshold exceeds the number of subconditions and can never be met.
    #[error("threshold {required} exceeds the {available} subconditions available")]
    ThresholdUnsatisfiable {
        /// Minimum number of valid subconditions required.
        required: usize,
        /// Number of subconditions present.
        available: usize,
    },

    /// Fewer than the required number of subconditions were satisfied.
    #[error("needed at least {required} passes, but only {satisfied} succeeded")]
    ThresholdNotMet {
        /// Minimum number of valid subconditions required.
        required: usize,
        /// Number of verified subconditions.
        satisfied: usize,
    },
}
