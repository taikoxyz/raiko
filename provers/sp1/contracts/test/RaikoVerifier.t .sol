// SPDX-License-Identifier: MIT
pragma solidity ^0.8.25;

import {Test, console} from "forge-std/Test.sol";
import {stdJson} from "forge-std/StdJson.sol";
import {RaikoVerifier} from "../src/RaikoVerifier.sol";

struct RaikoProofFixture {
    bytes proof;
    bytes32 publicValues;
    bytes32 vkey;
}

contract RaikoTest is Test {
    using stdJson for string;

    RaikoVerifier public raiko;

    function loadFixture() public view returns (RaikoProofFixture memory) {
        string memory root = vm.projectRoot();
        string memory path = string.concat(root, "/src/fixtures/fixture.json");
        string memory json = vm.readFile(path);
        bytes memory jsonBytes = json.parseRaw(".");
        return abi.decode(jsonBytes, (RaikoProofFixture));
    }

    function setUp() public {
        console.logString("Setting up RaikoTest");
        RaikoProofFixture memory fixture = loadFixture();
        raiko = new RaikoVerifier(fixture.vkey);
    }

    function test_ValidRaikoProof() public {
        RaikoProofFixture memory fixture = loadFixture();
        bytes32 pi_hash = raiko.verifyRaikoProof(
            fixture.proof, 
            abi.encodePacked(fixture.publicValues)
        );
    }

    function testFail_InvalidRaikoProof() public view {
        RaikoProofFixture memory fixture = loadFixture();

        // Create a fake proof.
        bytes memory fakeProof = new bytes(fixture.proof.length);

        raiko.verifyRaikoProof(fakeProof, abi.encodePacked(fixture.publicValues));
    }
}
