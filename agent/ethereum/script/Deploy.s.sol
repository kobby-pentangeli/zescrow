// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

import {Script} from "forge-std/Script.sol";
import {Escrow} from "../src/Escrow.sol";
import {IRiscZeroVerifier} from "../src/IRiscZeroVerifier.sol";

/// @title Escrow deployment script.
/// @notice Deploys the escrow pinned to a RISC Zero verifier router and guest
///         image id. The signing key is supplied by the `forge` CLI
///         (`--ledger`, `--account`, or `--private-key`), never read here, so no
///         secret enters the script. The verifier address and image id come from
///         the environment:
///         - `ZESCROW_VERIFIER`: the RISC Zero verifier router for the network.
///         - `ZESCROW_IMAGE_ID`: the audited guest image id (bytes32).
contract Deploy is Script {
    function run() external returns (Escrow escrow) {
        address verifier = vm.envAddress("ZESCROW_VERIFIER");
        bytes32 imageId = vm.envBytes32("ZESCROW_IMAGE_ID");

        vm.startBroadcast();
        escrow = new Escrow(IRiscZeroVerifier(verifier), imageId);
        vm.stopBroadcast();
    }
}
