# syntax=docker/dockerfile:1.7
# Supply immutable registry digests selected by the release operator.
ARG RUST_IMAGE
ARG RUNTIME_IMAGE
FROM ${RUST_IMAGE} AS builder
ARG CHIRAL_POLICY_PUBLIC_KEY
ENV CHIRAL_POLICY_PUBLIC_KEY=${CHIRAL_POLICY_PUBLIC_KEY}
RUN test ${#CHIRAL_POLICY_PUBLIC_KEY} -eq 64 && test "$CHIRAL_POLICY_PUBLIC_KEY" != "0000000000000000000000000000000000000000000000000000000000000000"
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev clang cmake protobuf-compiler && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY src-tauri/ src-tauri/
RUN cargo build --locked --release --no-default-features --manifest-path src-tauri/Cargo.toml --bins

FROM ${RUNTIME_IMAGE} AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libssl3 libgomp1 curl && rm -rf /var/lib/apt/lists/* && groupadd --gid 10001 chiral && useradd --uid 10001 --gid 10001 --no-create-home chiral && mkdir -p /var/lib/chiral && chown 10001:10001 /var/lib/chiral
COPY --from=builder /build/src-tauri/target/release/chiral /usr/local/bin/chiral
USER 10001:10001
ENV XDG_DATA_HOME=/var/lib/chiral
WORKDIR /var/lib/chiral

FROM runtime AS provider
COPY --from=builder /build/src-tauri/target/release/chiral_daemon /usr/local/bin/chiral_daemon
ENTRYPOINT ["chiral_daemon"]
CMD ["--config", "/etc/chiral/provider.json", "--version-policy", "/etc/chiral/version-policy.json"]

FROM runtime AS relay
COPY --from=builder /build/src-tauri/target/release/relay_server /usr/local/bin/relay_server
ENTRYPOINT ["relay_server"]
CMD ["--config", "/etc/chiral/relay.json"]

FROM runtime AS chain-rpc
COPY --from=builder /build/src-tauri/target/release/chiral_rpc_gateway /usr/local/bin/chiral_rpc_gateway
ENTRYPOINT ["chiral_rpc_gateway"]
CMD ["--config", "/etc/chiral/rpc-gateway.json", "--journal-dir", "/var/lib/chiral", "--bearer-file", "/run/secrets/rpc.token"]

FROM runtime AS cli
ENTRYPOINT ["chiral"]
