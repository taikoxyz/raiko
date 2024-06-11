FROM ubuntu:latest AS builder
WORKDIR /root/raiko/
ARG ARCH

COPY .. .
RUN apt-get update && apt-get install gcc curl -y

# Get Rust
RUN curl https://sh.rustup.rs -sSf | bash -s -- -y

ENV PATH="/root/.cargo/bin:${PATH}"
RUN rustup target install x86_64-unknown-linux-musl

# Build the Rust application
RUN cargo build --release --target=x86_64-unknown-linux-musl --features nitro

# Set up runtime
FROM debian:bookworm-slim AS runner
WORKDIR /raiko

# Copy the Rust compiled binary from previous image
COPY --from=builder /root/raiko/target/x86_64-unknown-linux-musl/release/raiko-host .

CMD ["./raiko-host"]
