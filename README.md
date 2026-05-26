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

## Layout

- `crates/` contains shared foundation crates
- `assets/` contains crate-owned shared assets and fixtures
- `.cargo/config.toml` owns local build enforcement for this workspace

See `AGENTS.md` for repository working rules.
