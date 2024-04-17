FROM rust:1.75.0 as builder

ENV DEBIAN_FRONTEND=noninteractive
ARG BUILD_FLAGS=""
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

WORKDIR /opt/raiko
COPY . .
RUN cargo build --release ${BUILD_FLAGS} --features "sgx"


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
RUN sed -i 's/https:\/\/localhost:8081/https:\/\/host.docker.internal:8081/g' /etc/sgx_default_qcnl.conf

RUN mkdir -p \
    ./bin \
    ./provers/sgx \
    /var/log/raiko

COPY --from=builder /opt/raiko/docker/entrypoint.sh ./bin/
COPY --from=builder /opt/raiko/provers/sgx/config/sgx-guest.docker.manifest.template ./provers/sgx/config/
COPY --from=builder /opt/raiko/host/config/config.sgx.json /etc/raiko/
COPY --from=builder /opt/raiko/target/release/sgx-guest ./bin/
COPY --from=builder /opt/raiko/target/release/raiko-host ./bin/

ARG EDMM=0
ENV EDMM=${EDMM}
RUN cd ./bin && \
    gramine-sgx-gen-private-key -f && \
    gramine-manifest -Dlog_level=error -Ddirect_mode=0 -Darch_libdir=/lib/x86_64-linux-gnu/ ../provers/sgx/config/sgx-guest.docker.manifest.template sgx-guest.manifest && \
    gramine-sgx-sign --manifest sgx-guest.manifest --output sgx-guest.manifest.sgx && \
    gramine-sgx-sigstruct-view "sgx-guest.sig"

ENTRYPOINT [ "/opt/raiko/bin/entrypoint.sh" ]
