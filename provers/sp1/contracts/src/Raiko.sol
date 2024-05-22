// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {SP1Verifier} from "./SP1Verifier.sol";

/// @title Raiko.
/// @author Succinct Labs
/// @notice This contract implements a simple example of verifying the proof of a computing a 
///         raiko number.
contract Raiko is SP1Verifier {
    /// @notice The verification key for the raiko program.
    bytes32 public raikoProgramVkey;

    struct Transition {
        bytes32 parentHash;
        bytes32 blockHash;
        bytes32 stateRoot;
        bytes32 graffiti;
    }

    constructor(bytes32 _raikoProgramVkey) {
        raikoProgramVkey = _raikoProgramVkey;
    }

    /// @notice The entrypoint for verifying the proof of a raiko number.
    /// @param proof The encoded proof.
    /// @param publicValues The encoded public values.
    function verifyRaikoProof(
        bytes memory proof,
        bytes memory publicValues
    ) public view/* returns (uint64, address, Transition memory, address, address, bytes32)*/ {

        this.verifyProof(raikoProgramVkey, publicValues, proof);
        /*(
            uint64 chain_id,
            address verifier_address,
            Transition memory transition,
            address sgx_instance,
            address prover,
            bytes32 meta_hash
        ) = abi.decode(publicValues, (uint64, address, Transition, address, address, bytes32));

        return (chain_id, verifier_address, transition, sgx_instance, prover, meta_hash);*/
    }
}
