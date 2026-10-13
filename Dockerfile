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
ARG BORINGTUN_VERSION=0.7.1
ARG BORINGTUN_SHA256=15dd6a8a89cbe8997f37ca0cf035e6ea4d64cd2ecea4aed83ffb9f99f7126939
ARG BORINGTUN_CLI_SHA256=5957811aef42617eab3d286faf823ad83a66573a45136c5bca276d674141fcc5

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

# Userspace WireGuard for kernels without the wireguard module. boringtun always
# opens an IPv6 socket and stops on kernels without IPv6 (ipv6.disable=1); the
# small patch below lets it carry on with IPv4 there. Both crates are checked
# against their crates.io checksums; dependencies are pinned by boringtun-cli's
# own lock file.
FROM rust-base AS boringtun
ARG BORINGTUN_VERSION
ARG BORINGTUN_SHA256
ARG BORINGTUN_CLI_SHA256
ARG TARGETARCH
WORKDIR /build
COPY <<'PATCH' /build/boringtun-ipv4-only.patch
--- a/src/device/mod.rs
+++ b/src/device/mod.rs
@@ -436,10 +436,17 @@
             port = udp_sock4.local_addr()?.as_socket().unwrap().port();
         }

-        let udp_sock6 = socket2::Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP))?;
-        udp_sock6.set_reuse_address(true)?;
-        udp_sock6.bind(&SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, port, 0, 0).into())?;
-        udp_sock6.set_nonblocking(true)?;
+        let udp_sock6 = match socket2::Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP)) {
+            Ok(sock) => {
+                sock.set_reuse_address(true)?;
+                sock.bind(&SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, port, 0, 0).into())?;
+                sock.set_nonblocking(true)?;
+                sock
+            }
+            // Kernel without IPv6 (ipv6.disable=1): carry on with IPv4 only.
+            Err(e) if e.raw_os_error() == Some(libc::EAFNOSUPPORT) => udp_sock4.try_clone()?,
+            Err(e) => return Err(e.into()),
+        };

         self.register_udp_handler(udp_sock4.try_clone().unwrap())?;
         self.register_udp_handler(udp_sock6.try_clone().unwrap())?;
PATCH
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/cache/boringtun-target,id=boringtun-target-${TARGETARCH},sharing=locked \
    --mount=type=secret,id=build_ca,required=false \
    set -eu; \
    if [ -s /run/secrets/build_ca ]; then \
        export CARGO_HTTP_CAINFO=/run/secrets/build_ca CURL_CA_BUNDLE=/run/secrets/build_ca; \
    fi; \
    v="${BORINGTUN_VERSION}"; \
    curl -fsSL -o boringtun.crate "https://static.crates.io/crates/boringtun/boringtun-${v}.crate"; \
    curl -fsSL -o boringtun-cli.crate "https://static.crates.io/crates/boringtun-cli/boringtun-cli-${v}.crate"; \
    printf '%s  %s\n' "${BORINGTUN_SHA256}" boringtun.crate "${BORINGTUN_CLI_SHA256}" boringtun-cli.crate \
        > SHA256SUMS; \
    sha256sum -c SHA256SUMS; \
    tar xzf boringtun.crate; \
    tar xzf boringtun-cli.crate; \
    patch -d "boringtun-${v}" -p1 < boringtun-ipv4-only.patch; \
    printf '\n[patch.crates-io]\nboringtun = { path = "../boringtun-%s" }\n' "${v}" \
        >> "boringtun-cli-${v}/Cargo.toml"; \
    cargo build --release --manifest-path "boringtun-cli-${v}/Cargo.toml" \
        --target-dir /cache/boringtun-target; \
    install -D -m 0755 /cache/boringtun-target/release/boringtun-cli /out/bin/boringtun-cli; \
    /out/bin/boringtun-cli --version

# The popcornstash binary, with web/dist embedded.
FROM rust-base AS rust
ARG TARGETARCH
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY vendor ./vendor
COPY --from=web /src/web/dist ./web/dist
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/src/target,id=popcornstash-target-${TARGETARCH},sharing=locked \
    --mount=type=secret,id=build_ca,required=false \
    test -f web/dist/index.html \
    && if [ -s /run/secrets/build_ca ]; then export CARGO_HTTP_CAINFO=/run/secrets/build_ca; fi \
    && cargo build --release -p popcornstash --locked \
    && install -D -m 0755 target/release/popcornstash /out/popcornstash

