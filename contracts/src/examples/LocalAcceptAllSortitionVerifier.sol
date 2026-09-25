// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import { ISortitionVerifier } from "../interfaces/ISortitionVerifier.sol";

/// @notice Accept-all verifier used only by the local Anvil end-to-end demo.
/// @dev Never deploy this verifier to a public or production network.
contract LocalAcceptAllSortitionVerifier is ISortitionVerifier {
    function verifySelection(
        bytes32,
        uint64,
        bytes32,
        address[] calldata,
        bytes calldata
    ) external pure returns (bool) {
        return true;
    }
}
