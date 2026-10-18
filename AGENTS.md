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
- `structural-lint` owns the structural rules below and their `syn`-based
  enforcement. It is the single implementation; every consuming repo wires
  it into its own xtask with its own lint roots.
- `ui-icons` owns shared icon identifiers and icon asset metadata.
- `ui-dom` owns the DOM, layout, paint, text, and Bevy/WGPU integration that
  belong to the shared UI runtime boundary.
- `xtask` owns repo automation (the `lint` command). It is the only binary
  in this workspace and contains no product logic.

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

Multi-file rules belong in the module `README.md`. Single-file rules belong in
that file comment.

If a task changes process boundaries, protocols, ownership, crate layout, build
commands, or shared asset ownership, update the affected module `README.md`
files in the same change, and update Rust file comments when local file
responsibilities or invariants changed.

## Rust quality expectations

`sccache` is required in this monorepo and is enforced in `.cargo/config.toml`.

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace`
- `cargo xtask lint` (structural rules; see below)

The `Justfile` mirrors these as `just fmt`, `just clippy`, `just test`,
`just lint-xtask`, and the combined `just lint`.

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

Observability is part of the public interface. Behavior worth asserting is
asserted through public API and designed metrics/history accessors. Do not add
mocks, test-only constructors, or visibility that exists only because a test
wanted it — reshape the test, not the production code.

When behavior's output is not observable, prefer growing the metrics/history
emission surface over writing internal tests: real counts of a function's cost
and business output justify the code's existence. A test that still cannot be
expressed through the public surface after emission is moot, and functionality
with no observable output is moot — delete both rather than bury them.

`history()` assertions are ordering-only: assert that one event's position is
before or after another's (`position(a) < position(b)`), never that the
history equals an exact array. Instrumentation must be free to grow without
breaking every test; exact-array assertions make growth a breaking change.

Metrics/history emission must remain perfectly zero-allocation. Metric names
are a static, closed vocabulary fixed at compile time; counters are
enum-indexed fixed arrays; history is a fixed-capacity name-only ring. No
Vecs, no dynamic strings, no per-event buffers, no payloads. Any proposal to
add allocation to the emission path is rejected on sight: a foundation
library cannot spend its consumer's allocator budget, instrumentation must be
invisible to the cost model it measures, and zero cost is what makes the
observation surface always-on in production rather than a debug-only build
mode. Content observability is achieved with read accessors over retained
state and on-demand computed projections (see `compute_style_map`), never
with emission payloads.

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

Scoped exception (adjudicated 2026-10-17, PLAN.md proposal G): CSS
declaration parsing is drop-and-continue, because CSS 2.1 §4.1.7 and
css-style-attr §3 mandate it — a malformed declaration is dropped and parsing
continues. The exception is rule-shaped, never a graceful silent fallback:
every drop is counted by metric AND returned in an explicit report type
(`StylesheetLoadReport`, `DeclDropReport`). It applies only to
declaration-list inputs (stylesheets, `set_all`); single-declaration APIs
stay strict.

## Structural rules

This repo enforces the structural rules owned by `structural-lint`
on itself via `cargo xtask lint`. Rules 1, 2, 3, and 5 are mechanical
and absolute within `crates/`:

1. **Behavior-type file naming** — a type with an `impl` (or a `trait`)
   lives in its own `snake_case` file; `mod.rs`/`lib.rs` hold no
   behavior-bearing types.
2. **Behavior files require unit tests** — every file with behavior
   methods has a `#[cfg(test)]` module with at least one real, non-empty
   test. Placeholder tests are not acceptable in this repo.
3. **Test isolation** — inline test modules import `use super::*` plus
   external crates only. Cross-module fixtures are declared at file scope
   as `#[cfg(test)] use crate::...;`, never inside the test module.
   Cross-module tests move to `tests/`.
4. **Visitor, not batch** — algorithms complete work during traversal;
   no batch-style accumulation with its extra accesses. This rule is
   architectural: it is enforced by metrics/history contract tests
   (e.g. `crates/ui-dom/tests/perf_metric.rs`), not by the
   lint. Any change that moves those pins is a performance change and
   is reviewed as one, in its own commit.
5. **Definition and impl in the same file** — a type's `impl` blocks live
   in the file named after the type.

The canonical rule text and rationale live in
`crates/structural-lint/README.md`. Do not weaken, suppress, or
grandfather violations in this repo.

## Structural migration policy

Applying the structural rules to pre-existing code is done in
behavior-preserving changesets:

- **Production behavior is frozen.** Moved items (types, impls, functions,
  tests) relocate byte-identically. A migration diff reads as cut/paste plus
  imports plus visibility adjustments. If a rule cannot be satisfied without
  changing behavior, stop and escalate — never improvise a semantic change
  to make a lint pass.
- **Algorithm changes are their own commits.** Any change to an
  algorithm's traversal or access pattern (rule 4) is a performance
  change: it is reviewed on its own, with the full test suite including
  the metrics/history contract tests green as the gate. If a contract
  pin moves, the code changed access patterns — that is never a side
  effect of other work. Never bundle algorithm changes into layout
  migrations.
- **Tests relocate; they are never rewritten or manufactured to fit.** A
  file's unit tests move with its types. A test that reaches across modules
  is an integration test and moves to `tests/`, where it observes through
  the crate's public interface — public API plus metrics/history accessors.
  Tests that depended on mocks or test-only visibility are examined for
  what they actually assert: genuine unit assertions re-home into the
  owning file's inline tests; cross-module assertions are re-expressed in
  `tests/` against the public interface. Production code is never reshaped
  to accommodate a test. Files left behavior-bearing without tests are
  enumerated for the coverage effort — they are not filled with
  manufactured tests during a migration.
- **The reviewer runs the gate.** Claims of green are verified by running
  fmt, clippy, the full test suite, and the lint before every merge.

## Workspace scope rules

Add a crate here only if it is:

- reused or clearly intended to be reused across repos
- structurally stable enough to justify a stricter home
- owned at a foundation boundary rather than an app boundary

Do not add:

- binaries (single exception: `xtask`, the repo-automation entry point;
  it holds no product logic)
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
