// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {SP1Verifier} from "./SP1Verifier.sol";

/// @title Fibonacci.
/// @author Succinct Labs
/// @notice This contract implements a simple example of verifying the proof of a computing a 
///         fibonacci number.
contract Fibonacci is SP1Verifier {
    /// @notice The verification key for the fibonacci program.
    bytes32 public fibonacciProgramVkey;

    constructor(bytes32 _fibonacciProgramVkey) {
        fibonacciProgramVkey = _fibonacciProgramVkey;
    } 

    /// @notice The entrypoint for verifying the proof of a fibonacci number.
    /// @param proof The encoded proof.
    /// @param publicValues The encoded public values.
    function verifyFibonacciProof(
        bytes memory proof,
        bytes memory publicValues
    ) public view returns (uint32, uint32, uint32) {
        this.verifyProof(fibonacciProgramVkey, publicValues, proof);
        (uint32 n, uint32 a, uint32 b) = abi.decode(publicValues, (uint32, uint32, uint32));
        return (n, a, b);
    }
}
