#!/usr/bin/env bash
set -e

TOOLCHAIN_RISC0=+nightly-2024-04-17
TOOLCHAIN_SP1=+nightly-2024-04-17
TOOLCHAIN_SGX=+nightly-2024-04-17

if [ -z "${DEBUG}" ]; then
	FLAGS=--release
fi


if [ -z "${RUN}" ]; then
	COMMAND=build
else
	COMMAND=run
fi

# SGX
if [ -z "$1" ] || [ "$1" == "sgx" ]; then
	if [ -z "${TEST}" ]; then
		cargo ${TOOLCHAIN_SGX} ${COMMAND} ${FLAGS} --features sgx
	else
		cargo ${TOOLCHAIN_SGX} test ${FLAGS} -p sgx-prover --features enable
	fi
fi
# RISC0
if [ -z "$1" ] || [ "$1" == "risc0" ]; then
	if [ -z "${TEST}" ]; then
		cargo ${TOOLCHAIN_RISC0} run --bin risc0-builder
		cargo ${TOOLCHAIN_RISC0} ${COMMAND} ${FLAGS} --features risc0
	else
		RISC0_DEV_MODE=1 cargo ${TOOLCHAIN_RISC0} test ${FLAGS} -p risc0-driver --features enable
	fi
fi
# SP1
if [ -z "$1" ] || [ "$1" == "sp1" ]; then
	if [ -z "${TEST}" ]; then
		cargo ${TOOLCHAIN_SP1} run --bin sp1-builder
		cargo ${TOOLCHAIN_SP1} ${COMMAND} ${FLAGS} --features sp1
	else
		cargo ${TOOLCHAIN_SP1} test ${FLAGS} -p sp1-driver --features enable
	fi
fi