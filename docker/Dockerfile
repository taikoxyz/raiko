# # Docker layer for building Gramine
# # from https://github.com/gramineproject/gramine/blob/master/packaging/docker/Dockerfile

# ARG UBUNTU_IMAGE=ubuntu:22.04

# FROM ${UBUNTU_IMAGE} as gramine-builder

# # ARGs cannot be grouped since each FROM in a Dockerfile initiates a new build
# # stage, resulting in the loss of ARG values from earlier stages.
# ARG UBUNTU_CODENAME=jammy

# RUN apt-get update && \
#     DEBIAN_FRONTEND=noninteractive apt-get install -y curl gnupg2 binutils

# RUN curl -fsSLo /usr/share/keyrings/gramine-keyring.gpg https://packages.gramineproject.io/gramine-keyring.gpg && \
#     echo 'deb [arch=amd64 signed-by=/usr/share/keyrings/gramine-keyring.gpg] https://packages.gramineproject.io/ '${UBUNTU_CODENAME}' main' > /etc/apt/sources.list.d/gramine.list

# RUN curl -fsSLo /usr/share/keyrings/intel-sgx-deb.key https://download.01.org/intel-sgx/sgx_repo/ubuntu/intel-sgx-deb.key && \
#     echo 'deb [arch=amd64 signed-by=/usr/share/keyrings/intel-sgx-deb.key] https://download.01.org/intel-sgx/sgx_repo/ubuntu '${UBUNTU_CODENAME}' main' > /etc/apt/sources.list.d/intel-sgx.list

# RUN apt-get update && \
#     DEBIAN_FRONTEND=noninteractive apt-get install -y gramine \
#     sgx-aesm-service \
#     libsgx-aesm-launch-plugin \
#     libsgx-aesm-epid-plugin \
#     libsgx-aesm-quote-ex-plugin \
#     libsgx-aesm-ecdsa-plugin \
#     libsgx-dcap-quote-verify \
#     psmisc && \
#     apt-get clean && \
#     rm -rf /var/lib/apt/lists/*

# RUN mkdir -p /var/run/aesmd/

# COPY docker/restart_aesm.sh /restart_aesm.sh


# Docker layer for building Raiko

FROM rust:latest as raiko-builder

WORKDIR /usr/src/raiko

# The SGX RA-TLS package requires cmake
RUN apt-get update && \
    DEBIAN_FRONTEND=noninteractive apt-get install -y cmake libclang-dev && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

COPY ./sgx-ra/src/*.so /usr/lib/
COPY . .
RUN cargo run --bin raiko-host -- --sgx-instance-id=123 --bind=0.0.0.0:8081
# RUN cargo build --release


# ENTRYPOINT ["/bin/sh", "-c"]
# CMD ["/restart_aesm.sh ; exec /bin/bash"]
# RUN cargo install --path raiko-host
