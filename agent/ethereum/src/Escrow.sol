// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import {IRiscZeroVerifier} from "./IRiscZeroVerifier.sol";

/// @title Zescrow Escrow Manager
/// @notice Holds native ETH until a release condition is proven and a time-lock
///         expires, or until the sender cancels for a refund. A conditioned
///         escrow releases only against a RISC Zero receipt whose public journal
///         binds the proof to this exact escrow; an unconditioned escrow
///         (`conditionId == 0`) releases on the recipient's call after its
///         finish block, with no proof.
/// @dev Funds are settled in native ETH only. The core asset model can also
///      express ERC-20, but no on-chain settlement path for them exists yet.
contract Escrow is ReentrancyGuard {
    /// @dev A single escrow's state.
    struct EscrowDB {
        address sender; // depositor
        address recipient; // beneficiary
        uint256 amount; // locked ETH (smallest unit)
        uint256 finishAfter; // release block (0 = immediate)
        uint256 cancelAfter; // refund block (0 = refund disabled)
        bytes32 conditionId; // release condition commitment (0 = unconditioned)
        bool settled; // prevents reuse
    }

    /// @notice The RISC Zero verifier (router) receipts are checked against.
    IRiscZeroVerifier public immutable verifier;

    /// @notice The pinned guest image id; only receipts from this exact guest
    ///         can authorize a conditioned release.
    bytes32 public immutable imageId;

    /// @dev Auto-incrementing escrow id; the first created escrow is id 1.
    uint256 private _nextEscrowId;

    /// @dev Escrow id to its state.
    mapping(uint256 => EscrowDB) private _escrows;

    event EscrowCreated(
        uint256 indexed escrowId,
        address indexed sender,
        address indexed recipient,
        uint256 amount,
        uint256 finishAfter,
        uint256 cancelAfter,
        bytes32 conditionId
    );
    event EscrowFinished(uint256 indexed escrowId, address indexed recipient, uint256 amount);
    event EscrowCancelled(uint256 indexed escrowId, address indexed sender, uint256 amount);

    error InvalidVerifier(); // verifier address is zero
    error InvalidImageId(); // pinned image id is zero
    error InvalidRecipient(); // recipient must be non-zero
    error InsufficientValue(); // msg.value must be non-zero
    error TimeLockUnset(); // neither finishAfter nor cancelAfter set
    error ConditionRequiresCancel(); // conditioned escrow without a refund deadline
    error InvalidTimeOrder(); // finishAfter >= cancelAfter when both set
    error EscrowNotExists(); // no escrow for the given id
    error OnlyRecipient(); // finishEscrow caller is not the recipient
    error OnlySender(); // cancelEscrow caller is not the sender
    error AlreadySettled(); // escrow already released or refunded
    error TooEarlyToFinish(); // block.number < finishAfter
    error TooEarlyToCancel(); // block.number < cancelAfter
    error CancelDisabled(); // cancelAfter == 0
    error TransferFailed(); // ETH transfer call returned false
    error MalformedJournal(); // journal is truncated, mis-versioned, or mis-tagged
    error JournalBindingMismatch(); // journal does not bind to this escrow

    /// @param verifier_ The RISC Zero verifier (router) for this network.
    /// @param imageId_ The pinned guest image id authorized to release funds.
    constructor(IRiscZeroVerifier verifier_, bytes32 imageId_) {
        if (address(verifier_) == address(0)) revert InvalidVerifier();
        if (imageId_ == bytes32(0)) revert InvalidImageId();
        verifier = verifier_;
        imageId = imageId_;
    }

    /// @notice Create a new escrow funded by `msg.value`.
    /// @dev At least one of `finishAfter`/`cancelAfter` must be set; when both
    ///      are set, `finishAfter < cancelAfter` so the release window opens
    ///      strictly before the refund window. A conditioned escrow
    ///      (`conditionId != 0`) must set `cancelAfter`: release is proof-gated,
    ///      so without a refund deadline the funds would lock forever if the
    ///      proof never arrives.
    /// @param recipient Address that receives the funds on release.
    /// @param finishAfter Block after which release is allowed (0 = immediate).
    /// @param cancelAfter Block after which refund is allowed (0 = disabled).
    /// @param conditionId Commitment to the release condition; pass `bytes32(0)`
    ///        for an unconditioned, time-lock-only escrow.
    /// @return escrowId The new escrow's unique identifier.
    function createEscrow(
        address recipient,
        uint256 finishAfter,
        uint256 cancelAfter,
        bytes32 conditionId
    ) external payable returns (uint256 escrowId) {
        if (recipient == address(0)) revert InvalidRecipient();
        if (msg.value == 0) revert InsufficientValue();
        if (finishAfter == 0 && cancelAfter == 0) revert TimeLockUnset();
        if (conditionId != bytes32(0) && cancelAfter == 0) {
            revert ConditionRequiresCancel();
        }
        if (finishAfter != 0 && cancelAfter != 0 && finishAfter >= cancelAfter) {
            revert InvalidTimeOrder();
        }

        escrowId = ++_nextEscrowId;
        _escrows[escrowId] = EscrowDB({
            sender: msg.sender,
            recipient: recipient,
            amount: msg.value,
            finishAfter: finishAfter,
            cancelAfter: cancelAfter,
            conditionId: conditionId,
            settled: false
        });

        emit EscrowCreated(
            escrowId, msg.sender, recipient, msg.value, finishAfter, cancelAfter, conditionId
        );
    }

    /// @notice Release an escrow to its recipient.
    /// @dev For a conditioned escrow, `seal`/`journal` must be a RISC Zero
    ///      receipt whose journal binds to this escrow and attests the
    ///      conditions were met; the proof is verified against the pinned image
    ///      id. For an unconditioned escrow they are ignored.
    /// @param escrowId The escrow to release.
    /// @param seal The receipt seal (empty for an unconditioned escrow).
    /// @param journal The committed journal bytes (empty for an unconditioned escrow).
    function finishEscrow(uint256 escrowId, bytes calldata seal, bytes calldata journal)
        external
        nonReentrant
    {
        EscrowDB storage escrow = _escrows[escrowId];
        if (escrow.sender == address(0)) revert EscrowNotExists();
        if (msg.sender != escrow.recipient) revert OnlyRecipient();
        if (escrow.settled) revert AlreadySettled();
        if (escrow.finishAfter != 0 && block.number < escrow.finishAfter) {
            revert TooEarlyToFinish();
        }

        if (escrow.conditionId != bytes32(0)) {
            _verifyReceipt(escrow, escrowId, seal, journal);
        }

        escrow.settled = true;
        uint256 payout = escrow.amount;
        escrow.amount = 0;
        emit EscrowFinished(escrowId, escrow.recipient, payout);

        (bool ok,) = payable(escrow.recipient).call{value: payout}("");
        if (!ok) revert TransferFailed();
    }

    /// @notice Cancel an escrow and refund the sender.
    /// @param escrowId The escrow to cancel.
    function cancelEscrow(uint256 escrowId) external nonReentrant {
        EscrowDB storage escrow = _escrows[escrowId];
        if (escrow.sender == address(0)) revert EscrowNotExists();
        if (msg.sender != escrow.sender) revert OnlySender();
        if (escrow.settled) revert AlreadySettled();
        if (escrow.cancelAfter == 0) revert CancelDisabled();
        if (block.number < escrow.cancelAfter) revert TooEarlyToCancel();

        escrow.settled = true;
        uint256 refund = escrow.amount;
        escrow.amount = 0;
        emit EscrowCancelled(escrowId, escrow.sender, refund);

        (bool ok,) = payable(escrow.sender).call{value: refund}("");
        if (!ok) revert TransferFailed();
    }

    /// @notice Retrieve an escrow's state.
    /// @param escrowId The escrow id.
    /// @return The `EscrowDB` for that id.
    function getEscrow(uint256 escrowId) external view returns (EscrowDB memory) {
        EscrowDB storage escrow = _escrows[escrowId];
        if (escrow.sender == address(0)) revert EscrowNotExists();
        return escrow;
    }

    /// @notice The id the next created escrow will receive.
    function nextEscrowId() external view returns (uint256) {
        return _nextEscrowId + 1;
    }

    /// @notice The total number of escrows created.
    function escrowCount() external view returns (uint256) {
        return _nextEscrowId;
    }

    /// @dev Check the journal binds to this escrow, then verify the seal against
    ///      the pinned image id over `sha256(journal)`. Reverts on any mismatch
    ///      or on an invalid proof.
    function _verifyReceipt(
        EscrowDB storage escrow,
        uint256 escrowId,
        bytes calldata seal,
        bytes calldata journal
    ) private view {
        _checkJournalBinding(escrow, escrowId, journal);
        verifier.verify(seal, imageId, sha256(journal));
    }

    /// @dev Parse the canonical journal and require every binding field to match
    ///      this escrow. Layout (big-endian, length-prefixed), version 1:
    ///        version(1)=1 | chain(1)=0 ETH | outcome(1)=1 success | state(1)=2
    ///        met | agentLen(2)=20 | agent(20) | escrowId(8) | senderLen(2)=20 |
    ///        sender(20) | recipientLen(2)=20 | recipient(20) | assetKind(1)=0
    ///        native | tokenLen(2)=0 | amount(32) | condition(32)
    function _checkJournalBinding(EscrowDB storage escrow, uint256 escrowId, bytes calldata journal)
        private
        view
    {
        uint256 off;
        uint256 value;

        (value, off) = _readBE(journal, off, 1);
        if (value != 1) revert MalformedJournal(); // journal version
        (value, off) = _readBE(journal, off, 1);
        if (value != 0) revert MalformedJournal(); // chain tag: Ethereum
        (value, off) = _readBE(journal, off, 1);
        if (value != 1) revert MalformedJournal(); // outcome: success
        (value, off) = _readBE(journal, off, 1);
        if (value != 2) revert MalformedJournal(); // state: conditions met

        address agent;
        (agent, off) = _readAddress(journal, off);
        if (agent != address(this)) revert JournalBindingMismatch();

        (value, off) = _readBE(journal, off, 8);
        if (value != escrowId) revert JournalBindingMismatch();

        address party;
        (party, off) = _readAddress(journal, off);
        if (party != escrow.sender) revert JournalBindingMismatch();
        (party, off) = _readAddress(journal, off);
        if (party != escrow.recipient) revert JournalBindingMismatch();

        (value, off) = _readBE(journal, off, 1);
        if (value != 0) revert MalformedJournal(); // asset kind: native
        (value, off) = _readBE(journal, off, 2);
        if (value != 0) revert MalformedJournal(); // native token field is empty

        (value, off) = _readBE(journal, off, 32);
        if (value != escrow.amount) revert JournalBindingMismatch();

        (value, off) = _readBE(journal, off, 32);
        if (bytes32(value) != escrow.conditionId) {
            revert JournalBindingMismatch();
        }

        if (off != journal.length) revert MalformedJournal(); // no trailing bytes
    }

    /// @dev Read a length-prefixed (`uint16`) field declared as a 20-byte
    ///      address and return it with the advanced offset. The accumulator is
    ///      `uint160` and fed only widening byte values, so exactly 20 bytes
    ///      fill the address width with no truncating cast.
    function _readAddress(bytes calldata journal, uint256 offset)
        private
        pure
        returns (address account, uint256 next)
    {
        uint256 len;
        (len, offset) = _readBE(journal, offset, 2);
        if (len != 20) revert MalformedJournal();

        next = offset + 20;
        if (next > journal.length) revert MalformedJournal();
        uint160 acc;
        for (uint256 i = offset; i < next; ++i) {
            acc = (acc << 8) | uint160(uint8(journal[i]));
        }
        account = address(acc);
    }

    /// @dev Read `n` (<= 32) big-endian bytes as a `uint256`, bounds-checked.
    function _readBE(bytes calldata journal, uint256 offset, uint256 n)
        private
        pure
        returns (uint256 value, uint256 next)
    {
        next = offset + n;
        if (next > journal.length) revert MalformedJournal();
        for (uint256 i = offset; i < next; ++i) {
            value = (value << 8) | uint256(uint8(journal[i]));
        }
    }
}
