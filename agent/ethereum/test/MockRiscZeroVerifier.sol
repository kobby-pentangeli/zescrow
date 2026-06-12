// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import {IRiscZeroVerifier} from "../src/IRiscZeroVerifier.sol";

/// @title Test-only RISC Zero verifier stub.
/// @notice Stands in for the on-chain verifier router so tests exercise the
///         escrow's binding and settlement logic without a real proof. It
///         either accepts every receipt (`accept`) or accepts only the receipt
///         whose `journalDigest` matches `expectedDigest`, letting a test assert
///         the contract forwards `sha256(journal)` correctly.
contract MockRiscZeroVerifier is IRiscZeroVerifier {
    error MockVerificationFailed();

    bool public accept;
    bytes32 public expectedDigest;

    constructor(bool accept_) {
        accept = accept_;
    }

    function setAccept(bool value) external {
        accept = value;
    }

    function setExpectedDigest(bytes32 digest) external {
        expectedDigest = digest;
    }

    function verify(bytes calldata, bytes32, bytes32 journalDigest) external view {
        if (accept) return;
        if (expectedDigest != bytes32(0) && journalDigest == expectedDigest) {
            return;
        }
        revert MockVerificationFailed();
    }
}
