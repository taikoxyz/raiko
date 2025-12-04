# Raiko V2 Dockerfile - zkVM only (RISC0 + SP1)
# No SGX support

FROM rust:1.85.0 AS base-builder

ENV DEBIAN_FRONTEND=noninteractive
ARG BUILD_FLAGS=""

RUN apt-get update && \
    apt-get install -y \
    build-essential \
    clang \
    pkg-config \
    libssl-dev \
    jq \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /opt/raiko

# Install zkVM toolchains
COPY script/install.sh script/install.sh
COPY makefile makefile

# Install RISC0 toolchain
ENV TARGET=risc0
RUN mkdir -p ~/.cargo/bin && make install

# Install SP1 toolchain
ENV TARGET=sp1
RUN make install

# =============================================================================
FROM base-builder AS builder

WORKDIR /opt/raiko

# Copy workspace files
COPY Cargo.lock Cargo.lock
COPY Cargo.toml Cargo.toml
COPY rust-toolchain rust-toolchain

# Copy raiko2 crates
COPY crates/primitives crates/primitives
COPY crates/driver crates/driver
COPY crates/provider crates/provider
COPY crates/stateless crates/stateless
COPY crates/prover crates/prover
COPY crates/engine crates/engine
COPY crates/protocol crates/protocol

# Copy raiko2 binary
COPY bin/raiko2 bin/raiko2

# Copy guest programs
COPY raiko-guests raiko-guests

# Copy required legacy crates (for dependencies)
COPY lib lib
COPY core core
COPY host host
COPY provers provers
COPY pipeline pipeline
COPY harness harness
COPY taskdb taskdb
COPY reqpool reqpool
COPY reqactor reqactor
COPY ballot ballot
COPY redis-derive redis-derive

# Copy build scripts
COPY script script

# Copy KZG settings
COPY kzg_settings_raw.bin kzg_settings_raw.bin

# Build guest programs
ENV TARGET=risc0
RUN echo "Building RISC0 guests..." && \
    ./script/build-guest.sh risc0 2>&1 | tee /tmp/risc0_build.log || true

ENV TARGET=sp1
RUN echo "Building SP1 guests..." && \
    ./script/build-guest.sh sp1 2>&1 | tee /tmp/sp1_build.log || true

# Build raiko2 binary
RUN echo "Building raiko2 binary..." && \
    cargo build --release -p raiko2 ${BUILD_FLAGS}

# =============================================================================
FROM ubuntu:22.04 AS raiko2

RUN mkdir -p \
    /opt/raiko/bin \
    /etc/raiko \
    /var/log/raiko \
    /tmp/risc0-cache

RUN apt-get update && apt-get install -y \
    ca-certificates \
    openssl \
    curl \
    jq \
    && rm -rf /var/lib/apt/lists/*

# Copy binary
COPY --from=builder /opt/raiko/target/release/raiko2 /opt/raiko/bin/

# Copy config files
COPY --from=builder /opt/raiko/host/config/chain_spec_list_default.json /etc/raiko/

# Copy environment file with image IDs (if exists)
COPY --from=builder /opt/raiko/.env* /opt/raiko/ 2>/dev/null || true

WORKDIR /opt/raiko/bin

# Default configuration
ENV RAIKO2_HOST=0.0.0.0
ENV RAIKO2_PORT=8080
ENV RUST_LOG=info

EXPOSE 8080

ENTRYPOINT ["/opt/raiko/bin/raiko2"]
CMD ["--host", "0.0.0.0", "--port", "8080"]
