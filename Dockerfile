# syntax=docker/dockerfile:1.7
#
# PopcornStash: one image, one container (docs/architecture.md sections 1 and 11).
#
#   docker build -t popcornstash:dev .
#   docker build --build-arg REGISTRY=mirror.gcr.io/library -t popcornstash:dev .
#
# Behind a TLS-intercepting proxy, pass its CA as a build secret (never stored in
# a layer): docker build --secret id=build_ca,src=/path/to/ca.pem ...

ARG REGISTRY=docker.io/library
ARG NODE_VERSION=22
ARG BORINGTUN_VERSION=0.6.0

# ---------------------------------------------------------------- web UI ----
# The UI is plain static files, so it is built once on the build machine's own
# platform and reused for every target architecture.
FROM --platform=$BUILDPLATFORM ${REGISTRY}/node:${NODE_VERSION}-slim AS web
WORKDIR /src/web
COPY web/package.json web/package-lock.json ./
RUN --mount=type=cache,target=/root/.npm,sharing=locked \
    --mount=type=secret,id=build_ca,required=false \
    if [ -s /run/secrets/build_ca ]; then export NODE_EXTRA_CA_CERTS=/run/secrets/build_ca; fi; \
    npm ci --ignore-scripts --no-audit --no-fund
COPY web/ ./
RUN npm run build

# ------------------------------------------------------------ rust tools ----
FROM ${REGISTRY}/rust:1-trixie AS rust-base
RUN apt-get update \
    && apt-get install -y --no-install-recommends libssl-dev pkg-config clang \
    && rm -rf /var/lib/apt/lists/*
ENV CARGO_TERM_COLOR=never CARGO_INCREMENTAL=0

# Userspace WireGuard for kernels without the wireguard module.
FROM rust-base AS boringtun
ARG BORINGTUN_VERSION
ARG TARGETARCH
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/cache/boringtun-target,id=boringtun-target-${TARGETARCH},sharing=locked \
    --mount=type=secret,id=build_ca,required=false \
    if [ -s /run/secrets/build_ca ]; then export CARGO_HTTP_CAINFO=/run/secrets/build_ca; fi; \
    cargo install boringtun-cli --version "${BORINGTUN_VERSION}" --locked \
        --target-dir /cache/boringtun-target --root /out \
    && /out/bin/boringtun-cli --version

# The popcornstash binary, with web/dist embedded.
FROM rust-base AS rust
ARG TARGETARCH
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY --from=web /src/web/dist ./web/dist
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/src/target,id=popcornstash-target-${TARGETARCH},sharing=locked \
    --mount=type=secret,id=build_ca,required=false \
    if [ -s /run/secrets/build_ca ]; then export CARGO_HTTP_CAINFO=/run/secrets/build_ca; fi; \
    cargo build --release -p popcornstash --locked \
    && install -D -m 0755 target/release/popcornstash /out/popcornstash

# --------------------------------------------------------------- runtime ----
FROM ${REGISTRY}/debian:trixie-slim AS runtime
LABEL org.opencontainers.image.title="PopcornStash" \
      org.opencontainers.image.description="A private, organized home for your family's videos and photos" \
      org.opencontainers.image.source="https://github.com/thekozugroup/PopcornStash" \
      org.opencontainers.image.licenses="AGPL-3.0-or-later"
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ffmpeg libvips-tools \
        wireguard-tools openvpn nftables iproute2 \
        ca-certificates tini libssl3t64 curl \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /config /stash /run/popcornstash
COPY --from=boringtun /out/bin/boringtun-cli /usr/local/bin/boringtun-cli
COPY --from=rust /out/popcornstash /usr/local/bin/popcornstash
COPY LICENSE /usr/share/doc/popcornstash/LICENSE
ENV STASH_ROOT=/stash \
    STASH_CONFIG=/config
EXPOSE 8788
# The container starts as root on purpose: `popcornstash run` is a small supervisor
# that sets up the VPN and kill switch, then runs the web server as PUID:PGID and
# the torrent engine as its own user, both without capabilities.
HEALTHCHECK --interval=30s --timeout=5s --start-period=30s --retries=3 \
    CMD ["popcornstash", "healthcheck"]
ENTRYPOINT ["/usr/bin/tini", "--", "popcornstash"]
CMD ["run"]
