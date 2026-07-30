# Justfile — host-agnostic build, test, and lint entry points.

set shell := ["bash", "-c"]

# Default: list recipes.
default:
    @just --list

# Run cargo fmt in check mode across the workspace.
fmt:
    cargo fmt --all --check

# Run clippy across the workspace with -D warnings.
clippy:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# Run tests across the whole workspace.
test:
    cargo test --workspace

# Run the xtask structural linter.
lint-xtask:
    cargo run -p xtask -- lint

# Run both linters (xtask structural lint + clippy).
lint: lint-xtask clippy
