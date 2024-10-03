FROM rust:1.75.0 AS builder

ENV DEBIAN_FRONTEND=noninteractive
ARG BUILD_FLAGS=""

# risc0 dependencies
# RUN curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash && \
#     cargo binstall -y --force cargo-risczero && \
#     cargo risczero install

WORKDIR /opt/raiko
COPY . .
RUN cargo build --release ${BUILD_FLAGS} --features "sgx" --features "docker_build"

FROM gramineproject/gramine:1.7-jammy AS runtime
ENV DEBIAN_FRONTEND=noninteractive
WORKDIR /opt/raiko

RUN apt-get update && \
    apt-get install -y \
    cracklib-runtime \
    libsgx-dcap-default-qpl \
    libsgx-dcap-ql \
    libsgx-urts \
    sgx-pck-id-retrieval-tool \
    jq \
    sudo && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

RUN sed -i 's/#default quoting type = ecdsa_256/default quoting type = ecdsa_256/' /etc/aesmd.conf && \
    sed -i 's/,"use_secure_cert": true/,"use_secure_cert": false/' /etc/sgx_default_qcnl.conf

RUN mkdir -p \
    ./bin \
    ./provers/sgx \
    /var/log/raiko

COPY --from=builder /opt/raiko/docker/entrypoint.sh ./bin/
COPY --from=builder /opt/raiko/provers/sgx/config/sgx-guest.docker.manifest.template ./provers/sgx/config/sgx-guest.local.manifest.template
# copy to /etc/raiko, but if self register mode, the mounted one will overwrite it.
COPY --from=builder /opt/raiko/host/config/config.sgx.json /etc/raiko/
COPY --from=builder /opt/raiko/host/config/chain_spec_list_default.json /etc/raiko/chain_spec_list.docker.json
COPY --from=builder /opt/raiko/target/release/sgx-guest ./bin/
COPY --from=builder /opt/raiko/target/release/raiko-host ./bin/
COPY --from=builder /opt/raiko/target/release/raiko-setup ./bin/
COPY --from=builder /opt/raiko/docker/enclave-key.pem /root/.config/gramine/enclave-key.pem

ARG EDMM=0
ENV EDMM=${EDMM}
WORKDIR /opt/raiko/bin
RUN gramine-manifest -Dlog_level=error -Ddirect_mode=0 -Darch_libdir=/lib/x86_64-linux-gnu/ ../provers/sgx/config/sgx-guest.local.manifest.template sgx-guest.manifest && \
    gramine-sgx-sign --manifest sgx-guest.manifest --output sgx-guest.manifest.sgx && \
    gramine-sgx-sigstruct-view "sgx-guest.sig"

ENTRYPOINT [ "/opt/raiko/bin/entrypoint.sh" ]
