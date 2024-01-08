#
# Docker layer for building Gramine
# from https://github.com/gramineproject/gramine/blob/master/packaging/docker/Dockerfile
#


# Build Stage
FROM rust:latest as builder
WORKDIR /opt/raiko
COPY . .
RUN apt-get update && apt-get install -y cmake \
    libclang-dev
RUN cargo build --release

ARG UBUNTU_IMAGE=ubuntu:22.04

FROM ${UBUNTU_IMAGE}

ARG UBUNTU_CODENAME=jammy
WORKDIR /opt/raiko

ENV IP_NUMBER=0.0.0.0
ENV PORT_NUMBER=9090
ENV SGX_INSTANCE_ID=123
ENV LOG_PATH=/data/log/sgx

RUN apt-get update && \
    DEBIAN_FRONTEND=noninteractive apt-get install -y curl gnupg2 binutils

RUN curl -fsSLo /usr/share/keyrings/gramine-keyring.gpg https://packages.gramineproject.io/gramine-keyring.gpg && \
    echo 'deb [arch=amd64 signed-by=/usr/share/keyrings/gramine-keyring.gpg] https://packages.gramineproject.io/ '${UBUNTU_CODENAME}' main' > /etc/apt/sources.list.d/gramine.list

RUN curl -fsSLo /usr/share/keyrings/intel-sgx-deb.key https://download.01.org/intel-sgx/sgx_repo/ubuntu/intel-sgx-deb.key && \
    echo 'deb [arch=amd64 signed-by=/usr/share/keyrings/intel-sgx-deb.key] https://download.01.org/intel-sgx/sgx_repo/ubuntu '${UBUNTU_CODENAME}' main' > /etc/apt/sources.list.d/intel-sgx.list

RUN apt-get update && \
    DEBIAN_FRONTEND=noninteractive apt-get install -y \
    gramine \
    libsgx-aesm-ecdsa-plugin \
    libsgx-aesm-epid-plugin \
    libsgx-aesm-launch-plugin \
    libsgx-aesm-quote-ex-plugin \
    libsgx-dcap-quote-verify \
    libssl-dev \
    pkg-config \
    psmisc \
    sgx-aesm-service \
    sudo && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

RUN mkdir -p \
    ./raiko-host/guests/sgx/secrets \
    /tmp/sgx \
    /var/run/aesmd/

COPY docker/restart_aesm.sh /restart_aesm.sh
COPY --from=builder /opt/raiko/target/release/raiko-guest ./raiko-host/guests/sgx/
COPY --from=builder /opt/raiko/raiko-guest/config/raiko-guest.manifest.template ./raiko-host/guests/sgx/
COPY --from=builder /opt/raiko/target/release/raiko-host ./raiko-host/
COPY ./sgx-ra/src/*.so /usr/lib/

RUN cd ./raiko-host/guests/sgx && \
    gramine-manifest -Dlog_level=error -Darch_libdir=/lib/x86_64-linux-gnu/ raiko-guest.manifest.template raiko-guest.manifest && \
    gramine-sgx-gen-private-key && \
    gramine-sgx-sign --manifest raiko-guest.manifest --output raiko-guest.manifest.sgx && \
    cd -

RUN ls -l /root/.config/gramine/

CMD /restart_aesm.sh && \
    cd raiko-host/guests/sgx && \
    gramine-sgx ./raiko-guest bootstrap && \
    cd - && \
    RUST_LOG=debug /opt/raiko/raiko-host/raiko-host --sgx-instance-id=${SGX_INSTANCE_ID} --bind=${BIND} --log-path=${LOG_PATH}
