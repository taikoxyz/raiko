#!/usr/bin/env bash

if [ "$1" == "native" ]; then
    echo "Skip toolchain modification"
elif [ "$1" == "sgx" ]; then
    cp provers/sgx/prover/rust-toolchain.toml ./
    echo "rust-toolchain.toml has been copied from SGX prover to the workspace root."
elif [ "$1" == "risc0" ]; then
    cp provers/risc0/rust-toolchain.toml ./
    echo "rust-toolchain.toml has been copied from Risc0 prover to the workspace root."
elif [ "$1" == "sp1" ]; then
    cp provers/sp1/prover/rust-toolchain.toml ./
    echo "rust-toolchain.toml has been copied from Sp1 prover to the workspace root."
else
  echo "Skip toolchain modification"
  exit 1
fi

rm rust-toolchain
