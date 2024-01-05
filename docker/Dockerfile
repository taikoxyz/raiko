#
# Docker layer for building Gramine
# from https://github.com/gramineproject/gramine/blob/master/packaging/docker/Dockerfile
#

ARG UBUNTU_IMAGE=ubuntu:22.04

FROM ${UBUNTU_IMAGE}

ARG UBUNTU_CODENAME=jammy
WORKDIR /usr/src/raiko

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
    cmake \
    gramine \
    libclang-dev \
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

RUN curl --proto '=https' --tlsv1.3 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

COPY ./sgx-ra/src/*.so /usr/lib/
COPY . .

RUN cargo build --release && \
    cp target/release/raiko-guest raiko-host/guests/sgx && \
    cd raiko-host/guests/sgx && \
    ln -s ../../../raiko-guest/config/raiko-guest.manifest.template && \
    gramine-manifest -Dlog_level=error -Darch_libdir=/lib/x86_64-linux-gnu/ raiko-guest.manifest.template raiko-guest.manifest && \
    gramine-sgx-gen-private-key && \
    gramine-sgx-sign --manifest raiko-guest.manifest --output raiko-guest.manifest.sgx

CMD /restart_aesm.sh && \
    cd raiko-host/guests/sgx && \
    sleep 10 && \
    gramine-sgx ./raiko-guest bootstrap && \
    cd - && \
    RUST_LOG=debug cargo run --bin raiko-host -- --sgx-instance-id=${SGX_INSTANCE_ID} --bind=${IP_NUMBER}:${PORT_NUMBER} --log-path=${LOG_PATH}
