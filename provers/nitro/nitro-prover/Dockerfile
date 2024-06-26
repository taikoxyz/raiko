FROM rust:1.75.0 as builder

ENV DEBIAN_FRONTEND=noninteractive
ARG BUILD_FLAGS=""

WORKDIR /opt/raiko-nitro
COPY . .
RUN cargo build --release

# Set up runtime
FROM debian:bookworm-slim AS runner
WORKDIR /raiko-nitro

# Copy the Rust compiled binary from previous image
COPY --from=builder /opt/raiko-nitro/target/release/nitro-prover .

CMD ["/raiko-nitro/nitro-prover"]
