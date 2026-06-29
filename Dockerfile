ARG RAIKO_RUNTIME_BASE_IMAGE=runtime-base-clean

FROM ghcr.io/edgelesssys/ego-dev:v1.8.1 AS build-gaiko
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
ARG ENCLAVE_KEY_PUBLIC_SHA256
# CI/local compose builds use a throwaway key; release builds pass a secret and hash.
RUN --mount=type=secret,id=enclave_key,target=/run/secrets/enclave-key.pem \
    set -e; \
    trap 'rm -f private.pem' EXIT; \
    if [ -s /run/secrets/enclave-key.pem ]; then \
        if [ -n "${ENCLAVE_KEY_PUBLIC_SHA256}" ]; then \
            test "$(openssl rsa -in /run/secrets/enclave-key.pem -pubout 2>/dev/null | openssl sha256 | awk '{print $2}')" = "${ENCLAVE_KEY_PUBLIC_SHA256}"; \
        fi; \
        cp /run/secrets/enclave-key.pem private.pem; \
    else \
        test -z "${ENCLAVE_KEY_PUBLIC_SHA256}"; \
        openssl genrsa -3 -out private.pem 3072; \
    fi; \
    ego sign && \
    ego bundle gaiko-ego gaiko
RUN ego uniqueid gaiko-ego 2>&1 | tee /tmp/gaiko_uniqueid.log
RUN ego signerid gaiko-ego

FROM rust:1.85.0 AS chef
RUN curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash
RUN cargo binstall -y cargo-chef
RUN cargo binstall -y wild-linker --version 0.7.0
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
FROM us-docker.pkg.dev/evmchain/images/raiko:base AS runtime-base-scrubber
RUN rm -f /root/.config/gramine/enclave-key.pem

FROM scratch AS runtime-base-clean
COPY --from=runtime-base-scrubber / /

FROM ${RAIKO_RUNTIME_BASE_IMAGE} AS runtime
ENV PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
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

ARG EDMM=0
ARG ENCLAVE_KEY_PUBLIC_SHA256
ENV EDMM=${EDMM}
WORKDIR /opt/raiko/bin
RUN --mount=type=secret,id=enclave_key,target=/run/secrets/enclave-key.pem \
    set -e; \
    enclave_key_path=/tmp/enclave-key.pem; \
    trap 'rm -f "$enclave_key_path"' EXIT; \
    if [ -s /run/secrets/enclave-key.pem ]; then \
        if [ -n "${ENCLAVE_KEY_PUBLIC_SHA256}" ]; then \
            test "$(openssl rsa -in /run/secrets/enclave-key.pem -pubout 2>/dev/null | openssl sha256 | awk '{print $2}')" = "${ENCLAVE_KEY_PUBLIC_SHA256}"; \
        fi; \
        cp /run/secrets/enclave-key.pem "$enclave_key_path"; \
    else \
        test -z "${ENCLAVE_KEY_PUBLIC_SHA256}"; \
        openssl genrsa -3 -out "$enclave_key_path" 3072; \
    fi; \
    gramine-manifest -Dlog_level=error -Ddirect_mode=0 -Darch_libdir=/lib/x86_64-linux-gnu/ ../provers/sgx/config/sgx-guest.local.manifest.template sgx-guest.manifest && \
    gramine-sgx-sign --key "$enclave_key_path" --manifest sgx-guest.manifest --output sgx-guest.manifest.sgx && \
    gramine-sgx-sigstruct-view "sgx-guest.sig" 2>&1 | tee /tmp/sgx_sigstruct.log


WORKDIR /opt/raiko/bin
ENTRYPOINT [ "/opt/raiko/bin/entrypoint.sh" ]
