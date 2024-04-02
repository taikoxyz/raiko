FROM rust:1.75.0 as builder

ENV DEBIAN_FRONTEND=noninteractive
ARG BUILD_FLAGS=""
WORKDIR /opt/raiko
COPY . .
RUN apt-get update && \
    apt-get install -y \
    cmake \
    libclang-dev && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

# risc0 dependencies

RUN curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash
RUN cargo binstall -y --force cargo-risczero
RUN cargo risczero install

RUN cargo build --release ${BUILD_FLAGS}
RUN ls -la /opt/raiko
RUN ls -la /opt/raiko/target
RUN ls -la /opt/raiko/target/release
RUN ls -la /opt/raiko/target/release/host


FROM gramineproject/gramine:1.6-jammy as runtime
ENV DEBIAN_FRONTEND=noninteractive
WORKDIR /opt/raiko

RUN curl -o setup.sh -sL https://deb.nodesource.com/setup_18.x && \
    chmod a+x setup.sh && \
    ./setup.sh && \
    apt-get update && \
    apt-get install -y \
        cracklib-runtime \
        libsgx-dcap-default-qpl \
        libsgx-dcap-ql \
        libsgx-urts \
        sgx-pck-id-retrieval-tool \
        sudo && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

RUN sed -i 's/#default quoting type = ecdsa_256/default quoting type = ecdsa_256/' /etc/aesmd.conf
RUN sed -i 's/,"use_secure_cert": true/,"use_secure_cert": false/' /etc/sgx_default_qcnl.conf
RUN sed -i 's/https:\/\/localhost:8081/https:\/\/pccs:8081/g' /etc/sgx_default_qcnl.conf

RUN mkdir -p \
    ./bin \
    ./provers/sgx \
    /tmp/sgx \
    /var/log/raiko

COPY --from=builder /opt/raiko/docker/entrypoint.sh ./bin/
COPY --from=builder /opt/raiko/provers/sgx/config/raiko-guest.manifest.template ./provers/sgx/
COPY --from=builder /opt/raiko/host/config/config.toml /etc/raiko/
RUN ls -la /opt/raiko
RUN ls -la /opt/raiko/bin
RUN ls -la /opt/raiko/provers
RUN ls -la /opt/raiko/provers/sgx
COPY --from=builder /opt/raiko/target/release/provers ./provers/sgx/
# ubuntu@VM-0-6-ubuntu:~/zeth-john/provers/sgx$ cargo build --release
# GROTH16_VERIFIER_ADDRESS="" cargo build --features "sgx" --release
# ./provers/sp1/target
# ./provers/risc0/guest/target
# /target/release/host
# "/opt/raiko/target/release/provers": not found

# COPY --from=builder /opt/raiko/target/release/host ./bin/

# ARG EDMM=0
# ENV EDMM=${EDMM}
# RUN cd ./provers/sgx && \
#     gramine-manifest -Dlog_level=error -Darch_libdir=/lib/x86_64-linux-gnu/ raiko-guest.manifest.template raiko-guest.manifest

# ENTRYPOINT [ "/opt/raiko/bin/entrypoint.sh" ]
ENTRYPOINT [ "/bin/bash" ]
