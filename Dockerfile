# =============================================================================
# Chiral Network — Multi-stage Docker build
#
# Targets:
#   daemon       — headless P2P node with DHT, file transfer, Drive API
#   relay        — bootstrap relay server (P2P + HTTP gateway)
#   cli          — CLI tool for interacting with a running daemon
#   test-node    — daemon pre-configured for automated testing
# =============================================================================

# ---------------------------------------------------------------------------
# Stage 1: Build all Rust binaries
# ---------------------------------------------------------------------------
FROM rust:1.82-bookworm AS builder

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    clang \
    cmake \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Cache dependency builds — copy manifests first
COPY src-tauri/Cargo.toml src-tauri/Cargo.lock ./src-tauri/
RUN mkdir -p src-tauri/src src-tauri/src/bin && \
    echo 'fn main() {}' > src-tauri/src/bin/chiral.rs && \
    echo 'fn main() {}' > src-tauri/src/bin/chiral_daemon.rs && \
    echo 'fn main() {}' > src-tauri/src/bin/relay_server.rs && \
    echo '' > src-tauri/src/lib.rs && \
    cd src-tauri && cargo fetch

# Copy actual source
COPY src-tauri/ ./src-tauri/

# Build all binaries in release mode
RUN cd src-tauri && \
    cargo build --release --bin chiral_daemon --bin chiral --bin relay_server && \
    strip target/release/chiral_daemon target/release/chiral target/release/relay_server

# ---------------------------------------------------------------------------
# Stage 2: Minimal runtime base
# ---------------------------------------------------------------------------
FROM debian:bookworm-slim AS runtime-base

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    && rm -rf /var/lib/apt/lists/*

RUN mkdir -p /data/chiral-network

ENV XDG_DATA_HOME=/data

# ---------------------------------------------------------------------------
# Target: daemon — headless P2P node
# ---------------------------------------------------------------------------
FROM runtime-base AS daemon

COPY --from=builder /build/src-tauri/target/release/chiral_daemon /usr/local/bin/chiral_daemon

EXPOSE 9419/tcp
EXPOSE 30303/tcp
EXPOSE 30303/udp

VOLUME ["/data"]

ENTRYPOINT ["chiral_daemon"]
CMD ["--port", "9419"]

# ---------------------------------------------------------------------------
# Target: relay — bootstrap relay server
# ---------------------------------------------------------------------------
FROM runtime-base AS relay

COPY --from=builder /build/src-tauri/target/release/relay_server /usr/local/bin/relay_server

EXPOSE 4001/tcp
EXPOSE 8080/tcp

VOLUME ["/data"]

ENTRYPOINT ["relay_server"]
CMD ["--port", "4001", "--http-port", "8080"]

# ---------------------------------------------------------------------------
# Target: cli — command-line client
# ---------------------------------------------------------------------------
FROM runtime-base AS cli

COPY --from=builder /build/src-tauri/target/release/chiral /usr/local/bin/chiral

VOLUME ["/data"]

ENTRYPOINT ["chiral"]

# ---------------------------------------------------------------------------
# Target: test-node — daemon configured for automated testing
# ---------------------------------------------------------------------------
FROM runtime-base AS test-node

COPY --from=builder /build/src-tauri/target/release/chiral_daemon /usr/local/bin/chiral_daemon
COPY --from=builder /build/src-tauri/target/release/chiral /usr/local/bin/chiral

EXPOSE 9419/tcp
EXPOSE 30303/tcp
EXPOSE 30303/udp

VOLUME ["/data"]

# Healthcheck: verify the HTTP gateway is responsive
HEALTHCHECK --interval=10s --timeout=5s --retries=3 \
    CMD curl -sf http://localhost:9419/api/drive/items || exit 1

ENTRYPOINT ["chiral_daemon"]
CMD ["--port", "9419"]
