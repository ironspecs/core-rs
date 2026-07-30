# `structural-lint`

`structural-lint` owns the mechanical enforcement of the structural
invariants shared across the organization's Rust repositories.

## Purpose

This crate exists so the structural rules below have exactly one
implementation. Product repos wire it into their own `xtask` (or equivalent)
with their own source roots; this crate owns the parsing, the checks, and
the violation model. It does not own repo policy, repo layout decisions, or
CI wiring — each repo supplies its own lint roots and decides how the lint
is invoked.

It owns:
- the four mechanical invariant checks (rules 1, 2, 3, 5) and their
  `syn`-based enforcement
- the `Violation` model and the `run_lint` / `collect_violations` entry
  points

It does not own:
- which directories a repo lints (callers pass `lint_roots`)
- vendored/third-party exclusions (callers omit those roots)
- repo-specific quality gates, hooks, or CI jobs
- rule 4 (algorithm work-patterns): that rule is architectural, and its
  enforcement lives in each crate's metrics/history contract tests, not
  in this crate

## Usage

```rust
// In a repo's xtask binary:
structural_lint::run_lint(&workspace_root, &["apps", "crates", "xtask"])?;
```

`run_lint` prints violations to stderr tagged `error[<rule>]` and returns
an error with the count when any exist. `collect_violations` returns the
same data as a `Vec<Violation>` for census and tooling use. Exit-code and
reporting policy belongs to the calling binary.

Repos without an `xtask` can self-check with a single integration test in
`tests/` that calls `collect_violations` on the workspace root and asserts
the result is empty — the lint then runs as part of `cargo test
--workspace`. Repos with an `xtask` should prefer the xtask command, which
is what `core-rs` itself does.

There are no exemptions, no grandfathered files, and no suppression
attributes. Every rule below is mechanical and absolute within the roots a
repo chooses to lint.

## 1. Behavior-type file naming

Any behavior-bearing type (a `struct` with an `impl`, an `enum` with an
`impl`, or a `trait`) must live in its own file whose name matches the type
(`snake_case`). Plain data types (no `impl`, not a trait) — including plain
Bevy marker `#[derive(Component)]` structs with no methods — may share a
file. `mod.rs`, `lib.rs`, and `main.rs` may not contain behavior-bearing
types.

**Why:** each behavior-bearing type is isolated and discoverable by
filename. You find `RuntimeSceneCameraNode` by looking for
`runtime_scene_camera_node.rs`. You assess the surface area of a type by
reading one file instead of grepping a god-module.

**Enforcement:** `syn` parses every file, finds top-level `impl` blocks
(extracting the `self_ty` name) and trait definitions (extracting the
`ident`), snake-cases each name, and asserts the filename matches.
Duplicate impl blocks on the same type in the same file produce one
violation, not N.

**Edge cases:**
- `OAuthError` -> `oauth_error.rs` (consecutive uppercase = acronym, no
  underscore between `O` and `A`).
- `ModuleId` -> `module_id.rs` (uppercase after lowercase = word
  boundary).
- Trait impls (`impl Display for Foo`) count — the `self_ty` is `Foo`.
- Generic impls (`impl<T> Foo<T>`) extract the last path segment.
- An `impl` on a foreign type still names the file: `impl From<Row> for
  ForeignType` lives in `foreign_type.rs` inside the crate that owns the
  impl.

**Bevy note:** `#[derive(Component)]` structs that only carry data and
have no `impl` are plain data types and may share a file. The moment a
type gains a method `impl`, it graduates into its own file. Plugin types
are ordinary behavior-bearing types: `impl Plugin for FooPlugin` places
`FooPlugin` in `foo_plugin.rs` like any other type.

## 2. Behavior files require unit tests

Any file containing behavior methods (functions inside an `impl` block, or
default method bodies in a trait definition) must have a `#[cfg(test)]`
module with at least one non-empty test that directly enforces the file's
contract.

**Why:** behavior — logic with control flow, branching, state
transitions — must be provable by reading one file. The tests in that
file are the proof. If the file has behavior methods but no tests, the
contract is unenforced. This also catches simple constructors:
`impl Foo { pub fn new() -> Self { ... } }` needs a test that asserts
`new()` produces the expected state.

**Enforcement:** `syn` checks whether any `ImplItem::Fn` exists in an
`impl` block, or any `TraitItem::Fn` with a default body exists in a
trait definition. If yes and no `#[cfg(test)]` module with at least one
non-empty test function is found, violation. Empty or placeholder test
functions do not satisfy the rule.

**Bevy note:** pure functions extracted from Bevy systems are behavior
and must be tested in their own file. Bevy systems themselves
(`fn foo(mut query: Query<...>)`) are wiring, not behavior — prefer to
extract the testable pure core into a behavior file and keep the system
thin. A plugin's `build` method is behavior: the plugin's file carries a
test that builds an `App`, adds the plugin, and asserts its registration
effects.

## 3. Test isolation — tests only test their own file