# --------------------------------------------------------------- runtime ----
FROM ${REGISTRY}/debian:trixie-slim AS runtime
LABEL org.opencontainers.image.title="PopcornStash" \
      org.opencontainers.image.description="A private, organized home for your family's videos and photos" \
      org.opencontainers.image.source="https://github.com/thekozugroup/PopcornStash" \
      org.opencontainers.image.licenses="AGPL-3.0-or-later"
# VA-API drivers make STASH_HWACCEL=vaapi/qsv usable (AMD everywhere; Intel on amd64).
# hadolint ignore=SC2086
RUN set -eu; \
    hwaccel="mesa-va-drivers"; \
    if [ "$(dpkg --print-architecture)" = amd64 ]; then \
        hwaccel="$hwaccel intel-media-va-driver libmfx-gen1.2"; \
    fi; \
    apt-get update; \
    apt-get install -y --no-install-recommends \
        ffmpeg libvips-tools $hwaccel \
        wireguard-tools openvpn nftables iproute2 \
        ca-certificates tini libssl3t64 curl; \
    rm -rf /var/lib/apt/lists/*; \
    mkdir -p /config /stash /run/popcornstash
# libvips hands camera RAW files to ImageMagick (libraw). Debian ships ImageMagick's
# "open" policy; library files can come from strangers (torrents), so every other
# coder, all delegate programs and filters, and "@file" indirection are turned off.
# (The web server also runs libvips with its untrusted loaders blocked for every
# other photo.)
COPY <<'POLICY' /usr/share/popcornstash/imagemagick-policy.xml
<?xml version="1.0" encoding="UTF-8"?>
<policymap>
  <policy domain="Undefined" rights="none"/>
  <policy domain="resource" name="memory" value="1GiB"/>
  <policy domain="resource" name="map" value="2GiB"/>
  <policy domain="resource" name="width" value="32KP"/>
  <policy domain="resource" name="height" value="32KP"/>
  <policy domain="resource" name="area" value="256MP"/>
  <policy domain="resource" name="disk" value="2GiB"/>
  <policy domain="resource" name="time" value="120"/>
  <policy domain="resource" name="list-length" value="32"/>
  <policy domain="delegate" rights="none" pattern="*"/>
  <policy domain="filter" rights="none" pattern="*"/>
  <policy domain="path" rights="none" pattern="@*"/>
  <policy domain="coder" rights="none" pattern="*"/>
  <policy domain="coder" rights="read" pattern="{3FR,ARW,CR2,CR3,CRW,DCR,DNG,ERF,K25,KDC,MEF,MOS,MRW,NEF,NRW,ORF,PEF,RAF,RAW,RMF,RW2,RWL,SR2,SRF,SRW,STI,X3F,JPEG,JPG,TIFF,TIF}"/>
</policymap>
POLICY
RUN set -eu; \
    found=0; \
    for dir in /etc/ImageMagick-*; do \
        [ -d "$dir" ] || continue; \
        cp /usr/share/popcornstash/imagemagick-policy.xml "$dir/policy.xml"; \
        found=1; \
    done; \
    test "$found" = 1
COPY --from=boringtun /out/bin/boringtun-cli /usr/local/bin/boringtun-cli
COPY --from=rust /out/popcornstash /usr/local/bin/popcornstash
COPY LICENSE /usr/share/doc/popcornstash/LICENSE
ENV STASH_ROOT=/stash \
    STASH_CONFIG=/config
EXPOSE 8788
# The container starts as root on purpose: `popcornstash run` is a small supervisor
# that sets up the VPN and kill switch, then runs the web server as PUID:PGID and
# the torrent engine as its own user, both without capabilities.
HEALTHCHECK --interval=30s --timeout=5s --start-period=60s --start-interval=2s --retries=3 \
    CMD ["popcornstash", "healthcheck"]
ENTRYPOINT ["/usr/bin/tini", "--", "popcornstash"]
CMD ["run"]
