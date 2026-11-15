# syntax=docker/dockerfile:1.7

FROM rust:1.90-bookworm AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release --locked -p corvusd -p corvus

FROM debian:bookworm-slim AS runtime
ARG CORVUS_VERSION=1.1.0
LABEL org.opencontainers.image.title="Corvus" \
      org.opencontainers.image.description="Corvus browser hub daemon" \
      org.opencontainers.image.version="$CORVUS_VERSION"

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 corvus \
    && useradd --uid 10001 --gid 10001 --home-dir /data --shell /usr/sbin/nologin --no-create-home corvus

COPY --from=builder /build/target/release/corvusd /usr/local/bin/corvusd
COPY --from=builder /build/target/release/corvus /usr/local/bin/corvus
COPY extension /opt/corvus/extension

RUN mkdir -p /data/data /data/logs /data/config \
    && chown -R 10001:10001 /data /opt/corvus

ENV RUST_LOG=info
USER 10001:10001
VOLUME ["/data"]
EXPOSE 7330 7331
STOPSIGNAL SIGTERM
HEALTHCHECK --interval=10s --timeout=3s --start-period=5s --retries=3 CMD curl -fsS http://127.0.0.1:7331/health || exit 1

ENTRYPOINT ["corvusd", "serve", "--bind", "0.0.0.0", "--home", "/data"]
