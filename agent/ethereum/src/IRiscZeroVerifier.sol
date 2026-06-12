// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.28;

/// @title RISC Zero verifier interface.
/// @notice Minimal declaration of the on-chain RISC Zero verifier, sufficient
///         for an application that only needs to check a receipt. In production
///         this is the address of a `RiscZeroVerifierRouter`, which dispatches
///         to the base verifier matching the zkVM version that produced the
///         receipt. The interface mirrors `risc0/risc0-ethereum`; only `verify`
///         is declared here because that is the only call this project makes.
interface IRiscZeroVerifier {
    /// @notice Verify that `seal` is a valid proof for `imageId` over
    ///         `journalDigest`, where `journalDigest` is the SHA-256 digest of
    ///         the journal committed by the guest.
    /// @dev Reverts if the seal is not a verifying proof. Returns normally on
    ///      success; it has no return value, so a non-reverting call is the
    ///      success signal.
    /// @param seal The encoded cryptographic proof (selector-prefixed Groth16).
    /// @param imageId The cryptographic identifier of the guest program.
    /// @param journalDigest The SHA-256 digest of the committed journal.
    function verify(bytes calldata seal, bytes32 imageId, bytes32 journalDigest) external view;
}
