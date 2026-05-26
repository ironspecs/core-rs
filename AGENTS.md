# Core Rust Foundation Repository Guidance

This repository is a Rust monorepo for stable, reusable foundation crates.

## Repository intent

- Keep shared crates reusable across multiple repos.
- Keep low-level utility pushed downward into testable libraries.
- Keep business and product decisions out of this repo unless the crate truly
  owns them.
- Prefer low-churn boundaries. This repo is intended to be harder to edit
  casually than app-local workspaces.

## Ownership boundaries

- `collections` owns generic collection behavior and metrics helpers.
- `ui-icons` owns shared icon identifiers and icon asset metadata.
- `ui-dom` owns the DOM, layout, paint, text, and Bevy/WGPU integration that
  belong to the shared UI runtime boundary.

Do not move app workflows, operator logic, or product orchestration into this
repo.

## Required docs to keep current

Each Rust module directory in `crates/` must have a `README.md` that explains:

- the module purpose and scope
- the module ownership boundaries
- the module architectural approach
- the invariants, rules, and decisions shared across multiple files

Each Rust `.rs` file must begin with a file-level comment that explains:

- the file purpose
- what the file owns
- the local invariants and rules for that file

If a task changes crate layout, ownership boundaries, build commands, or shared
asset ownership, update the affected `README.md` files in the same change.

## Rust quality expectations

`sccache` is required in this monorepo and is enforced in `.cargo/config.toml`.

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace`

Do not commit with `--no-verify` unless the user explicitly asks for a
checkpoint commit.

For changed public APIs:

- add doc comments
- add tests
- add fixture examples when the crate already uses fixtures

## Design rules

All business decisions should be pushed forward and upward into consuming
codebases. Shared utility should be pushed downward into the smallest honest
crate that can own it.

All functions must have cyclomatic and cognitive complexity less than 20.

When there is a choice, prefer the least surprising behavior.

All code must fail fast and be strict with assertions.

All functions must do exactly what their names promise, never more or less.

Never return graceful empty fallback values unless the function is explicitly
named `*_safe` or `try_*` and that weaker contract is the real contract.

Respect the Law of Demeter. Prefer context objects over long parameter lists.

Prefer maps of functions over long switch or `if`/`else` chains when that keeps
decision ownership clearer.

## Panic and boundary policy

In production code, `.expect()`, `.unwrap()`, `assert!`, and `debug_assert!`
may only be used for self-owned structural invariants that indicate internal
corruption.

Do not use panic-backed validation on boundary data such as:

- deserialized payloads
- file contents
- network input
- host-provided timestamps
- asset metadata owned outside the current function

Prefer typed errors or explicit boundary rejection.

## Workspace scope rules

Add a crate here only if it is:

- reused or clearly intended to be reused across repos
- structurally stable enough to justify a stricter home
- owned at a foundation boundary rather than an app boundary

Do not add:

- binaries
- app shells
- repo-local experiments
- crates whose main value is product orchestration

## Handoff checklist

Before finishing a task:

1. Explain what changed.
2. List commands that were run.
3. List anything not verified.
4. Point to updated docs.
5. State whether repository invariants still hold.
