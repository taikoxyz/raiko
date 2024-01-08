#
# Docker layer for building Gramine
# from https://github.com/gramineproject/gramine/blob/master/packaging/docker/Dockerfile
#


# Build Stage
FROM rust:1.75.0 as builder
ARG BUILD_FLAGS=""
WORKDIR /opt/raiko
COPY . .
RUN apt-get update && apt-get install -y cmake \
    libclang-dev
RUN cargo build --release ${BUILD_FLAGS}

FROM gramineproject/gramine:1.6-focal as runtime
WORKDIR /opt/raiko

RUN mkdir -p \
    ./guests/sgx \
    ./secrets \
    ./bin \
    /tmp/sgx \
    /data/log/sgx

COPY --from=builder /opt/raiko/target/release/raiko-guest ./guests/sgx/
COPY --from=builder /opt/raiko/raiko-guest/config/raiko-guest.manifest.template ./guests/sgx/
COPY --from=builder /opt/raiko/target/release/raiko-host ./bin
COPY ./sgx-ra/src/*.so /usr/lib/

RUN cd ./guests/sgx && \
    gramine-manifest -Dlog_level=error -Darch_libdir=/lib/x86_64-linux-gnu/ raiko-guest.manifest.template raiko-guest.manifest && \
    gramine-sgx-gen-private-key && \
    gramine-sgx-sign --manifest raiko-guest.manifest --output raiko-guest.manifest.sgx && \
    cd -

ENTRYPOINT [ "/opt/raiko/bin/raiko-host" ]