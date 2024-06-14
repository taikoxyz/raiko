#!/usr/bin/env bash

# Any error will result in failure
set -e

TOOLCHAIN_RISC0=+nightly-2024-04-18
TOOLCHAIN_SP1=+nightly-2024-04-18
TOOLCHAIN_SGX=+nightly-2024-04-18

check_toolchain() {
	local TOOLCHAIN=$1

	# Remove the plus sign from the toolchain name
	TOOLCHAIN=${TOOLCHAIN#+}

	# Function to check if the toolchain is installed
	exist() {
		rustup toolchain list | grep "$TOOLCHAIN" >/dev/null
	}

	# Main script logic
	if exist; then
		echo "Toolchain $TOOLCHAIN exists"
	else
		echo "Installing Rust toolchain: $TOOLCHAIN"
		rustup install "$TOOLCHAIN"
	fi
}

# NATIVE
if [ -z "$1" ] || [ "$1" == "native" ]; then
	cargo clippy -- -D warnings
fi

# SGX
if [ -z "$1" ] || [ "$1" == "sgx" ]; then
	check_toolchain $TOOLCHAIN_SGX
	cargo ${TOOLCHAIN_SGX} clippy -p raiko-host -p sgx-prover -F "sgx enable" -- -D warnings
fi

# SP1
if [ -z "$1" ] || [ "$1" == "sp1" ]; then
	check_toolchain $TOOLCHAIN_SP1
	cargo ${TOOLCHAIN_SP1} clippy -p raiko-host -p sp1-builder -p sp1-driver -F "sp1 enable"
fi

# RISC0
if [ -z "$1" ] || [ "$1" == "risc0" ]; then
	check_toolchain $TOOLCHAIN_RISC0
	./script/setup-bonsai.sh
	cargo ${TOOLCHAIN_RISC0} run --bin risc0-builder
	cargo ${TOOLCHAIN_RISC0} clippy -p raiko-host -p risc0-builder -p risc0-driver -F "risc0 enable"
fi
