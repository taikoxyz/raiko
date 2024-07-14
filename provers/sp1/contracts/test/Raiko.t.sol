// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {Test, console} from "forge-std/Test.sol";
import {stdJson} from "forge-std/StdJson.sol";
import {Raiko} from "../src/Raiko.sol";
import {SP1VerifierGateway} from "@sp1-contracts/SP1VerifierGateway.sol";

struct RaikoProofFixture {
    bytes32 pi_hash;
    bytes proof;
    bytes publicValues;
    bytes32 vkey;
}

struct SP1ProofFixtureJson {
    uint32 a;
    uint32 b;
    uint32 n;
    bytes proof;
    bytes publicValues;
    bytes32 vkey;
}


contract RaikoTest is Test {
    using stdJson for string;

    address verifier;
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
        verifier = address(new SP1VerifierGateway(address(1)));

        raiko = new Raiko(verifier, fixture.vkey);
    }

    function test_ValidRaikoProof() public {
        RaikoProofFixture memory fixture = loadFixture();

        vm.mockCall(verifier, abi.encodeWithSelector(SP1VerifierGateway.verifyProof.selector), abi.encode(true));

        bytes32 pi_hash = raiko.verifyRaikoProof(fixture.proof, fixture.publicValues);
        assert(pi_hash == fixture.pi_hash);
    }

    function testFail_InvalidRaikoProof() public view {
        RaikoProofFixture memory fixture = loadFixture();

        // Create a fake proof.
        bytes memory fakeProof = new bytes(fixture.proof.length);

        raiko.verifyRaikoProof(fakeProof, fixture.publicValues);
    }
}
