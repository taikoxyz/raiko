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
RUN cargo build --release ${BUILD_FLAGS}

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
        sudo \
        tree && \
    # '|| true' is used as a workaround because the installation script incorrectly assumes that the
    # systemd is available to restart the PCCS service. We will need to start it manually instead.
    # TODO TODO TODO sgx-dcap-pccs is not installed correctly (it differs with the version installed on the host machine) - run install.sh to complete installation with npm
    apt-get install -y sgx-dcap-pccs || true && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

RUN mkdir /opt/intel/sgx-dcap-pccs/ssl_key
# RUN cd /opt/intel/sgx-dcap-pccs && \
#     openssl genrsa -out ssl_key/private.pem 2048 && \
#     openssl req -new -key ssl_key/private.pem -out ssl_key/csr.pem && \
#     openssl x509 -req -days 365 -in ssl_key/csr.pem -signkey ssl_key/private.pem -out ssl_key/file.crt
RUN sed -i 's/#default quoting type = ecdsa_256/default quoting type = ecdsa_256/' /etc/aesmd.conf
# RUN /restart_aesm.sh
RUN sed -i 's/,"use_secure_cert": true/,"use_secure_cert": false/' /etc/sgx_default_qcnl.conf

RUN mkdir -p \
    ./bin \
    ./guests/sgx \
    /tmp/sgx \
    /var/log/raiko

COPY --from=builder /opt/raiko/docker/entrypoint.sh ./bin/
# We could alternatively execute the install.sh script, but it requires interactive input.
# Refer to https://github.com/intel/SGXDataCenterAttestationPrimitives/blob/master/QuoteGeneration/pccs/install.sh
COPY --from=builder /opt/raiko/raiko-guest/config/default.json /opt/intel/sgx-dcap-pccs/config/default.json
COPY --from=builder /opt/raiko/raiko-guest/config/csr.pem /opt/intel/sgx-dcap-pccs/ssl_key/csr.pem
COPY --from=builder /opt/raiko/raiko-guest/config/file.crt /opt/intel/sgx-dcap-pccs/ssl_key/file.crt
COPY --from=builder /opt/raiko/raiko-guest/config/private.pem /opt/intel/sgx-dcap-pccs/ssl_key/private.pem
COPY --from=builder /opt/raiko/raiko-guest/config/raiko-guest.manifest.template ./guests/sgx/
COPY --from=builder /opt/raiko/raiko-host/config/config.toml /etc/raiko/
COPY --from=builder /opt/raiko/target/release/raiko-guest ./guests/sgx/
COPY --from=builder /opt/raiko/target/release/raiko-host ./bin/
COPY ./sgx-ra/src/*.so /usr/lib/

ARG EDMM=0
ENV EDMM=${EDMM}
RUN cd ./guests/sgx && \
    gramine-manifest -Dlog_level=error -Darch_libdir=/lib/x86_64-linux-gnu/ raiko-guest.manifest.template raiko-guest.manifest

ENTRYPOINT [ "/opt/raiko/bin/entrypoint.sh" ]
