FROM rust:1.75.0 as builder

ENV DEBIAN_FRONTEND=noninteractive
ARG BUILD_FLAGS=""

# risc0 dependencies
# RUN curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash && \
#     cargo binstall -y --force cargo-risczero && \
#     cargo risczero install

WORKDIR /opt/raiko
COPY . .
RUN cargo build --release --features "nitro" --features "docker_build"

# Set up runtime
FROM debian:bookworm-slim AS runner
WORKDIR /raiko

# Copy the Rust compiled binary from previous image
COPY --from=builder /opt/raiko/target/release/raiko-host .

CMD ["./raiko-host"]
