#!/bin/bash

# This is a workaround tos set up cargo component in CI
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
    CRATE_TOOLCHAIN="provers/risc0/driver/rust-toolchain.toml"
    echo "rust-toolchain.toml has been copied from Risc0 prover to the workspace root."
elif [ "$1" == "sp1" ]; then
    CRATE_TOOLCHAIN="provers/sp1/driver/rust-toolchain.toml"
    echo "rust-toolchain.toml has been copied from Sp1 prover to the workspace root."
else
  echo "Skip toolchain modification"
  exit 1
fi

# Extract the channel (toolchain version) from the selected crate toolchain file
TOOLCHAIN_VERSION=$(grep "channel = " $CRATE_TOOLCHAIN | cut -d '"' -f 2)

# Extract components from the selected crate toolchain file and add them using rustup
grep "components = \[" $CRATE_TOOLCHAIN | sed 's/.*\[\(.*\)\].*/\1/' | tr ',' '\n' | while read -r component; do
    component=$(echo $component | xargs | tr -d '"')
    if [ ! -z "$component" ]; then
        echo "Adding component: $component"
        rustup component add "$component" --toolchain "$TOOLCHAIN_VERSION"
    fi
done