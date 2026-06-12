// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import {Test} from "forge-std/Test.sol";
import {Escrow} from "../src/Escrow.sol";
import {IRiscZeroVerifier} from "../src/IRiscZeroVerifier.sol";
import {MockRiscZeroVerifier} from "./MockRiscZeroVerifier.sol";

contract EscrowTest is Test {
    Escrow internal escrow;
    MockRiscZeroVerifier internal verifier;

    address internal sender;
    address internal recipient;
    address internal attacker;

    bytes32 internal constant IMAGE_ID = keccak256("zescrow-guest-image");
    bytes32 internal constant CONDITION_ID = keccak256("hashlock-commitment");
    uint256 internal constant AMOUNT = 1 ether;

    function setUp() public {
        sender = makeAddr("sender");
        recipient = makeAddr("recipient");
        attacker = makeAddr("attacker");

        verifier = new MockRiscZeroVerifier(true);
        escrow = new Escrow(IRiscZeroVerifier(address(verifier)), IMAGE_ID);

        vm.deal(sender, 100 ether);
    }

    /// Builds a success journal binding to the given fields.
    function _journal(
        address agent,
        uint256 escrowId,
        address from,
        address to,
        uint256 amount,
        bytes32 conditionId
    ) internal pure returns (bytes memory) {
        // Test escrow ids are small and sequential, so narrowing to the
        // journal's 8-byte id field cannot truncate.
        // forge-lint: disable-next-line(unsafe-typecast)
        uint64 id8 = uint64(escrowId);
        bytes memory head = abi.encodePacked(
            uint8(1), // version
            uint8(0), // chain: Ethereum
            uint8(1), // outcome: success
            uint8(2), // state: conditions met
            uint16(20),
            bytes20(agent),
            id8
        );
        bytes memory parties = abi.encodePacked(uint16(20), bytes20(from), uint16(20), bytes20(to));
        bytes memory asset = abi.encodePacked(uint8(0), uint16(0), amount, conditionId);
        return bytes.concat(head, parties, asset);
    }

    function _create(uint256 finishAfter, uint256 cancelAfter, bytes32 conditionId)
        internal
        returns (uint256 id)
    {
        vm.prank(sender);
        id = escrow.createEscrow{value: AMOUNT}(recipient, finishAfter, cancelAfter, conditionId);
    }

    function test_Constructor_RevertsOnZeroVerifier() public {
        vm.expectRevert(Escrow.InvalidVerifier.selector);
        new Escrow(IRiscZeroVerifier(address(0)), IMAGE_ID);
    }

    function test_Constructor_RevertsOnZeroImageId() public {
        vm.expectRevert(Escrow.InvalidImageId.selector);
        new Escrow(IRiscZeroVerifier(address(verifier)), bytes32(0));
    }

    function test_Constructor_PinsVerifierAndImageId() public view {
        assertEq(address(escrow.verifier()), address(verifier));
        assertEq(escrow.imageId(), IMAGE_ID);
    }

    function test_CreateEscrow_StoresFields() public {
        uint256 id = _create(block.number + 1, block.number + 2, CONDITION_ID);
        assertEq(id, 1);
        assertEq(escrow.escrowCount(), 1);
        assertEq(escrow.nextEscrowId(), 2);

        Escrow.EscrowDB memory db = escrow.getEscrow(id);
        assertEq(db.sender, sender);
        assertEq(db.recipient, recipient);
        assertEq(db.amount, AMOUNT);
        assertEq(db.conditionId, CONDITION_ID);
        assertFalse(db.settled);
        assertEq(address(escrow).balance, AMOUNT);
    }

    function test_CreateEscrow_RevertsOnZeroRecipient() public {
        vm.prank(sender);
        vm.expectRevert(Escrow.InvalidRecipient.selector);
        escrow.createEscrow{value: AMOUNT}(address(0), block.number + 1, 0, CONDITION_ID);
    }

    function test_CreateEscrow_RevertsOnZeroValue() public {
        vm.prank(sender);
        vm.expectRevert(Escrow.InsufficientValue.selector);
        escrow.createEscrow{value: 0}(recipient, block.number + 1, 0, CONDITION_ID);
    }

    function test_CreateEscrow_RevertsOnNoTimelock() public {
        vm.prank(sender);
        vm.expectRevert(Escrow.TimeLockUnset.selector);
        escrow.createEscrow{value: AMOUNT}(recipient, 0, 0, CONDITION_ID);
    }

    function test_CreateEscrow_RevertsOnConditionedWithoutCancel() public {
        // A conditioned escrow is proof-gated, so it must set a refund deadline;
        // a finish-only window leaves no exit if the proof never arrives.
        vm.prank(sender);
        vm.expectRevert(Escrow.ConditionRequiresCancel.selector);
        escrow.createEscrow{value: AMOUNT}(recipient, block.number + 1, 0, CONDITION_ID);
    }

    function test_CreateEscrow_RevertsOnBadTimeOrder() public {
        vm.prank(sender);
        vm.expectRevert(Escrow.InvalidTimeOrder.selector);
        escrow.createEscrow{value: AMOUNT}(
            recipient, block.number + 5, block.number + 2, CONDITION_ID
        );
    }

    function test_FinishUnconditioned_AfterTimelock() public {
        uint256 id = _create(block.number + 3, 0, bytes32(0));
        vm.roll(block.number + 3);

        uint256 before = recipient.balance;
        vm.prank(recipient);
        escrow.finishEscrow(id, "", "");

        assertEq(recipient.balance, before + AMOUNT);
        assertTrue(escrow.getEscrow(id).settled);
        assertEq(address(escrow).balance, 0);
    }

    function test_FinishUnconditioned_RevertsTooEarly() public {
        uint256 id = _create(block.number + 3, 0, bytes32(0));
        vm.prank(recipient);
        vm.expectRevert(Escrow.TooEarlyToFinish.selector);
        escrow.finishEscrow(id, "", "");
    }

    function test_FinishUnconditioned_RevertsNotRecipient() public {
        uint256 id = _create(block.number + 1, 0, bytes32(0));
        vm.roll(block.number + 1);
        vm.prank(attacker);
        vm.expectRevert(Escrow.OnlyRecipient.selector);
        escrow.finishEscrow(id, "", "");
    }

    function test_FinishUnconditioned_RevertsOnReuse() public {
        uint256 id = _create(block.number + 1, 0, bytes32(0));
        vm.roll(block.number + 1);
        vm.prank(recipient);
        escrow.finishEscrow(id, "", "");

        vm.prank(recipient);
        vm.expectRevert(Escrow.AlreadySettled.selector);
        escrow.finishEscrow(id, "", "");
    }

    function test_Finish_RevertsOnUnknownEscrow() public {
        vm.prank(recipient);
        vm.expectRevert(Escrow.EscrowNotExists.selector);
        escrow.finishEscrow(999, "", "");
    }

    function test_FinishConditioned_WithValidProof() public {
        uint256 id = _create(0, block.number + 100, CONDITION_ID);
        bytes memory journal =
            _journal(address(escrow), id, sender, recipient, AMOUNT, CONDITION_ID);

        uint256 before = recipient.balance;
        vm.prank(recipient);
        escrow.finishEscrow(id, hex"c0ffee", journal);

        assertEq(recipient.balance, before + AMOUNT);
        assertTrue(escrow.getEscrow(id).settled);
    }

    function test_FinishConditioned_ForwardsJournalDigestToVerifier() public {
        // The mock rejects everything except the exact digest, so a passing
        // finish proves the contract forwarded sha256(journal) unmodified.
        uint256 id = _create(0, block.number + 100, CONDITION_ID);
        bytes memory journal =
            _journal(address(escrow), id, sender, recipient, AMOUNT, CONDITION_ID);

        verifier.setAccept(false);
        verifier.setExpectedDigest(sha256(journal));

        vm.prank(recipient);
        escrow.finishEscrow(id, hex"c0ffee", journal);
        assertTrue(escrow.getEscrow(id).settled);
    }

    function test_FinishConditioned_RevertsOnInvalidProof() public {
        uint256 id = _create(0, block.number + 100, CONDITION_ID);
        bytes memory journal =
            _journal(address(escrow), id, sender, recipient, AMOUNT, CONDITION_ID);

        verifier.setAccept(false);
        vm.prank(recipient);
        vm.expectRevert(MockRiscZeroVerifier.MockVerificationFailed.selector);
        escrow.finishEscrow(id, hex"c0ffee", journal);
    }

    function test_FinishConditioned_RevertsOnAmountMismatch() public {
        uint256 id = _create(0, block.number + 100, CONDITION_ID);
        bytes memory journal =
            _journal(address(escrow), id, sender, recipient, AMOUNT + 1, CONDITION_ID);

        vm.prank(recipient);
        vm.expectRevert(Escrow.JournalBindingMismatch.selector);
        escrow.finishEscrow(id, hex"c0ffee", journal);
    }

    function test_FinishConditioned_RevertsOnRecipientMismatch() public {
        uint256 id = _create(0, block.number + 100, CONDITION_ID);
        bytes memory journal = _journal(address(escrow), id, sender, attacker, AMOUNT, CONDITION_ID);

        vm.prank(recipient);
        vm.expectRevert(Escrow.JournalBindingMismatch.selector);
        escrow.finishEscrow(id, hex"c0ffee", journal);
    }

    function test_FinishConditioned_RevertsOnConditionMismatch() public {
        uint256 id = _create(0, block.number + 100, CONDITION_ID);
        bytes memory journal =
            _journal(address(escrow), id, sender, recipient, AMOUNT, keccak256("other-condition"));

        vm.prank(recipient);
        vm.expectRevert(Escrow.JournalBindingMismatch.selector);
        escrow.finishEscrow(id, hex"c0ffee", journal);
    }

    function test_FinishConditioned_RevertsOnAgentMismatch() public {
        uint256 id = _create(0, block.number + 100, CONDITION_ID);
        bytes memory journal = _journal(attacker, id, sender, recipient, AMOUNT, CONDITION_ID);

        vm.prank(recipient);
        vm.expectRevert(Escrow.JournalBindingMismatch.selector);
        escrow.finishEscrow(id, hex"c0ffee", journal);
    }

    function test_FinishConditioned_RevertsOnEscrowIdMismatch() public {
        uint256 id = _create(0, block.number + 100, CONDITION_ID);
        bytes memory journal =
            _journal(address(escrow), id + 1, sender, recipient, AMOUNT, CONDITION_ID);

        vm.prank(recipient);
        vm.expectRevert(Escrow.JournalBindingMismatch.selector);
        escrow.finishEscrow(id, hex"c0ffee", journal);
    }

    function test_FinishConditioned_RevertsOnBadVersion() public {
        uint256 id = _create(0, block.number + 100, CONDITION_ID);
        bytes memory journal =
            _journal(address(escrow), id, sender, recipient, AMOUNT, CONDITION_ID);
        journal[0] = bytes1(uint8(2));

        vm.prank(recipient);
        vm.expectRevert(Escrow.MalformedJournal.selector);
        escrow.finishEscrow(id, hex"c0ffee", journal);
    }

    function test_FinishConditioned_RevertsOnFailureOutcome() public {
        uint256 id = _create(0, block.number + 100, CONDITION_ID);
        bytes memory journal =
            _journal(address(escrow), id, sender, recipient, AMOUNT, CONDITION_ID);
        journal[2] = bytes1(uint8(0)); // outcome: failure

        vm.prank(recipient);
        vm.expectRevert(Escrow.MalformedJournal.selector);
        escrow.finishEscrow(id, hex"c0ffee", journal);
    }

    function test_FinishConditioned_RevertsOnTrailingBytes() public {
        uint256 id = _create(0, block.number + 100, CONDITION_ID);
        bytes memory journal = abi.encodePacked(
            _journal(address(escrow), id, sender, recipient, AMOUNT, CONDITION_ID), uint8(0)
        );

        vm.prank(recipient);
        vm.expectRevert(Escrow.MalformedJournal.selector);
        escrow.finishEscrow(id, hex"c0ffee", journal);
    }

    function test_FinishConditioned_RevertsOnTruncatedJournal() public {
        uint256 id = _create(0, block.number + 100, CONDITION_ID);
        bytes memory full = _journal(address(escrow), id, sender, recipient, AMOUNT, CONDITION_ID);
        bytes memory journal = new bytes(full.length - 1);
        for (uint256 i = 0; i < journal.length; ++i) {
            journal[i] = full[i];
        }

        vm.prank(recipient);
        vm.expectRevert(Escrow.MalformedJournal.selector);
        escrow.finishEscrow(id, hex"c0ffee", journal);
    }

    function test_Cancel_AfterCancelAfter() public {
        uint256 id = _create(block.number + 2, block.number + 5, bytes32(0));
        vm.roll(block.number + 5);

        uint256 before = sender.balance;
        vm.prank(sender);
        escrow.cancelEscrow(id);

        assertEq(sender.balance, before + AMOUNT);
        assertTrue(escrow.getEscrow(id).settled);
    }

    function test_Cancel_RevertsTooEarly() public {
        uint256 id = _create(block.number + 2, block.number + 5, bytes32(0));
        vm.prank(sender);
        vm.expectRevert(Escrow.TooEarlyToCancel.selector);
        escrow.cancelEscrow(id);
    }

    function test_Cancel_RevertsNotSender() public {
        uint256 id = _create(block.number + 2, block.number + 5, bytes32(0));
        vm.roll(block.number + 5);
        vm.prank(attacker);
        vm.expectRevert(Escrow.OnlySender.selector);
        escrow.cancelEscrow(id);
    }

    function test_Cancel_RevertsWhenDisabled() public {
        uint256 id = _create(block.number + 2, 0, bytes32(0));
        vm.prank(sender);
        vm.expectRevert(Escrow.CancelDisabled.selector);
        escrow.cancelEscrow(id);
    }

    function testFuzz_CreateEscrow_AmountRoundTrips(uint256 amount) public {
        amount = bound(amount, 1, 1000 ether);
        vm.deal(sender, amount);
        vm.prank(sender);
        uint256 id = escrow.createEscrow{value: amount}(
            recipient, block.number + 1, block.number + 100, CONDITION_ID
        );
        assertEq(escrow.getEscrow(id).amount, amount);
    }

    function testFuzz_FinishConditioned_BindsAmountAndCondition(uint256 amount, bytes32 conditionId)
        public
    {
        amount = bound(amount, 1, 1000 ether);
        vm.assume(conditionId != bytes32(0));
        vm.deal(sender, amount);

        vm.prank(sender);
        uint256 id =
            escrow.createEscrow{value: amount}(recipient, 0, block.number + 100, conditionId);

        bytes memory journal = _journal(address(escrow), id, sender, recipient, amount, conditionId);

        uint256 before = recipient.balance;
        vm.prank(recipient);
        escrow.finishEscrow(id, hex"c0ffee", journal);
        assertEq(recipient.balance, before + amount);
    }

    function testFuzz_FinishConditioned_RevertsOnAmountMismatch(uint256 good, uint256 bad) public {
        good = bound(good, 1, 1000 ether);
        bad = bound(bad, 1, 1000 ether);
        vm.assume(good != bad);
        vm.deal(sender, good);

        vm.prank(sender);
        uint256 id =
            escrow.createEscrow{value: good}(recipient, 0, block.number + 100, CONDITION_ID);

        bytes memory journal = _journal(address(escrow), id, sender, recipient, bad, CONDITION_ID);

        vm.prank(recipient);
        vm.expectRevert(Escrow.JournalBindingMismatch.selector);
        escrow.finishEscrow(id, hex"c0ffee", journal);
    }
}
