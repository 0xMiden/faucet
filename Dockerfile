# Build stage
FROM rust:1.96-bookworm AS builder

WORKDIR /app

# System deps for Rust builds + Node.js for frontend bundling.
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    pkg-config \
    libssl-dev \
    cmake \
    && curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*

# Copy source
COPY . .

# Build release binary (this will also build+embed the frontend via build.rs)
RUN cargo build --release -p miden-faucet

# Runtime stage
FROM debian:bookworm-slim

# Runtime deps (TLS roots + OpenSSL for rustls/native-tls stacks).
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/miden-faucet /usr/local/bin/miden-faucet
COPY docker/entrypoint.sh /usr/local/bin/entrypoint.sh

RUN chmod +x /usr/local/bin/entrypoint.sh

LABEL org.opencontainers.image.source=https://github.com/0xMiden/faucet \
    org.opencontainers.image.documentation=https://github.com/0xMiden/faucet \
    org.opencontainers.image.vendor=Miden \
    org.opencontainers.image.licenses=MIT

ARG CREATED
ARG VERSION
ARG COMMIT
LABEL org.opencontainers.image.created=$CREATED \
    org.opencontainers.image.version=$VERSION \
    org.opencontainers.image.revision=$COMMIT

EXPOSE 8000 8080

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
CMD ["start"]
