FROM rust:latest as builder

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
RUN cargo build --release ${BUILD_FLAGS}

FROM gramineproject/gramine:1.6-jammy as runtime
WORKDIR /opt/raiko

RUN apt-get update && \
    apt-get install -y sudo && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

RUN mkdir -p \
    ./bin \
    ./guests/sgx \
    /tmp/sgx \
    /var/log/raiko

COPY --from=builder /opt/raiko/docker/entrypoint.sh ./bin/
COPY --from=builder /opt/raiko/raiko-guests/sgx/config/raiko-sgx.manifest.template ./guests/sgx/
COPY --from=builder /opt/raiko/raiko-host/config/config.toml /etc/raiko/
COPY --from=builder /opt/raiko/target/release/raiko-sgx ./guests/sgx/
COPY --from=builder /opt/raiko/target/release/raiko-host ./bin/
COPY ./sgx-ra/src/*.so /usr/lib/

ARG EDMM=0
ENV EDMM=${EDMM}
RUN cd ./guests/sgx && \
    gramine-manifest -Dlog_level=error -Darch_libdir=/lib/x86_64-linux-gnu/ raiko-sgx.manifest.template raiko-sgx.manifest

ENTRYPOINT [ "/opt/raiko/bin/entrypoint.sh" ]
