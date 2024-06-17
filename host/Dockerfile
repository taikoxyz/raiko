FROM rust:1.75.0 as builder

ENV DEBIAN_FRONTEND=noninteractive
ARG BUILD_FLAGS=""

# RUN apt-get update && apt-get install -y \
#    build-essential \
#    cmake \
#    curl \
#    git \
#    libssl-dev \
#    pkg-config \
#    python3 \
#    python3-pip \
#    python3-setuptools \
#    python3-wheel \
#    ninja-build \
#    wget
# risc0 dependencies
# RUN curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash && \
#    cargo binstall -y --force cargo-risczero && \
#    cargo risczero build-toolchain && \
#    cargo risczero install

WORKDIR /opt/raiko
COPY . .
RUN cargo build --release --features "nitro" --features "docker_build"

# Set up runtime
FROM debian:bookworm-slim AS runner
WORKDIR /raiko

# Copy the Rust compiled binary from previous image
COPY --from=builder /opt/raiko/target/release/raiko-host .

CMD ["./raiko-host"]
