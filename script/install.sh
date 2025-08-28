#!/usr/bin/env bash

# Any error will result in failure
set -e

# GPU support for Zisk proofs is automatically enabled when CUDA toolkit is detected.
# The installation script will automatically rebuild Zisk with GPU features if CUDA is available.
# 
# Prerequisites for GPU support:
# - NVIDIA GPU  
# - CUDA Toolkit installed (https://developer.nvidia.com/cuda-toolkit)
# 
# For brand new environments: Just run `TARGET=zisk make install` - GPU support will be automatically
# configured if CUDA toolkit is available on the system.

# report the CI image status
if [ -n "$CI" ]; then
	source ./script/ci-env-check.sh
fi

# toolchain necessary to compile c-kzg in SP1/risc0 (32-bit)
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

# RISC-V64 bare-metal toolchain necessary for Zisk
if [ -z "$1" ] || [ "$1" == "zisk" ]; then
	# Check if we can use the existing riscv toolchain with 64-bit support
	if [ -f /opt/riscv/bin/riscv-none-elf-gcc ]; then
		echo "Checking if existing RISC-V toolchain supports 64-bit..."
		# Test if the existing toolchain can compile for rv64ima
		if /opt/riscv/bin/riscv-none-elf-gcc -march=rv64ima -mabi=lp64 -S -o /dev/null -xc /dev/null 2>/dev/null; then
			echo "Existing RISC-V toolchain supports 64-bit, will use /opt/riscv/bin/riscv-none-elf-gcc"
		else
			echo "Warning: Existing RISC-V toolchain doesn't support 64-bit."
			echo "You may need to install a 64-bit capable RISC-V toolchain manually."
		fi
	else
		echo "Installing bare-metal RISC-V64 cross-compiler toolchain..."
		if command -v apt-get >/dev/null 2>&1; then
			# Ubuntu/Debian - try bare-metal toolchain first
			sudo apt-get update
			if sudo apt-get install -y gcc-riscv64-unknown-elf 2>/dev/null; then
				echo "Installed gcc-riscv64-unknown-elf"
			else
				echo "gcc-riscv64-unknown-elf not available, trying alternative..."
				# Download prebuilt bare-metal RISC-V64 toolchain
				RISCV64_ARCHIVE="/tmp/riscv64-unknown-elf-gcc.tar.gz"
				echo "Downloading prebuilt RISC-V64 bare-metal toolchain..."
				wget -O "$RISCV64_ARCHIVE" "https://github.com/riscv-collab/riscv-gnu-toolchain/releases/download/2024.02.02/riscv64-elf-ubuntu-22.04-gcc-nightly-2024.02.02-nightly.tar.gz" || {
					echo "Warning: Could not download RISC-V64 toolchain."
					echo "Please install a bare-metal RISC-V64 toolchain manually."
					echo "The existing /opt/riscv toolchain may work if it supports rv64ima."
				}
				if [ -f "$RISCV64_ARCHIVE" ]; then
					echo "Extracting RISC-V64 toolchain to /opt/riscv64..."
					sudo mkdir -p /opt/riscv64
					sudo tar -xzf "$RISCV64_ARCHIVE" -C /opt/riscv64 --strip-components=1
					echo "RISC-V64 bare-metal toolchain installed to /opt/riscv64"
				fi
			fi
		else
			echo "Warning: Could not install RISC-V64 toolchain automatically."
			echo "Please install a bare-metal RISC-V64 toolchain manually."
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

