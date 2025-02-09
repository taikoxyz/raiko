#!/usr/bin/env bash

# Any error will result in failure
set -e

# report the CI image status
if [ -n "$CI" ]; then
    source ./script/ci-env-check.sh
fi

# toolchain necessary to compile c-kzg in SP1/risc0
if [ -z "$1" ] || [ "$1" == "sp1" ] || [ "$1" == "risc0" ]; then
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

	if [ -z "${CI}" ] || [ ! command -v rzup &> /dev/null ]; then
		PROFILE=$HOME/.bashrc
		echo ${PROFILE}
		source ${PROFILE}
		rzup install
	else
		echo "/home/runner/.config/.risc0/bin" >> $GITHUB_PATH
		echo $GITHUB_PATH
		/home/runner/.risc0/bin/rzup --verbose install
	fi
fi
# SP1
if [ -z "$1" ] || [ "$1" == "sp1" ]; then
	curl -L https://sp1.succinct.xyz | bash
	echo "SP1 installed"
	if [ -z "${CI}" ] || [ ! command -v sp1up &> /dev/null ]; then
		echo "Non-CI environment"
		# Need to add sp1up to the path here
		PROFILE=$HOME/.bashrc
		echo ${PROFILE}
		source ${PROFILE}
		sp1up -v v4.0.0-rc.1
	else
		echo "CI environment"
		source /home/runner/.bashrc
		echo "/home/runner/.sp1/bin" >> $GITHUB_PATH
		/home/runner/.sp1/bin/sp1up
	fi
fi
