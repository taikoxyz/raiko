#!/usr/bin/env bash

TOOLCHAIN_RISC0=+nightly-2024-02-06
TOOLCHAIN_SP1=+nightly-2024-02-06
TOOLCHAIN_SGX=+nightly-2024-02-06


check_toolchain() {
	local TOOLCHAIN=$1

	# Remove the plus sign from the toolchain name
    TOOLCHAIN=${TOOLCHAIN#+}

	# Function to check if the toolchain is installed
	exist() {
		rustup toolchain list | grep "$TOOLCHAIN" > /dev/null
	}

	# Main script logic
	if exist; then
		echo "Toolchain $TOOLCHAIN exists"
	else
		echo "Installing Rust toolchain: $TOOLCHAIN"
		rustup install "$TOOLCHAIN"
	fi
} 

if [ -z "${DEBUG}" ]; then
	FLAGS=--release
else 
	echo "Warning: in debug mode"
fi

if [ -z "${RUN}" ]; then
	COMMAND=build
else
	COMMAND=run
fi

# SGX
if [ -z "$1" ] || [ "$1" == "sgx" ]; then
	check_toolchain $TOOLCHAIN_SGX
	if [ -z "${RUN}" ]; then
		if [ -z "${TEST}" ]; then
			cargo ${TOOLCHAIN_SGX} build ${FLAGS} --features sgx
		else
			cargo ${TOOLCHAIN_SGX} test ${FLAGS} -p sgx-prover --features enable --no-run
		fi
	else
		if [ -z "${TEST}" ]; then
			cargo ${TOOLCHAIN_SGX} run ${FLAGS} --features sgx
		else
			cargo ${TOOLCHAIN_SGX} test ${FLAGS} -p sgx-prover --features enable
		fi
	fi
fi
# RISC0
if [ -z "$1" ] || [ "$1" == "risc0" ]; then
	check_toolchain $TOOLCHAIN_RISC0
	if [ -z "${RUN}" ]; then
		if [ -z "${TEST}" ]; then
			echo "Building Risc0 prover"
			cargo ${TOOLCHAIN_RISC0} run --bin risc0-builder
		else
			echo "Building test elfs for Risc0 prover"
			cargo ${TOOLCHAIN_RISC0} run --bin risc0-builder --features test
		fi
		cargo ${TOOLCHAIN_RISC0} build ${FLAGS} --features risc0
	else
		if [ -z "${TEST}" ]; then
			echo "Running Sp1 prover"
			cargo ${TOOLCHAIN_RISC0} run ${FLAGS} --features risc0
		else
			echo "Running Sp1 tests"
			RISC0_DEV_MODE=1 cargo ${TOOLCHAIN_RISC0} test ${FLAGS} -p risc0-driver --features enable
		fi
	fi
fi

# SP1
if [ -z "$1" ] || [ "$1" == "sp1" ]; then
	check_toolchain $TOOLCHAIN_SP1
	if [ -z "${RUN}" ]; then
		if [ -z "${TEST}" ]; then
			echo "Building Sp1 prover"
			cargo ${TOOLCHAIN_SP1} run --bin sp1-builder
		else
			echo "Building test elfs for Sp1 prover"
			cargo ${TOOLCHAIN_SP1} run --bin sp1-builder --features test
		fi
		cargo ${TOOLCHAIN_SP1} build ${FLAGS} --features sp1
	else
		if [ -z "${TEST}" ]; then
			echo "Running Sp1 prover"
			cargo ${TOOLCHAIN_SP1} run ${FLAGS} --features sp1
		else
			echo "Running Sp1 tests"
			cargo ${TOOLCHAIN_SP1} test ${FLAGS} -p sp1-driver --features enable
		fi
	fi
fi