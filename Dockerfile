FROM rust:1.90 as builder


RUN apt-get update && \
    apt-get install -y \
    libwebkit2gtk-4.1-dev \
    libgtk-3-dev\
    build-essential \
    curl \
    wget \
    file \
    libxdo-dev \
    libssl-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev

WORKDIR /app


# make empty dist directory to prevent error
RUN mkdir -p dist


# Copy source and build the binary
COPY src-tauri /app/src-tauri
RUN cd src-tauri && NO_STRIP=true cargo build --release

# ---

FROM debian:trixie-slim

RUN apt-get update && \
    apt-get install -y \
    libwebkit2gtk-4.1-dev \
    libgtk-3-0 \
    libxdo3 \
    libssl3 \
    libayatana-appindicator3-1 \
    librsvg2-2

WORKDIR /app

# Copy the built binary
COPY --from=builder /app/src-tauri/target/release/chiral-network /usr/local/bin/chiral-network

EXPOSE 4001 
EXPOSE 8545

ENV ENABLE_GETH=""
ENV IS_BOOTSTRAP=""
ENV SECRET=""

# Add --enable-geth if ENABLE_GETH is set to true
ENTRYPOINT ["/bin/sh", "-c", "exec /usr/local/bin/chiral-network --headless --dht-port 4001 --show-multiaddr ${ENABLE_GETH:+--enable-geth} ${IS_BOOTSTRAP:+--is-bootstrap} ${SECRET:+--secret $SECRET}"]