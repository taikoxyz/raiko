#!/usr/bin/env bash

# Any error will result in failure
set -e

# report the CI image status
if [ -n "$CI" ]; then
	source ./script/ci-env-check.sh
fi

# toolchain necessary to compile c-kzg in SP1/risc0
if [ -z "$1" ] || [ "$1" == "sp1" ] || [ "$1" == "risc0" ] || [ "$1" == "openvm" ]; then
	# Check if the RISC-V GCC prebuilt binary archive already exists
	if [ -f /tmp/riscv32-unknown-elf.gcc-13.2.0.tar.gz ]; then
		echo "riscv-gcc-prebuilt existed, please check the file manually"
	else
		# Download the file using wget
		wget -O /tmp/riscv32-unknown-elf.gcc-13.2.0.tar.gz https://github.com/stnolting/riscv-gcc-prebuilt/releases/download/rv32i-131023/riscv32-unknown-elf.gcc-13.2.0.tar.gz
		# Check if wget succeeded
		if [ $? -ne 0 ]; then
			echo "Failed to download riscv-gcc-prebuilt"
			exit 1
		fi
		# Create the directory if it doesn't exist
		if [ ! -d /opt/riscv ]; then
			mkdir /opt/riscv
		fi
		# Extract the downloaded archive
		tar -xzf /tmp/riscv32-unknown-elf.gcc-13.2.0.tar.gz -C /opt/riscv/
		# Check if tar succeeded
		if [ $? -ne 0 ]; then
			echo "Failed to extract riscv-gcc-prebuilt"
			exit 1
		fi
	fi
fi

# SGX
if [ -z "$1" ] || [ "$1" == "sgx" ]; then
	# also check if sgx is already installed
	if command -v gramine-sgx >/dev/null 2>&1; then
		echo "gramine already installed"
	else
		echo "gramine not installed, installing..."
		# For SGX, install gramine: https://github.com/gramineproject/gramine.
		wget -O /tmp/gramine.deb https://packages.gramineproject.io/pool/main/g/gramine/gramine_1.6.2_amd64.deb
		sudo apt install -y /tmp/gramine.deb
	fi
fi
# RISC0
if [ -z "$1" ] || [ "$1" == "risc0" ]; then
	echo "Current TERM: $TERM"
	if [ -z "$TERM" ] || [ "$TERM" = "dumb" ]; then
		# Set TERM to xterm-color256
		echo "Setting TERM to xterm"
		export TERM=xterm
	fi
	curl -L https://risczero.com/install | bash

	env_rzup=rzup
	if [ -z "${CI}" ] || ! command -v rzup >/dev/null 2>&1; then
		PROFILE=$HOME/.bashrc
		echo "Load PROFILE: $PROFILE"
		if [ -f "$PROFILE" ]; then
			source "$PROFILE"
		fi
		if ! command -v rzup >/dev/null 2>&1; then
			export PATH="$HOME/.risc0/bin:$PATH"
			env_rzup="$HOME/.risc0/bin/rzup"
		fi
	else
		echo "/home/runner/.risc0/bin" >>"$GITHUB_PATH"
		echo "/home/runner/.config/.risc0/bin" >>$GITHUB_PATH
		echo $GITHUB_PATH
		env_rzup=/home/runner/.risc0/bin/rzup
	fi
	echo "start running $env_rzup"
	if ! command -v "$env_rzup" >/dev/null 2>&1; then
		echo "env_rzup is not working, please re-install rzup."
		exit 1
	fi
	$env_rzup install rust 1.85.0
	$env_rzup install cpp 2024.1.5
	$env_rzup install r0vm 2.0.2
	$env_rzup install cargo-risczero 2.0.2
fi
# SP1
if [ -z "$1" ] || [ "$1" == "sp1" ]; then
	curl -L https://sp1.succinct.xyz | bash
	echo "SP1 installed"
	# if [ -z "${CI}" ] || [ ! command -v sp1up &> /dev/null ]; then
	# echo "Non-CI environment"
	# Need to add sp1up to the path here
	PROFILE=$HOME/.profile
	echo ${PROFILE}
	source ${PROFILE}
	if command -v sp1up >/dev/null 2>&1; then
		echo "sp1 found in path"
		sp1up -v v4.1.7
	else
		echo "sp1 not found in path"
		"$HOME/.sp1/bin/sp1up" -v v4.1.7
	fi
	# else
	# 	echo "CI environment"
	# 	source /home/runner/.bashrc
	# 	echo "/home/runner/.sp1/bin" >> $GITHUB_PATH
	# 	/home/runner/.sp1/bin/sp1up
	# fi
fi

# OPENVM
if [ -z "$1" ] || [ "$1" == "openvm" ]; then
	echo "Installing OpenVM..."

	# Install nightly toolchain for OpenVM
	OPENVM_TOOLCHAIN=nightly-2025-02-14
	rustup install $OPENVM_TOOLCHAIN
	rustup component add rust-src --toolchain $OPENVM_TOOLCHAIN

	# Add RISC-V target for OpenVM guest compilation
	rustup target add riscv32im-unknown-none-elf --toolchain $OPENVM_TOOLCHAIN

	# Install additional dependencies for OpenVM
	# svm-rs for Solidity compiler management
	if ! command -v svm >/dev/null 2>&1; then
		echo "Installing svm-rs..."
		cargo install --version 0.5.7 svm-rs
	else
		echo "svm already installed"
	fi

	# Install solc version needed by OpenVM
	if ! svm list | grep -q "0.8.19"; then
		echo "Installing solc 0.8.19..."
		svm install 0.8.19
	else
		echo "solc 0.8.19 already installed"
	fi

	# Install cargo-openvm CLI tool
	if ! command -v cargo-openvm >/dev/null 2>&1; then
		echo "Installing cargo-openvm CLI..."
		cargo +1.86 install --locked --git https://github.com/openvm-org/openvm.git --tag v1.4.0 cargo-openvm
	else
		echo "cargo-openvm already installed"
		cargo openvm --version
	fi

	echo "OpenVM installation complete!"
fi