# `structural-lint`

`structural-lint` owns the mechanical enforcement of the structural
invariants shared across the organization's Rust repositories.

## Purpose

This crate exists so the five structural rules below have exactly one
implementation. Product repos wire it into their own `xtask` (or equivalent)
with their own source roots; this crate owns the parsing, the checks, and
the violation model. It does not own repo policy, repo layout decisions, or
CI wiring — each repo supplies its own lint roots and decides how the lint
is invoked.

It owns:
- the five invariant checks and their `syn`-based enforcement
- the `Violation` model and the `run_lint` / `collect_violations` entry
  points

It does not own:
- which directories a repo lints (callers pass `lint_roots`)
- vendored/third-party exclusions (callers omit those roots)
- repo-specific quality gates, hooks, or CI jobs

## Usage

```rust
// In a repo's xtask binary:
structural_lint::run_lint(&workspace_root, &["apps", "crates", "xtask"])?;
```

`run_lint` prints violations to stderr tagged `error[<rule>]` and returns
an error with the count when any exist. `collect_violations` returns the
same data as a `Vec<Violation>` for census and tooling use. Exit-code and
reporting policy belongs to the calling binary.

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
   `use super::*`. Use this for fixture types the tested logic
   collaborates with but the file does not otherwise import.
2. **Move the test to a dedicated test module:** a `#[cfg(test)]`-gated
   source module of the crate (a `*_seam_tests.rs` pattern), or
   `tests/` for public-API scenarios. Genuinely cross-module assertions
   belong here by nature.

**Enforcement:** `syn` parses each `#[cfg(test)]` module and walks every
`use` tree (including braced groups) and every path in expression, type,
and pattern position. Flagged: any path leading with `crate` whose
second segment is not the current file's module, and any path beginning
`super::super`. Paths inside macro invocation token streams are
unreachable and not checked.

**Allowed:**
- `use super::*` — the file's own items and its own imports.
- `use crate::<current_module>::...` — same file.
- `use serde_json::Value` — external crate (not local logic).

**Flagged:**
- `use crate::camera_controller::CameraController` in a test inside
  `scene_entities.rs` — cross-file import.
- `use super::super::super::super::media_audio::AudioBlock` — the same
  import spelled as a climb; equally flagged.

## 4. No recursion

Recursive algorithms are written as explicit iterative loops (with an
explicit stack or queue) or as visitor patterns where the visitor
completes work *during* traversal. Direct self-recursion is not used
anywhere: not in free functions, not in `impl` methods, not in trait
default methods.

**Why:** recursion causes stack overflow on deep structures, allocates
intermediate Vecs at each frame, and is structurally slower than the
equivalent loop. The visitor pattern is the preferred alternative: walk
the structure iteratively, and at each node the visitor does its work
immediately — no return value to accumulate, no list-building on the
way back.

**Enforcement:** `syn::visit::Visit` walks each function body. A
function is flagged when it calls itself by name through a
single-segment path (`foo()`) or, for methods, through a `self`
receiver (`self.foo()`). There is no suppression attribute: the lint
cannot prove tail position mechanically, so tail-recursive algorithms
are converted too — the loop form of a tail-recursive algorithm is a
mechanical rewrite.

**What counts as recursion:**
- `fn foo() { foo() }` — flagged (direct call).
- `impl T { fn foo(&self) { self.foo() } }` — flagged (`self`
  receiver).
- `fn foo() { other.foo() }` — NOT flagged (receiver is not `self`).
- `fn run() { commands::health::run() }` — NOT flagged (qualified
  path).

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
