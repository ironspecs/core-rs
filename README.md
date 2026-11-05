# `core-rs`

Shared low-churn Rust crates that are intended to be reused across projects.

This workspace is the home for stable foundational crates such as:

- `collections`
- `ui-dom`
- `ui-icons`

## Purpose

`core-rs` is a cold-storage workspace for crates that should stay reusable,
strict, and intentionally harder to churn than app-local code.

The workspace is for:

- shared crates with clear ownership boundaries
- low-level UI/runtime foundations reused by multiple repos
- utility pushed downward into testable crates instead of drifting upward into
  product code

The workspace is not for:

- app binaries
- product-specific workflows
- fast-moving operator or feature code

## Operating Rules

- `sccache` is required and enforced in repo-local Cargo config.
- warnings are denied at build time.
- `unsafe` is forbidden by workspace lint policy.
- changed public APIs must ship with doc comments and tests.
- crate boundaries should stay unsurprising and low-churn.

## Quality Gates

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace`

## CI build caching (sccache L0→L1 WebDAV pilot)

The builder image pins **sccache v0.19.0** (see `docker/versions.env` and
`docker/builder/Dockerfile`); `sccache` remains the enforced `rustc-wrapper`.
The pilot adds an optional two-level cache in CI only:

- **L0** is the local disk cache (`SCCACHE_DIR`, the durable `/cache/sccache`
  mount).
- **L1** is a shared WebDAV cache, used only when the build box injects a
  healthy, authenticated WebDAV configuration as `BUILDER_WEBDAV_*`.

`docker/with-webdav.sh` wraps the gate and decides the chain at runtime. When a
complete `BUILDER_WEBDAV_*` set is present and an authenticated
`PROPFIND` (Depth: 0, 5s timeout) returns 2xx, it selects `disk,webdav` and
maps the WebDAV settings to `SCCACHE_WEBDAV_*`. Otherwise — including a
credential mismatch, a timeout, or a full outage — it selects `disk` only and
the gate proceeds exactly as before. The WebDAV credentials are never printed
and never placed in argv (curl reads the `Authorization` header from a
stdin-fed config file, not `curl --user`).

Two settings are fixed by the wrapper and only affect CI:

- `SCCACHE_MULTILEVEL_WRITE_ERROR_POLICY=l0` — only a failed L0 (disk) write
  can fail a compile; an L1 (WebDAV) write failure is best-effort and logged.
- `SCCACHE_CACHE_SIZE=6G` — the Forgejo-safe CI cache budget. The repo's
  workstation default (40G in `.cargo/config.toml`, `force = false`) is
  untouched; the environment variable simply wins in CI.

An isolated `collections` cache proof (`docker/prove_sccache_webdav.py`) runs
after the normal gate. It compiles `collections` against a throwaway L0
directory, target directory, and sccache server port, then verifies from JSON
stats that a cleared L0 is repopulated from L1 (increased L1 hits and L0
backfill without increased compilations) and that a follow-up build hits L0. It
also proves the disk-only fallback with a missing-vars control and a
closed-port control. Cargo "Fresh" output is never accepted as evidence.

The pilot changes no public registry or source consumption: builds still use
the same pinned toolchain, the same `Cargo.lock`, and the same in-tree vendored
assets. The WebDAV L1 is append-only remote cache; a cold or unreachable L1
falls back to L0 with no effect on what gets built.

## Release

`core-rs` is Tier 3 cold storage. Release from this repo only when the
workspace is intentionally stable and all three crates are meant to move
together at one version.

Release procedure for `0.1.0` and later:

1. Confirm the workspace version in `Cargo.toml`.
2. Run the quality gates.
3. Commit the release changes.
4. Create an annotated repo tag such as `v0.1.0`.
5. Push the commit and tag with `git push origin main --follow-tags`.

Consumer repos should depend on tagged git revisions from
`git@github.com:ironspecs/core-rs.git` and use consumer-side Cargo profile
overrides when they need Tier 2 dev builds with release-grade optimization for
selected `core-rs` crates.

## Layout

- `crates/` contains shared foundation crates
- `assets/` contains crate-owned shared assets and fixtures
- `.cargo/config.toml` owns local build enforcement for this workspace

See `AGENTS.md` for repository working rules.
