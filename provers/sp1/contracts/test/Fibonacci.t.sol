// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {Test, console} from "forge-std/Test.sol";
import {stdJson} from "forge-std/StdJson.sol";
import {Fibonacci} from "../src/Fibonacci.sol";
import {SP1Verifier} from "../src/SP1Verifier.sol";

struct SP1ProofFixtureJson {
    uint32 a;
    uint32 b;
    uint32 n;
    bytes proof;
    bytes publicValues;
    bytes32 vkey;
}

contract FibonacciTest is Test {
    using stdJson for string;

    Fibonacci public fibonacci;

    function loadFixture() public view returns (SP1ProofFixtureJson memory) {
        string memory root = vm.projectRoot();
        string memory path = string.concat(root, "/src/fixtures/fixture.json");
        string memory json = vm.readFile(path);
        bytes memory jsonBytes = json.parseRaw(".");
        return abi.decode(jsonBytes, (SP1ProofFixtureJson));
    }

    function setUp() public {
        SP1ProofFixtureJson memory fixture = loadFixture();
        fibonacci = new Fibonacci(fixture.vkey);
    }

    function test_ValidFibonacciProof() public view {
        SP1ProofFixtureJson memory fixture = loadFixture();
        (uint32 n, uint32 a, uint32 b) = fibonacci.verifyFibonacciProof(
            fixture.proof,
            fixture.publicValues
        );
        assert(n == fixture.n);
        assert(a == fixture.a);
        assert(b == fixture.b);
    }

    function testFail_InvalidFibonacciProof() public view {
         SP1ProofFixtureJson memory fixture = loadFixture();
        fibonacci.verifyFibonacciProof(
            fixture.publicValues,
            fixture.publicValues
        );
    } 
}
