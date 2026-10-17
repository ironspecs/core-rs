# core-rs Linux builder — pinned toolchain for the PR gate.
#
# One job: give `cargo fmt/clippy/test/xtask lint` a hermetic, reproducible
# home. This repo enforces sccache as the rustc wrapper via .cargo/config.toml,
# so the image bakes the sccache static binary; everything else the gate needs
# (workspace crates, vendored fonts, shaders) is already in the checkout.
#
# No musl targets, no sops/age, no extra system packages beyond `lld`
# (installed below for the gate's heavy link steps): core-rs builds pure-Rust
# dependency trees (bevy without winit, wgpu, cosmic-text) that otherwise need
# nothing beyond the official rust image's base toolchain.
#
# Cache mounts (durable across PR runs on the runner):
#   /cache/sccache   SCCACHE_DIR
#   /cache/cargo     CARGO_HOME

ARG RUST_VERSION=1.97.1

FROM rust:${RUST_VERSION}-bookworm

ARG SCCACHE_VERSION=v0.10.0

# The lint gate needs clippy + rustfmt, which the official image does NOT ship
# preinstalled — add them explicitly (verified: `rustup component add` required).
RUN rustup component add clippy rustfmt \
    && rustc --version \
    && rustfmt --version \
    && cargo clippy --version

# lld — use the LLVM linker for the gate's heavy link steps. Linking bevy-heavy
# test binaries with the default bfd/gold linker OOMs the memory-constrained CI
# VM (`collect2: ld terminated with signal 9`); lld links in a fraction of the
# memory and faster. RUSTFLAGS below points the toolchain at it.
RUN apt-get update \
    && apt-get install -y --no-install-recommends lld \
    && rm -rf /var/lib/apt/lists/*

# sccache — static musl binary (same approach as services-rs/docker/builder).
RUN arch="$(uname -m)" \
    && case "${arch}" in \
         x86_64)  sc_arch=x86_64 ;; \
         aarch64) sc_arch=aarch64 ;; \
         *) echo "unsupported arch: ${arch}" >&2; exit 1 ;; \
       esac \
    && curl -fsSL -o /tmp/sccache.tgz \
         "https://github.com/mozilla/sccache/releases/download/${SCCACHE_VERSION}/sccache-${SCCACHE_VERSION}-${sc_arch}-unknown-linux-musl.tar.gz" \
    && tar -xzf /tmp/sccache.tgz -C /tmp \
    && mv "/tmp/sccache-${SCCACHE_VERSION}-${sc_arch}-unknown-linux-musl/sccache" /usr/local/bin/sccache \
    && chmod +x /usr/local/bin/sccache \
    && rm -rf /tmp/sccache.tgz "/tmp/sccache-${SCCACHE_VERSION}-${sc_arch}-unknown-linux-musl" \
    && sccache --version

# .cargo/config.toml already sets rustc-wrapper = "sccache"; these keep the
# wrapper's cache and the cargo registry off the ephemeral container layer.
ENV SCCACHE_DIR=/cache/sccache \
    CARGO_HOME=/cache/cargo \
    CARGO_INCREMENTAL=0 \
    CARGO_TERM_COLOR=never \
    RUSTFLAGS="-Dwarnings -C link-args=-fuse-ld=lld"

RUN mkdir -p /cache/cargo /cache/sccache

WORKDIR /app
