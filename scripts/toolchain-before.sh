#!/bin/bash

# download & verify riscv-gcc-prebuilt
if [ -f /tmp/riscv32-unknown-elf.gcc-13.2.0.tar.gz ]; then
    echo "riscv-gcc-prebuilt existed, please check the file manually"
else
    wget -O /tmp/riscv32-unknown-elf.gcc-13.2.0.tar.gz https://github.com/stnolting/riscv-gcc-prebuilt/releases/download/rv32i-131023/riscv32-unknown-elf.gcc-13.2.0.tar.gz
    if [ $? -ne 0 ]; then
        echo "failed to download riscv-gcc-prebuilt"
        exit 1
    fi
fi

# This is a workaround tos set up cargo toolchain in CI
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
echo $TOOLCHAIN_VERSION > $ROOT_TOOLCHAIN