An inline `#[cfg(test)]` module proves the contract of the file that
contains it. Its imports are `use super::*` plus external crates —
nothing else. `use super::*` already sees everything the file
legitimately depends on: the file's own items *and* the file's own
imports (a child module sees its parent's private `use` bindings).

**Forbidden in inline test modules** — in `use` trees and in expression,
type, and pattern paths alike:

- `crate::<other_module>::...`
- `super::super::...` — any climb past the immediate parent. A `super`
  chain that walks to the crate root and back down is a `crate::` path
  in disguise, and it silently re-roots when module depth changes.

**Why:** each file's test suite is a proof of that file's contract, not
a tangle of cross-module integration assertions. If `foo.rs` calls
`bar::do_thing()` internally, the test exercises that transitively —
that's fine. But the test must not *directly* call `bar::do_thing()` or
assert on `bar::SomeType`'s behavior. This forces dependencies to be
injected or trait-abstracted so the file under test is isolated.

**When a test needs a type the file itself does not use**, the
dependency is cross-module and has two honest homes:

1. **Declare it at file scope:** `#[cfg(test)] use
   crate::<other_module>::SomeType;`. The import is absolute, visible
   in the file header, and picked up by the test module's
   `use super::*`. Use this for one or two fixture *types* the tested
   logic collaborates with but the file does not otherwise import.
   This is not a license to launder an integration test: when a test
   module imports behavior from several modules or drives another
   module's logic, the test is not a unit test — use home 2.
2. **Move the test to `tests/`:** the crate's integration-test
   directory. Integration tests import the crate by name
   (`use ui_dom::...`) and are outside rule 3 by nature. There is no
   in-`src/` home for cross-module tests.

**Tests observe through the intended public interface only.** A moved
test asserts on public behavior and on the crate's designed
observability surface — public API plus metrics/history accessors —
never on mocked internals, and never through visibility that exists
only because a test wanted it. If a test cannot be expressed that way,
examine what it is actually asserting: the genuine unit assertion
belongs in the owning file's own inline tests, and the cross-module
assertion belongs in `tests/`. Production code is never reshaped to
accommodate a test.

**Enforcement:** `syn` parses each `#[cfg(test)]` module and walks every
`use` tree (including braced groups) and every path in expression, type,
and pattern position. Flagged: any path leading with `crate` whose
second segment is not the current file's module, and any path beginning
`super::super`. Paths inside macro invocation token streams are
unreachable and not checked. There are no exempt files.

**Allowed:**
- `use super::*` — the file's own items and its own imports.
- `use crate::<current_module>::...` — same file.
- `use serde_json::Value` — external crate (not local logic).

**Flagged:**
- `use crate::camera_controller::CameraController` in a test inside
  `scene_entities.rs` — cross-file import.
- `use super::super::super::super::media_audio::AudioBlock` — the same
  import spelled as a climb; equally flagged.

## 4. Work completes during traversal — visitor, not batch

Algorithms that walk a structure complete their work *during* the walk.
The alternative — accumulating intermediate collections on the way down
and combining them on the way back (list/batch style) — allocates per
frame, holds intermediate results alive, and changes the structure's
access pattern. A visitor-style pass walks the structure and, at each
node, does its work immediately: no return value to accumulate, no
list-building on the way back, no second fetch of a node already
visited.

This rule is about the algorithm's *work pattern*, not its syntax.
Recursion versus iteration is an entirely different discussion and is
not what this rule governs: a recursive short-circuit search that
visits each node once and allocates nothing is visitor-style; an
iterative loop that collects a `Vec` per level and folds it afterward
is batch-style. What is forbidden is introducing batch-style
accumulation — and the extra structure accesses that come with it —
where a visitor pass does the job.

**Why not a mechanical check:** whether an algorithm is visitor- or
batch-style is a semantic property of its access and allocation
pattern, not of its shape. A syntactic proxy (for example, flagging
any self-call) misidentifies conforming algorithms and, worse,
pressures conversions that *change* the access pattern — a real
performance change that review cannot see.

**Enforcement:** performance invariants are enforced by measurement,
not by syntax. Crates pin the access pattern of performance-sensitive
operations with metrics/history contract tests (see
`crates/ui-dom/tests/perf_metric_contract.rs`: scenario through the
public API, then exact assertion of metric names, counts, and
node-access totals). Any change to an algorithm's access pattern moves
those pins — which is precisely the signal that the change is a
performance change and must be reviewed as one, in its own commit.

## 5. Definition and impl in the same file

A type's definition and all of its `impl` blocks must live in the same
file, named after the type. A type defined in one file cannot have an
`impl` block in another file within the same module directory.

**Why:** splitting a type's definition from its implementation across
sibling files defeats the purpose of one-file-per-type discoverability.
You read `camera_controller.rs` expecting to find both the struct and
its methods — not just the impl with the struct stashed in a
`types.rs`.

**Enforcement:** two-pass, scoped per crate. Pass one collects every
type definition and every `impl` block. Pass two flags any type that
has an `impl` in a file that differs from its definition file when both
files are in the same parent directory.

**Not flagged:**
- `impl` blocks on types defined in a different crate (foreign types) —
  though rule 1 still names the impl's file after the self type.
- `impl` blocks that live in a different module directory from the
  definition (legitimate cross-module conversions).

## Relationship to naming rules

These structural rules are about *file shape* (one type per file, tests
colocated, no cross-file impl). Repo-level naming conventions (honest
names, coordinate/quantity vocabulary) are complementary and owned by
each repo: a behavior-bearing type must be in its own correctly-named
file AND must be honestly named.
