#
# Docker layer for building Gramine
# from https://github.com/gramineproject/gramine/blob/master/packaging/docker/Dockerfile
#


# Build Stage
FROM rust:latest as builder
ARG BUILD_FLAGS=""
WORKDIR /opt/raiko
COPY . .
RUN apt-get update && apt-get install -y cmake \
    libclang-dev
RUN cargo build --release ${BUILD_FLAGS}

FROM gramineproject/gramine:latest as runtime
WORKDIR /opt/raiko

ENV RAIKO_HOST_BIND=0.0.0.0:9090
ENV RAIKO_HOST_SGX_INSTANCE_ID=123
ENV RAIKO_HOST_LOG_PATH=/data/log/sgx

RUN mkdir -p \
    ./raiko-host/guests/sgx/secrets \
    /tmp/sgx \
    /var/run/aesmd/ \
    /data/log/sgx

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

CMD /restart_aesm.sh && \
    cd raiko-host/guests/sgx && \
    gramine-sgx ./raiko-guest bootstrap && \
    cd - && \
    /opt/raiko/raiko-host/raiko-host
