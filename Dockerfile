FROM ghcr.io/edgelesssys/ego-dev:v1.7.0 AS build-gaiko
WORKDIR /opt/gaiko

# Install dependencies
COPY gaiko/go.mod .
COPY gaiko/go.sum .
RUN go mod download

# Build
COPY gaiko/ .
RUN ego-go build -o gaiko-ego ./cmd/gaiko

# Sign with our enclave config and private key
COPY gaiko/ego/enclave.json .
COPY docker/enclave-key.pem private.pem
RUN ego sign && ego bundle gaiko-ego gaiko
RUN ego uniqueid gaiko-ego 2>&1 | tee /tmp/gaiko_uniqueid.log
RUN ego signerid gaiko-ego

FROM rust:1.85.0 AS chef
RUN curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash
RUN cargo binstall -y cargo-chef wild-linker
RUN apt-get update && apt-get install -y clang
WORKDIR /opt/raiko
ENV DEBIAN_FRONTEND=noninteractive
ARG BUILD_FLAGS=""

FROM chef AS planner
COPY . .
COPY docker/cargo-config.toml .cargo/config.toml
RUN cargo chef prepare --recipe-path recipe.json

# risc0 dependencies
# RUN curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash && \
#     cargo binstall -y --force cargo-risczero && \
#     cargo risczero install

FROM chef AS builder
COPY --from=planner /opt/raiko/recipe.json recipe.json
RUN cargo chef cook --release ${BUILD_FLAGS} --features "sgx" --features "docker_build" --recipe-path recipe.json
COPY . .
COPY docker/cargo-config.toml .cargo/config.toml
RUN cargo build --release ${BUILD_FLAGS} --features "sgx" --features "docker_build"

# FROM gramineproject/gramine:1.8-jammy AS runtime
# ENV DEBIAN_FRONTEND=noninteractive
# WORKDIR /opt/raiko

# RUN apt-get update && \
#     apt-get install -y \
#     cracklib-runtime \
#     libsgx-dcap-default-qpl \
#     libsgx-dcap-ql \
#     libsgx-urts \
#     sgx-pck-id-retrieval-tool \
#     build-essential \
#     libssl-dev \
#     jq \
#     sudo && \
#     apt-get clean all && \
#     rm -rf /var/lib/apt/lists/*

# RUN sed -i 's/#default quoting type = ecdsa_256/default quoting type = ecdsa_256/' /etc/aesmd.conf && \
#     sed -i 's/,"use_secure_cert": true/,"use_secure_cert": false/' /etc/sgx_default_qcnl.conf

# use base image from us-docker.pkg.dev/evmchain/images/raiko:base
# to avoid re-setup all intel sgx dependencies, some of them are not available in repository
FROM us-docker.pkg.dev/evmchain/images/raiko:base AS runtime
ENV DEBIAN_FRONTEND=noninteractive
WORKDIR /opt/raiko

RUN mkdir -p \
    ./bin \
    ./provers/sgx \
    /var/log/raiko

COPY --from=build-gaiko /opt/gaiko/gaiko ./bin/
COPY --from=build-gaiko /tmp/gaiko_uniqueid.log /tmp/
COPY --from=builder /opt/raiko/docker/entrypoint.sh ./bin/
COPY --from=builder /opt/raiko/provers/sgx/config/sgx-guest.docker.manifest.template ./provers/sgx/config/sgx-guest.local.manifest.template
# copy to /etc/raiko, but if self register mode, the mounted one will overwrite it.
COPY --from=builder /opt/raiko/host/config/config.sgx.json /etc/raiko/config.sgx.json
COPY --from=builder /opt/raiko/host/config/config.devnet.json /etc/raiko/config.devnet.json
COPY --from=builder /opt/raiko/host/config/chain_spec_list_default.json /etc/raiko/chain_spec_list_default.json
COPY --from=builder /opt/raiko/host/config/chain_spec_list_devnet.json /etc/raiko/chain_spec_list_devnet.json
COPY --from=builder /opt/raiko/target/release/sgx-guest ./bin/
COPY --from=builder /opt/raiko/target/release/raiko-host ./bin/
COPY --from=builder /opt/raiko/target/release/raiko-setup ./bin/
COPY --from=builder /opt/raiko/docker/enclave-key.pem /root/.config/gramine/enclave-key.pem

ARG EDMM=0
ENV EDMM=${EDMM}
WORKDIR /opt/raiko/bin
RUN gramine-manifest -Dlog_level=error -Ddirect_mode=0 -Darch_libdir=/lib/x86_64-linux-gnu/ ../provers/sgx/config/sgx-guest.local.manifest.template sgx-guest.manifest && \
    gramine-sgx-sign --manifest sgx-guest.manifest --output sgx-guest.manifest.sgx && \
    gramine-sgx-sigstruct-view "sgx-guest.sig" 2>&1 | tee /tmp/sgx_sigstruct.log

# Generate or update .env file with extracted MRENCLAVE from SGX signing process
WORKDIR /opt/raiko
RUN echo "Updating .env file with extracted MRENCLAVE..." && \
    MRENCLAVE=$(grep "mr_enclave:" /tmp/sgx_sigstruct.log | grep -o '[a-fA-F0-9]\{64\}' | head -1) && \
    if [ -n "$MRENCLAVE" ] && [ ${#MRENCLAVE} -eq 64 ]; then \
        if [ ! -f ".env" ]; then \
            echo "SGX_MRENCLAVE=$MRENCLAVE" > .env; \
        else \
            if grep -q "^SGX_MRENCLAVE=" .env; then \
                sed -i "s/^SGX_MRENCLAVE=.*/SGX_MRENCLAVE=$MRENCLAVE/" .env; \
            else \
                echo "SGX_MRENCLAVE=$MRENCLAVE" >> .env; \
            fi; \
        fi && \
        echo "Updated .env file with MRENCLAVE: $MRENCLAVE"; \
    else \
        echo "Failed to extract MRENCLAVE, .env file unchanged" && \
        if [ ! -f ".env" ]; then touch .env; fi; \
    fi && \
    echo "Extracting SGXGETH uniqueid..." && \
    UNIQUEID=$(grep -o '[a-fA-F0-9]\{64\}' /tmp/gaiko_uniqueid.log | head -1) && \
    if [ -n "$UNIQUEID" ] && [ ${#UNIQUEID} -eq 64 ]; then \
        SGXGETH_MRENCLAVE="$UNIQUEID" && \
        echo "Found actual SGXGETH uniqueid: $SGXGETH_MRENCLAVE"; \
    else \
        SGXGETH_MRENCLAVE="ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff" && \
        echo "No SGXGETH uniqueid found (likely cached), using default: $SGXGETH_MRENCLAVE"; \
    fi && \
    if grep -q "^SGXGETH_MRENCLAVE=" .env; then \
        sed -i "s/^SGXGETH_MRENCLAVE=.*/SGXGETH_MRENCLAVE=$SGXGETH_MRENCLAVE/" .env; \
    else \
        echo "SGXGETH_MRENCLAVE=$SGXGETH_MRENCLAVE" >> .env; \
    fi && \
    echo "Final .env file:" && \
    cat .env && \
    cp .env bin/.env

WORKDIR /opt/raiko/bin
ENTRYPOINT [ "/opt/raiko/bin/entrypoint.sh" ]
