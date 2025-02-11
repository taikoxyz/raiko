#!/usr/bin/env bash

# Any error will result in failure
set -e

TOOLCHAIN_RISC0=+nightly-2024-12-20
TOOLCHAIN_SP1=+nightly-2024-12-20
TOOLCHAIN_SGX=+nightly-2024-12-20
export PROOF_TYPE="$1"

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

if [ "$CPU_OPT" = "1" ]; then
	export RUSTFLAGS='-C target-cpu=native'
	echo "Enable cpu optimization with host RUSTFLAGS"
fi

# NATIVE
if [ -z "$1" ] || [ "$1" == "native" ]; then
	cargo test -F integration run_scenarios_sequentially
fi

# SGX
if [ "$1" == "sgx" ]; then
	check_toolchain $TOOLCHAIN_SGX
	if [ "$MOCK" = "1" ]; then
		export SGX_DIRECT=1
	fi
	cargo ${TOOLCHAIN_SGX} test -F "sgx integration" run_scenarios_sequentially
fi

# RISC0
if [ "$1" == "risc0" ]; then
	check_toolchain $TOOLCHAIN_RISC0
	./script/setup-bonsai.sh
	cargo ${TOOLCHAIN_RISC0} run --bin risc0-builder -F test
	cargo ${TOOLCHAIN_RISC0} test -F "risc0 integration" run_scenarios_sequentially
fi

# SP1
if [ "$1" == "sp1" ]; then
	check_toolchain $TOOLCHAIN_SP1
	cargo ${TOOLCHAIN_SP1} run --bin sp1-builder
	cargo ${TOOLCHAIN_SP1} test -F "sp1 integration" run_scenarios_sequentially
fi
