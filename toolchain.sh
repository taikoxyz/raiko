#!/usr/bin/env bash

if [ "$1" == "native" ]; then
	echo "Skip toolchain modification"
elif [ "$1" == "sgx" ]; then
	cp provers/sgx/prover/rust-toolchain.toml ./
	echo "rust-toolchain.toml has been copied from SGX prover to the workspace root."
elif [ "$1" == "risc0" ]; then
    cp provers/risc0/driver/rust-toolchain.toml ./
    echo "rust-toolchain.toml has been copied from Risc0 prover to the workspace root."
elif [ "$1" == "sp1" ]; then
    cp provers/sp1/driver/rust-toolchain.toml ./
    echo "rust-toolchain.toml has been copied from Sp1 prover to the workspace root."
else
	echo "Skip toolchain modification"
	exit 1
fi

# Check if the file exists
if [ -f "rust-toolchain" ]; then
    # If the file exists, remove it
    rm "some-file"
    echo "File removed successfully."
fi