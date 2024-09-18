// SPDX-License-Identifier: MIT
pragma solidity ^0.8.25;

import {SP1Verifier} from "./exports/SP1VerifierPlonk.sol";
import "forge-std/console.sol";  


/// @title Raiko.
/// @author Taiko Labs
/// @notice This contract implements a simple example of verifying the proof of a computing a 
///         raiko number.
contract RaikoVerifier is SP1Verifier {

    /// @notice The verification key for the raiko program.
    bytes32 public raikoProgramVkey;


    constructor(bytes32 _raikoProgramVkey) {
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
        this.verifyProof(raikoProgramVkey, publicValues, proof);
        
        console.logBytes(publicValues);
        console.logBytes32(raikoProgramVkey);
        console.logBytes(proof);

        bytes32 pi_hash = abi.decode(publicValues, (bytes32));
        return pi_hash;
    }

}