# ZISK
if [ -z "$1" ] || [ "$1" == "zisk" ]; then
	# Always ensure PATH includes zisk bin directory
	export PATH="$HOME/.zisk/bin:$PATH"
	
	# Check if cargo-zisk is already installed
	if command -v cargo-zisk >/dev/null 2>&1; then
		echo "Zisk already installed, version: $(cargo-zisk --version)"
		
		# Check if rust toolchain is installed (needed for zisk compilation)
		if [ ! -f "$HOME/.zisk/bin/rustc" ]; then
			echo "Installing Zisk Rust toolchain..."
			
			# Install using official installation script first if needed
			if [ ! -d "$HOME/.zisk" ]; then
				curl -s https://raw.githubusercontent.com/0xPolygonHermez/zisk/main/ziskup/install.sh | bash
				export PATH="$HOME/.zisk/bin:$PATH"
			fi
			
			# Try to install toolchain, if it fails, do manual extraction
			echo "Attempting to install Zisk Rust toolchain..."
			if ! cargo-zisk sdk install-toolchain; then
				echo "Automatic toolchain installation failed, trying manual extraction..."
				cd "$HOME/.zisk"
				if [ -f "rust-toolchain-x86_64-unknown-linux-gnu.tar.gz" ]; then
					echo "Extracting rust toolchain manually..."
					tar -xzf rust-toolchain-x86_64-unknown-linux-gnu.tar.gz
					if [ -f "$HOME/.zisk/bin/rustc" ]; then
						echo "Rust toolchain extracted successfully"
					else
						echo "Failed to extract rust toolchain"
						exit 1
					fi
				else
					echo "Rust toolchain archive not found, please run: cargo-zisk sdk install-toolchain"
					exit 1
				fi
			fi
		else
			echo "Zisk Rust toolchain already installed"
		fi
		
		# Check if GPU support should be enabled and rebuild if necessary
		if command -v nvcc >/dev/null 2>&1; then
			echo "CUDA toolkit detected, checking if Zisk has GPU support..."
			
			# Check if current binaries were built with GPU support by looking at build timestamp
			# If CUDA is available but binaries are old, rebuild with GPU
			ZISK_BUILD_DATE=$(stat -c %Y "$HOME/.zisk/bin/cargo-zisk" 2>/dev/null || echo "0")
			CURRENT_TIME=$(date +%s)
			REBUILD_THRESHOLD=3600  # Rebuild if binaries are older than 1 hour and no GPU marker exists
			
			if [ ! -f "$HOME/.zisk/.gpu-enabled" ]; then
				echo "Rebuilding Zisk with GPU support for better performance..."
				
				# Clone and build Zisk with GPU features
				TEMP_DIR=$(mktemp -d)
				cd "$TEMP_DIR"
				git clone https://github.com/0xPolygonHermez/zisk.git zisk-gpu-build
				cd zisk-gpu-build
				
				echo "Building Zisk with GPU features (this may take a few minutes)..."
				if cargo build --release --features gpu; then
					# Replace binaries with GPU-enabled versions
					cp target/release/cargo-zisk "$HOME/.zisk/bin/"
					cp target/release/ziskemu "$HOME/.zisk/bin/"
					cp target/release/libzisk_witness.so "$HOME/.zisk/bin/"
					cp target/release/libziskclib.a "$HOME/.zisk/bin/"
					
					# Mark as GPU-enabled
					touch "$HOME/.zisk/.gpu-enabled"
					echo "Zisk successfully rebuilt with GPU support!"
				else
					echo "GPU build failed, continuing with existing binaries"
				fi
				
				# Cleanup
				cd /
				rm -rf "$TEMP_DIR"
			else
				echo "Zisk already has GPU support enabled"
			fi
		fi
	else
		echo "Installing Zisk using prebuilt binaries..."
		
		# Install Zisk using the official installation script
		curl -s https://raw.githubusercontent.com/0xPolygonHermez/zisk/main/ziskup/install.sh | bash
		
		# Ensure PATH is updated
		export PATH="$HOME/.zisk/bin:$PATH"
		
		# Source profile to ensure zisk tools are in PATH
		PROFILE=$HOME/.profile
		if [ -f "$PROFILE" ]; then
			source "$PROFILE"
		fi
		
		# Also try .bashrc if .profile doesn't work
		if ! command -v cargo-zisk >/dev/null 2>&1; then
			PROFILE=$HOME/.bashrc
			if [ -f "$PROFILE" ]; then
				source "$PROFILE"
			fi
		fi
		
		# Verify installation
		if command -v cargo-zisk >/dev/null 2>&1; then
			echo "Zisk installed successfully, version: $(cargo-zisk --version)"
			
			# Install rust toolchain
			echo "Installing Zisk Rust toolchain..."
			if ! cargo-zisk sdk install-toolchain; then
				echo "Automatic toolchain installation failed, trying manual extraction..."
				cd "$HOME/.zisk"
				if [ -f "rust-toolchain-x86_64-unknown-linux-gnu.tar.gz" ]; then
					echo "Extracting rust toolchain manually..."
					tar -xzf rust-toolchain-x86_64-unknown-linux-gnu.tar.gz
					if [ -f "$HOME/.zisk/bin/rustc" ]; then
						echo "Rust toolchain extracted successfully"
					else
						echo "Failed to extract rust toolchain"
						exit 1
					fi
				else
					echo "Rust toolchain archive not found"
					exit 1
				fi
			fi
			
			# Check if CUDA is available and rebuild with GPU support for new installations
			if command -v nvcc >/dev/null 2>&1; then
				echo "CUDA toolkit detected, building Zisk with GPU support for optimal performance..."
				
				# Clone and build Zisk with GPU features
				TEMP_DIR=$(mktemp -d)
				cd "$TEMP_DIR"
				git clone https://github.com/0xPolygonHermez/zisk.git zisk-gpu-build
				cd zisk-gpu-build
				
				echo "Building Zisk with GPU features (this may take a few minutes)..."
				if cargo build --release --features gpu; then
					# Replace binaries with GPU-enabled versions
					cp target/release/cargo-zisk "$HOME/.zisk/bin/"
					cp target/release/ziskemu "$HOME/.zisk/bin/"
					cp target/release/libzisk_witness.so "$HOME/.zisk/bin/"
					cp target/release/libziskclib.a "$HOME/.zisk/bin/"
					
					# Mark as GPU-enabled
					touch "$HOME/.zisk/.gpu-enabled"
					echo "Zisk successfully built with GPU support!"
				else
					echo "GPU build failed, continuing with prebuilt binaries"
				fi
				
				# Cleanup
				cd /
				rm -rf "$TEMP_DIR"
			fi
		else
			echo "Failed to install Zisk. Please install manually:"
			echo "curl https://raw.githubusercontent.com/0xPolygonHermez/zisk/main/ziskup/install.sh | bash"
			exit 1
		fi
	fi
fi
