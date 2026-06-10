//! Binding public commitment for the zero-knowledge proof journal.
//!
//! The guest commits a [`PublicCommitment`] to the receipt journal so a valid
//! receipt is cryptographically tied to exactly one escrow settlement: an
//! on-chain verifier reconstructs the same bytes from its own state, checks the
//! seal against the pinned guest image id over `sha256(journal)`, and releases
//! only on a match. The witness that satisfies the condition never enters the journal;
//! only the binding fields and the pass/fail outcome are public.

use sha2::{Digest, Sha256};

use crate::error::CommitmentError;
use crate::{Asset, AssetKind, BigNumber, Chain, Condition, Escrow, ExecutionState, ID};

type Result<T> = std::result::Result<T, CommitmentError>;

const JOURNAL_VERSION: u8 = 1;

// Width, in bytes, of the big-endian amount field (256-bit).
const AMOUNT_WIDTH: usize = 32;

/// Outcome of escrow execution as committed to the public journal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionResult {
    /// Conditions were satisfied; the escrow reached the given terminal state.
    Success(ExecutionState),
    /// Execution failed; funds must not be released.
    Failure,
}

/// The private input handed to the guest: the full escrow context plus the
/// on-chain identifiers needed to bind the proof to a specific settlement.
#[derive(Debug, Clone, bincode::Encode, bincode::Decode)]
pub struct ProofInput {
    /// The escrow whose conditions the guest verifies.
    pub escrow: Escrow,
    /// Network the escrow settles on.
    pub chain: Chain,
    /// On-chain escrow program id (Solana) or contract address (Ethereum).
    pub agent_id: ID,
    /// Unique on-chain identifier for this escrow instance.
    pub escrow_id: u64,
}

/// The public commitment the guest writes to the receipt journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicCommitment {
    /// Network the escrow settles on.
    pub chain: Chain,
    /// Raw bytes of the on-chain program id / contract address.
    pub agent_id: Vec<u8>,
    /// Unique on-chain identifier for this escrow instance.
    pub escrow_id: u64,
    /// Raw address bytes of the funding party.
    pub sender: Vec<u8>,
    /// Raw address bytes of the recipient.
    pub recipient: Vec<u8>,
    /// Kind of asset under escrow.
    pub asset_kind: AssetKind,
    /// Raw bytes of the token contract / mint (empty for the native coin).
    pub asset_token: Vec<u8>,
    /// Amount under escrow, in the smallest unit.
    pub amount: BigNumber,
    /// Witness-free commitment to the release condition (all-zero when none).
    pub condition: [u8; 32],
    /// Pass/fail outcome of executing the escrow.
    pub outcome: ExecutionResult,
}

impl PublicCommitment {
    /// Builds the commitment from the guest input and the execution outcome.
    ///
    /// # Errors
    ///
    /// Returns [`CommitmentError`] if any participant or asset identity cannot
    /// be decoded into raw bytes.
    pub fn new(input: &ProofInput, outcome: ExecutionResult) -> crate::Result<Self> {
        let Asset {
            kind,
            agent_id,
            amount,
            ..
        } = &input.escrow.asset;

        let asset_token = agent_id
            .as_ref()
            .map(ID::to_bytes)
            .transpose()?
            .unwrap_or_default();

        Ok(Self {
            chain: input.chain,
            agent_id: input.agent_id.to_bytes()?,
            escrow_id: input.escrow_id,
            sender: input.escrow.sender.to_bytes()?,
            recipient: input.escrow.recipient.to_bytes()?,
            asset_kind: *kind,
            asset_token,
            amount: amount.clone(),
            condition: condition_commitment(input.escrow.condition.as_ref()),
            outcome,
        })
    }

    /// Serializes the commitment into the journal byte layout.
    ///
    /// # Errors
    ///
    /// Returns [`CommitmentError::AmountTooLarge`] if the amount exceeds 256
    /// bits, or [`CommitmentError::FieldTooLong`] if a length-prefixed field
    /// exceeds `u16::MAX` bytes.
    pub fn to_journal_bytes(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        out.push(JOURNAL_VERSION);
        out.push(chain_tag(self.chain));

        match self.outcome {
            ExecutionResult::Success(state) => {
                out.push(1);
                out.push(state_tag(state));
            }
            ExecutionResult::Failure => out.push(0),
        }

        push_bytes(&mut out, &self.agent_id)?;
        out.extend_from_slice(&self.escrow_id.to_be_bytes());
        push_bytes(&mut out, &self.sender)?;
        push_bytes(&mut out, &self.recipient)?;
        out.push(asset_kind_tag(self.asset_kind));
        push_bytes(&mut out, &self.asset_token)?;
        out.extend_from_slice(&amount_be_bytes(&self.amount)?);
        out.extend_from_slice(&self.condition);

        Ok(out)
    }

