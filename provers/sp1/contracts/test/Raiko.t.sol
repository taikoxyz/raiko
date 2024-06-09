// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {Test} from "forge-std/Test.sol";
import {stdJson} from "forge-std/StdJson.sol";
import {Raiko} from "../src/Raiko.sol";
import {SP1Verifier} from "../src/SP1Verifier.sol";
import "forge-std/console.sol";

struct RaikoProofFixture {
    uint64 chain_id;
    address verifier_address;
    Raiko.Transition transition;
    address sgx_instance;
    address prover;
    bytes32 meta_hash;
    bytes32 vkey;
    bytes proof;
    bytes publicValues;
}

contract RaikoTest is Test {
    using stdJson for string;

    Raiko public raiko;

    function loadFixture() public view returns (RaikoProofFixture memory) {
        string memory root = vm.projectRoot();
        string memory path = string.concat(root, "/src/fixtures/fixture.json");
        string memory json = vm.readFile(path);
        bytes memory jsonBytes = json.parseRaw(".");
        return abi.decode(jsonBytes, (RaikoProofFixture));
    }

    function setUp() public {
        RaikoProofFixture memory fixture = loadFixture();
        raiko = new Raiko(fixture.vkey);
    }

    function test_ValidRaikoProof() public view {
        RaikoProofFixture memory fixture = loadFixture();
        //console.logUint(fixture.chain_id);
        //console.logBytes(fixture.randomName);
        console.logBytes(fixture.proof);
        console.logBytes32(fixture.vkey);
        (
            uint64 chain_id,
            address verifier_address,
            Raiko.Transition memory transition,
            address sgx_instance,
            address prover,
            bytes32 meta_hash
        ) = raiko.verifyRaikoProof(
            fixture.proof,
            fixture.publicValues
        );
        // assertEq(chain_id, fixture.chain_id);
        // assertEq(verifier_address, fixture.verifier_address);
        // assertEq(transition, fixture.transition);
        // assertEq(sgx_instance, fixture.sgx_instance);
        // assertEq(prover, fixture.prover);
        // assertEq(meta_hash, fixture.meta_hash);
    }

    function testFail_InvalidRaikoProof() public view {
        RaikoProofFixture memory fixture = loadFixture();
        raiko.verifyRaikoProof(
            fixture.publicValues,
            fixture.publicValues
        );
    }
}
