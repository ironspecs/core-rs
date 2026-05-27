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