    /// Parses a commitment back from journal bytes.
    ///
    /// # Errors
    ///
    /// Returns [`CommitmentError`] if `bytes` is truncated, carries an unknown
    /// version, or contains an unrecognized enum tag.
    pub fn from_journal_bytes(bytes: &[u8]) -> Result<Self> {
        let mut reader = Reader::new(bytes);

        let version = reader.u8()?;
        if version != JOURNAL_VERSION {
            return Err(CommitmentError::UnknownVersion(version));
        }

        let chain = chain_from_tag(reader.u8()?)?;
        let outcome = match reader.u8()? {
            0 => ExecutionResult::Failure,
            1 => ExecutionResult::Success(state_from_tag(reader.u8()?)?),
            other => return Err(CommitmentError::unknown_tag("outcome", other)),
        };

        let agent_id = reader.length_prefixed()?.to_vec();
        let escrow_id = reader.u64()?;
        let sender = reader.length_prefixed()?.to_vec();
        let recipient = reader.length_prefixed()?.to_vec();
        let asset_kind = asset_kind_from_tag(reader.u8()?)?;
        let asset_token = reader.length_prefixed()?.to_vec();
        let amount = BigNumber(num_bigint::BigUint::from_bytes_be(
            reader.take(AMOUNT_WIDTH)?,
        ));
        let condition = reader.array32()?;
        reader.finish()?;

        Ok(Self {
            chain,
            agent_id,
            escrow_id,
            sender,
            recipient,
            asset_kind,
            asset_token,
            amount,
            condition,
            outcome,
        })
    }

    /// Returns `sha256` of the journal bytes; the digest an on-chain
    /// verifier checks the receipt seal against.
    ///
    /// # Errors
    ///
    /// Propagates [`Self::to_journal_bytes`] errors.
    pub fn digest(&self) -> Result<[u8; 32]> {
        Ok(Sha256::digest(self.to_journal_bytes()?).into())
    }
}

/// Commitment to an optional release condition: the condition's own commitment,
/// or all-zero when the escrow has no cryptographic condition.
fn condition_commitment(condition: Option<&Condition>) -> [u8; 32] {
    condition.map(Condition::commitment).unwrap_or([0u8; 32])
}

/// Encodes `amount` as fixed-width 32-byte big-endian (`uint256`).
fn amount_be_bytes(amount: &BigNumber) -> Result<[u8; AMOUNT_WIDTH]> {
    let be = amount.0.to_bytes_be();
    let offset = AMOUNT_WIDTH
        .checked_sub(be.len())
        .ok_or(CommitmentError::AmountTooLarge)?;
    let mut padded = [0u8; AMOUNT_WIDTH];
    padded[offset..].copy_from_slice(&be);
    Ok(padded)
}

/// Appends a `u16` big-endian length prefix followed by `bytes`.
fn push_bytes(out: &mut Vec<u8>, bytes: &[u8]) -> Result<()> {
    let len = u16::try_from(bytes.len()).map_err(|_| CommitmentError::FieldTooLong(bytes.len()))?;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(bytes);
    Ok(())
}

fn chain_tag(chain: Chain) -> u8 {
    match chain {
        Chain::Ethereum => 0,
        Chain::Solana => 1,
    }
}

fn chain_from_tag(tag: u8) -> Result<Chain> {
    match tag {
        0 => Ok(Chain::Ethereum),
        1 => Ok(Chain::Solana),
        other => Err(CommitmentError::unknown_tag("chain", other)),
    }
}

fn asset_kind_tag(kind: AssetKind) -> u8 {
    match kind {
        AssetKind::Native => 0,
        AssetKind::Token => 1,
    }
}

fn asset_kind_from_tag(tag: u8) -> Result<AssetKind> {
    match tag {
        0 => Ok(AssetKind::Native),
        1 => Ok(AssetKind::Token),
        other => Err(CommitmentError::unknown_tag("asset_kind", other)),
    }
}

fn state_tag(state: ExecutionState) -> u8 {
    match state {
        ExecutionState::Initialized => 0,
        ExecutionState::Funded => 1,
        ExecutionState::ConditionsMet => 2,
    }
}

fn state_from_tag(tag: u8) -> Result<ExecutionState> {
    match tag {
        0 => Ok(ExecutionState::Initialized),
        1 => Ok(ExecutionState::Funded),
        2 => Ok(ExecutionState::ConditionsMet),
        other => Err(CommitmentError::unknown_tag("execution_state", other)),
    }
}

