FROM ubuntu:latest AS builder
WORKDIR /root/raiko/
ARG ARCH

# Ensure the architecture is set
RUN if [ -n "${ARCH}" ]; \
    then echo "Will target architecture \"${ARCH}\"."; \
    else 
        echo "Target architecture not specified. Will use x86_64."; \
        ARCH="x86_64"; \ 
    fi

COPY . .
RUN apt-get update && apt-get install gcc curl -y

# Get Rust
RUN curl https://sh.rustup.rs -sSf | bash -s -- -y

ENV PATH="/root/.cargo/bin:${PATH}"
RUN rustup target install ${ARCH}-unknown-linux-musl

# Build the Rust application
RUN cargo build --release --target=${ARCH}-unknown-linux-musl --features nitro

# Set up runtime
FROM bookworm-slim:latest AS runner
WORKDIR /raiko

# Copy the Rust compiled binary from previous image
COPY --from=builder /root/raiko/target/${ARCH}-unknown-linux-musl/release/raiko-host .

CMD ["./raiko-host"]