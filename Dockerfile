FROM --platform=$BUILDPLATFORM node:24-bookworm AS web-build
WORKDIR /src
RUN corepack enable
COPY web/package.json web/pnpm-lock.yaml web/pnpm-workspace.yaml ./web/
COPY apps/desktop/package.json ./apps/desktop/package.json
RUN corepack prepare pnpm@10.24.0 --activate \
  && pnpm --dir web install --frozen-lockfile
COPY web ./web
RUN pnpm --dir web build

FROM --platform=$BUILDPLATFORM rust:1.95-bookworm AS rust-build
ARG TARGETPLATFORM
ARG MIHOMO_VERSION=latest
ARG USE_PREPARED_RUNTIME_ASSETS=false
WORKDIR /src
RUN apt-get update \
  && apt-get install -y --no-install-recommends curl gzip unzip jq ca-certificates gcc-aarch64-linux-gnu binutils-aarch64-linux-gnu libc6-dev-arm64-cross \
  && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY packaging ./packaging
COPY scripts ./scripts
COPY --from=web-build /src/web/dist ./web/dist
RUN --mount=type=secret,id=github_token,required=false \
  if [ -f /run/secrets/github_token ]; then export GITHUB_TOKEN="$(cat /run/secrets/github_token)"; fi \
  && case "$TARGETPLATFORM" in \
    "linux/amd64") rustup target add x86_64-unknown-linux-gnu && bash scripts/package-core.sh --target linux-amd64 --version "$MIHOMO_VERSION" && CARGO_TARGET=x86_64-unknown-linux-gnu ;; \
    "linux/arm64") rustup target add aarch64-unknown-linux-gnu && bash scripts/package-core.sh --target linux-arm64 --version "$MIHOMO_VERSION" && CARGO_TARGET=aarch64-unknown-linux-gnu ;; \
    *) echo "unsupported TARGETPLATFORM=$TARGETPLATFORM" >&2; exit 1 ;; \
  esac \
  && if [ "$USE_PREPARED_RUNTIME_ASSETS" = "true" ]; then \
       test -s packaging/cache/rule-sets/manifest.json; \
       test "$(find packaging/cache/rule-sets -maxdepth 1 -type f -name '*.list' | wc -l)" -eq 13; \
       test -s packaging/cache/runtime/geoip.metadb; \
       test -s packaging/cache/runtime/manifest.json; \
     else \
       bash scripts/package-rule-sets.sh; \
       bash scripts/package-runtime-assets.sh; \
     fi \
  && if [ "$TARGETPLATFORM" = "linux/arm64" ]; then \
       export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc; \
       export CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc; \
       export AR_aarch64_unknown_linux_gnu=aarch64-linux-gnu-ar; \
     fi \
  && cargo build -p rweb-clash-bin --features embedded-assets --release --target "$CARGO_TARGET" \
  && cp "target/$CARGO_TARGET/release/rweb-clash-bin" /usr/local/bin/rweb-clash

FROM debian:bookworm-slim AS runtime
LABEL org.opencontainers.image.title="rweb-clash"
LABEL org.opencontainers.image.description="R-Clash Linux backend with embedded web UI and Mihomo core"
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates curl \
  && rm -rf /var/lib/apt/lists/* \
  && groupadd --system --gid 10001 rweb-clash \
  && useradd --system --uid 10001 --gid rweb-clash --home-dir /var/lib/rweb-clash --shell /usr/sbin/nologin rweb-clash \
  && install -d -o rweb-clash -g rweb-clash /var/lib/rweb-clash
COPY --from=rust-build /usr/local/bin/rweb-clash /usr/local/bin/rweb-clash
ENV RWEB_CLASH_LISTEN=0.0.0.0:31990
ENV RWEB_CLASH_ROOT=/var/lib/rweb-clash
VOLUME ["/var/lib/rweb-clash"]
EXPOSE 31990
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
  CMD curl -fsS -H "Authorization: Bearer ${RWEB_CLASH_API_TOKEN}" http://127.0.0.1:31990/api/setup/status >/dev/null || exit 1
USER 10001:10001
ENTRYPOINT ["/usr/local/bin/rweb-clash"]