/// Bounds-checked forward cursor over the journal bytes.
struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or(CommitmentError::Truncated(self.offset))?;
        let slice = self
            .bytes
            .get(self.offset..end)
            .ok_or(CommitmentError::Truncated(self.offset))?;
        self.offset = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8> {
        self.take(1).map(|s| s[0])
    }

    fn u64(&mut self) -> Result<u64> {
        let bytes: [u8; 8] = self.take(8)?.try_into().expect("slice of length 8");
        Ok(u64::from_be_bytes(bytes))
    }

    fn array32(&mut self) -> Result<[u8; 32]> {
        Ok(self.take(32)?.try_into().expect("slice of length 32"))
    }

    fn length_prefixed(&mut self) -> Result<&'a [u8]> {
        let len = u16::from_be_bytes(self.take(2)?.try_into().expect("slice of length 2"));
        self.take(usize::from(len))
    }

    fn finish(self) -> Result<()> {
        (self.offset == self.bytes.len())
            .then_some(())
            .ok_or(CommitmentError::TrailingBytes(self.offset))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Party, Result};

    fn sample(condition: Option<Condition>, outcome: ExecutionResult) -> Result<PublicCommitment> {
        let escrow = Escrow::new(
            Party::for_chain(
                Chain::Ethereum,
                "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
            )?,
            Party::for_chain(
                Chain::Ethereum,
                "0xEA674fdDe714fd979de3EdF0F56AA9716B898ec8",
            )?,
            Asset::native(BigNumber::from(1_000_000_000u64)),
            condition,
        );
        let input = ProofInput {
            escrow,
            chain: Chain::Ethereum,
            agent_id: ID::for_chain(
                Chain::Ethereum,
                "0x00000000000000000000000000000000DeaDBeef",
            )?,
            escrow_id: 42,
        };
        PublicCommitment::new(&input, outcome)
    }

    #[test]
    fn journal_roundtrip_success() {
        let commitment = sample(
            None,
            ExecutionResult::Success(ExecutionState::ConditionsMet),
        )
        .unwrap();
        let bytes = commitment.to_journal_bytes().unwrap();
        let decoded = PublicCommitment::from_journal_bytes(&bytes).unwrap();
        assert_eq!(decoded, commitment);
        assert_eq!(decoded.escrow_id, 42);
        assert_eq!(decoded.amount, BigNumber::from(1_000_000_000u64));
    }

    #[test]
    fn journal_roundtrip_failure_and_condition() {
        let preimage = b"zkEscrow".to_vec();
        let hash = sha2::Sha256::digest(&preimage).into();
        let condition = Condition::hashlock(hash, preimage);
        let expected = condition.commitment();

        let commitment = sample(Some(condition), ExecutionResult::Failure).unwrap();
        assert_eq!(commitment.condition, expected);

        let bytes = commitment.to_journal_bytes().unwrap();
        let decoded = PublicCommitment::from_journal_bytes(&bytes).unwrap();
        assert_eq!(decoded, commitment);
        assert_eq!(decoded.outcome, ExecutionResult::Failure);
        assert!(decoded.condition != [0u8; 32]);
    }

    #[test]
    fn no_condition_commits_zero() {
        let commitment = sample(
            None,
            ExecutionResult::Success(ExecutionState::ConditionsMet),
        )
        .unwrap();
        assert_eq!(commitment.condition, [0u8; 32]);
    }

    #[test]
    fn digest_matches_sha256_of_journal() {
        let commitment = sample(
            None,
            ExecutionResult::Success(ExecutionState::ConditionsMet),
        )
        .unwrap();
        let bytes = commitment.to_journal_bytes().unwrap();
        let expected: [u8; 32] = sha2::Sha256::digest(&bytes).into();
        assert_eq!(commitment.digest().unwrap(), expected);
    }

    #[test]
    fn truncated_journal_is_rejected() {
        let commitment = sample(
            None,
            ExecutionResult::Success(ExecutionState::ConditionsMet),
        )
        .unwrap();
        let bytes = commitment.to_journal_bytes().unwrap();
        assert!(PublicCommitment::from_journal_bytes(&bytes[..bytes.len() - 1]).is_err());
    }

    #[test]
    fn trailing_bytes_are_rejected() {
        let commitment = sample(
            None,
            ExecutionResult::Success(ExecutionState::ConditionsMet),
        )
        .unwrap();
        let mut bytes = commitment.to_journal_bytes().unwrap();
        bytes.push(0);
        assert!(PublicCommitment::from_journal_bytes(&bytes).is_err());
    }

    #[test]
    fn unknown_version_is_rejected() {
        let commitment = sample(
            None,
            ExecutionResult::Success(ExecutionState::ConditionsMet),
        )
        .unwrap();
        let mut bytes = commitment.to_journal_bytes().unwrap();
        bytes[0] = 0xFF;
        assert!(matches!(
            PublicCommitment::from_journal_bytes(&bytes),
            Err(CommitmentError::UnknownVersion(0xFF))
        ));
    }
}
