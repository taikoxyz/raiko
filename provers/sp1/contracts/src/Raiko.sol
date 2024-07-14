// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {ISP1Verifier} from "@sp1-contracts/ISP1Verifier.sol";

/// @title Raiko.
/// @author Taiko Labs
/// @notice This contract implements a simple example of verifying the proof of a computing a 
///         raiko number.
contract Raiko {
    /// @notice The address of the SP1 verifier contract.
    /// @dev This can either be a specific SP1Verifier for a specific version, or the
    ///      SP1VerifierGateway which can be used to verify proofs for any version of SP1.
    ///      For the list of supported verifiers on each chain, see:
    ///      https://github.com/succinctlabs/sp1-contracts/tree/main/contracts/deployments
    address public verifier;

    /// @notice The verification key for the raiko program.
    bytes32 public raikoProgramVkey;


    constructor(address _verifier, bytes32 _raikoProgramVkey) {
        verifier = _verifier;
        raikoProgramVkey = _raikoProgramVkey;
    }

    /// @notice The entrypoint for verifying the proof of a Raiko number.
    /// @param proof The encoded proof.
    /// @param publicValues The encoded public values.
    function verifyRaikoProof(bytes calldata proof, bytes calldata publicValues)
        public
        view
        returns (bytes32)
    {
        ISP1Verifier(verifier).verifyProof(raikoProgramVkey, publicValues, proof);
        bytes32 pi_hash = abi.decode(publicValues, (bytes32));
        return pi_hash;
    }
}
