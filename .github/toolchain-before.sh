#!/bin/bash

CRATE_TOOLCHAIN=""
ROOT_TOOLCHAIN="rust-toolchain"

# Define paths
if [ "$1" == "native" ]; then
    echo "Skip toolchain modification"
    exit 1
elif [ "$1" == "sgx" ]; then
    CRATE_TOOLCHAIN="provers/sgx/prover/rust-toolchain.toml"
    echo "rust-toolchain.toml has been copied from SGX prover to the workspace root."
elif [ "$1" == "risc0" ]; then
    CRATE_TOOLCHAIN="provers/risc0/rust-toolchain.toml"
    echo "rust-toolchain.toml has been copied from Risc0 prover to the workspace root."
elif [ "$1" == "sp1" ]; then
    CRATE_TOOLCHAIN="provers/sp1/prover/rust-toolchain.toml"
    echo "rust-toolchain.toml has been copied from Sp1 prover to the workspace root."
else
  echo "Skip toolchain modification"
  exit 1
fi

# Extract the channel (toolchain version) from the selected crate toolchain file
TOOLCHAIN_VERSION=$(grep "channel = " $CRATE_TOOLCHAIN | cut -d '"' -f 2)
echo $TOOLCHAIN_VERSION > $ROOT_TOOLCHAIN
