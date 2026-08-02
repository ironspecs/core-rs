# structural-lint engine refactor — design & product specification

**Status:** approved for implementation  
**Audience:** implementer (Kimi K2 / GLM / equivalent)  
**Owner crate:** `crates/structural-lint` in **core-rs**  
**Constraint:** behavior-preserving refactor first. No new product rules in this PR unless listed as optional stretch under §12.

If a choice is not specified here, **do not invent**. Match existing public behavior of `run_lint` / `collect_violations` and the vocabulary in §3.

---

## 1. Goal (end result)

Transform `structural-lint` from a **monolithic check script** (`lib.rs` + ad-hoc helpers) into a **small linter engine** shaped like Biome / oxlint / ESLint:

1. A **filtered file walk** builds shared context.
2. **Isolated rules** observe that context and **emit diagnostics**.
3. One or more **reporters** consume diagnostics (stderr today; structured list for tests).
4. **Config** is shared for that run (roots, which rules run — keep minimal).

**After this work:**

- Every existing mechanical rule is a **separate rule module** with **its own unit tests**.
- The public entry points `run_lint` and `collect_violations` still exist and remain the **only** required API for xtask callers (signature may gain an optional config later; default must match today’s behavior).
- `cargo test -p structural-lint` and `cargo xtask lint` in core-rs stay green.
- **No change** to which rule IDs fire on a clean core-rs tree (same violations, same rule tag strings).

**Success test for the human reviewer:** open `src/rules/` and see one file (or directory) per rule; open `src/engine/` and see walk / context / emit / report only — no rule logic mixed in.

---

## 2. Non-goals (explicitly out of scope)

Do **not** implement any of the following in this change:

| Out of scope | Why |
|--------------|-----|
| New product rules (tests/ vocabulary, module seal, proofs harness, e2e-only process, unit-no-network) | Separate PRs after the engine exists |
| Autofix / codemods | Unsafe for structural moves |
| SARIF, GitHub annotations, JSON CLI flags | Reporter trait may allow a second reporter later; **only** implement HumanStderr + collect-to-vec |
| Parallel file walk / rayon | Premature |
| Plugin system, dynamic loading, WASM | Not needed |
| Span pretty-printing, source snippets, miette | Path + message only (as today) |
| Replacing clippy or rustc | Orthogonal |
| Changing services-rs in this PR | services-rs adopts the crate later |
| TOML/JSON config files on disk | Optional stretch only (§12); default hard-coded “all rules on” |
| Suppression attributes (`#[allow(structural::…)]`) | Forbidden today; keep forbidden |
| Renaming rule IDs | Must stay stable (see §6) |
| “Improving” or relaxing existing rule semantics | Behavior-preserving only |
| Documentation website, diagrams beyond this file | This file is enough |

If the implementer believes a dependency or module is required and is not listed in §5–§7, **stop and ask** — do not add it.

---

## 3. Vocabulary (use these words only)

Use this vocabulary in code, comments, and tests. **Do not** introduce alternate metaphors (pipeline, middleware, bus, actor, plugin host, analyzer service, etc.).

| Term | Meaning in this crate |
|------|------------------------|
| **Rule** | One isolated check with a stable string **rule id**. Implements the `Rule` trait. |
| **Rule id** | Stable kebab or existing tag string used in output, e.g. `behavior-type-file`, `test-isolation`. |
| **Diagnostic** | One finding produced by a rule (replaces / wraps today’s `Violation` for internal use; public `Violation` may remain as a thin alias or identical struct for API stability). |
| **Emit** / **Emitter** | Sink that accepts diagnostics during a run (`emit(diagnostic)`). |
| **Reporter** | Consumer of the finished diagnostic list (and/or stream). Writes human output and/or returns data. |
| **Config** | Per-run settings: lint roots, enabled rules (default: all current rules on). |
| **Context** | Read-only view given to a rule for one file (path, parsed AST, crate key, filename stem, flags). |
| **Workspace index** | Data accumulated across all files for **multi-file rules** only (type def/impl maps). |
| **Runner** / **Engine** | Orchestrates: walk → parse → file rules → workspace rules → reporters. |
| **Filtered walk** | Recursive directory walk under configured roots, skipping `target`, `.git`, `node_modules`, only `*.rs`. |
| **File phase** | Rules that run once per successfully parsed file. |
| **Workspace phase** | Rules that run once after all files are indexed (cross-file-impl). |

**Competitors / reference products** (study these for shape, not for feature parity):

| Product | Steal this idea | Do not steal |
|---------|-----------------|--------------|
| **ESLint** | Rule modules + rule ids + severity; CLI vs library split | JS plugin ecosystem, shareable configs complexity |
| **Biome** | Internal analyzer + emitters + printers; monorepo-quality Rust-adjacent design docs | Full formatter, JS/TS stack |
| **oxlint / Oxc** | Fast lint, isolated rules, clear diagnostic codes | JS parser, napi bindings |
| **clippy** | `declare_clippy_lint!`, per-lint modules, rustc driver | rustc internals, cargo integration as a driver |
| **ruff** | Rule codes, categories, select/ignore | Python AST, huge rule catalog |

When unsure how to name a type, prefer **ESLint/Biome terms** (`Rule`, `Diagnostic`, `Reporter`) over inventing new ones.

---

## 4. Behavioral contract (must not break)

### 4.1 Public API (callers)

These must keep working for `core-rs` xtask and any external caller:

```rust
// Existing — preserve signatures and semantics.
pub fn collect_violations(workspace_root: &Path, lint_roots: &[&str]) -> Vec<Violation>;
pub fn run_lint(workspace_root: &Path, lint_roots: &[&str]) -> anyhow::Result<()>;

pub struct Violation {
    pub rule: &'static str,
    pub message: String,
}
```

**`run_lint` semantics today (preserve):**

- On zero violations: print exactly `lint: no violations` to **stdout**, return `Ok(())`.
- On one or more: print each as `error[{rule}]: {message}` to **stderr** (see current `lib.rs`), then `bail!("lint: {} violation(s)", count)`.
- Message field content for each rule must remain **semantically equivalent** (same rule id; same paths/types called out). Wording may match current format strings exactly where possible to avoid noisy diffs in downstream allowlists.

**`collect_violations`:** returns the same set of findings; no printing required.

### 4.2 Rule ids (stable strings)

Use **exactly** these rule id strings (match current `error[…]` tags in code — verify against `lib.rs` when implementing and do not rename):

| Rule id (verify in source) | Phase |
|----------------------------|--------|
| `behavior-type-file` | file |
| `behavior-tests` | file |
| empty-tests / whatever tag empty test modules use today | file |
| `test-isolation` | file |
| `cross-file-impl` | workspace |

If the current code uses a slightly different tag string, **keep the current string** — do not “clean up” ids in this PR.

### 4.3 Lint roots

- Caller passes `lint_roots: &[&str]` relative to `workspace_root`.
- core-rs xtask continues to pass `&["crates"]` only.
- Walk behavior: skip dirs named `target`, `.git`, `node_modules`; only files with extension `rs`.
- Missing root dir: skip silently (today’s behavior).

### 4.4 Parse failures

Today: skip file on `syn::parse_file` failure (`continue`).  
**Preserve:** skip unparsable files; do not add a parse diagnostic in this PR.

---

## 5. Module layout (exact)

Replace the current two-file layout with the following. **Do not add modules beyond this list** without necessity.

```text
crates/structural-lint/
  Cargo.toml                 # deps unchanged unless §8
  README.md                  # update to describe engine + rule list (short)
  DESIGN-SPEC.md             # this file (do not delete)
  src/
    lib.rs                   # public re-exports + run_lint + collect_violations ONLY
    engine/
      mod.rs                 # re-export engine pieces
      config.rs              # Config
      diagnostic.rs          # Diagnostic (+ From/Into Violation if split)
      emit.rs                # Emitter trait + VecEmitter
      reporter.rs            # Reporter trait + HumanReporter
      context.rs             # FileContext, paths helpers used by rules
      index.rs               # WorkspaceIndex (type def/impl maps)
      walk.rs                # filtered walk of *.rs
      runner.rs              # orchestrate phases
    rules/
      mod.rs                 # registry: all_rules() -> Vec<Box<dyn Rule>>
      behavior_type_file.rs
      behavior_tests.rs      # includes empty-test-module logic if it lives with behavior-tests today
      test_isolation.rs      # moves test_path_visitor usage here
      cross_file_impl.rs
    # KEEP or relocate:
    test_path_visitor.rs     # stay as support for test_isolation rule ONLY
                             # (either src/test_path_visitor.rs or rules/test_isolation/visitor.rs)
```

**`lib.rs` responsibilities only:**

- `pub use` of `Violation`, `run_lint`, `collect_violations` (and optionally `Config` if public).
- Wire `collect_violations` → `engine::runner::run(...)`.
- **No rule logic.**

**Forbidden layout:**

- One mega-`rules.rs` with all checks inlined.
- Rules calling reporters directly (rules only `emit`).
- Engine modules importing a specific rule by path except `rules/mod.rs` registry.

---

## 6. Contracts (types and traits)

### 6.1 `Config` (`engine/config.rs`)

```rust
pub struct Config {
    /// Relative directory names under workspace root (same as today’s lint_roots).
    pub lint_roots: Vec<String>,
    // Future: rule enable map. For this PR:
    // either omit, or include HashMap<RuleId, bool> defaulting all true.
}
```

```rust
impl Config {
    pub fn with_roots(roots: &[&str]) -> Self { /* ... */ }
}
```

`collect_violations(workspace_root, lint_roots)` builds `Config::with_roots(lint_roots)` internally.

### 6.2 `Diagnostic` (`engine/diagnostic.rs`)

```rust
pub struct Diagnostic {
    pub rule: &'static str,
    pub message: String,
    // Do not add fields in this PR (no severity, no span) unless required
    // for a lossless mapping from Violation — prefer identical to Violation.
}
```

If `Diagnostic` === `Violation` fields, you may:

```rust
pub type Violation = Diagnostic;
// or
impl From<Diagnostic> for Violation { ... }
```

Public name **`Violation` must remain** for API stability.

### 6.3 `Emitter` (`engine/emit.rs`)

```rust
pub trait Emitter {
    fn emit(&mut self, diagnostic: Diagnostic);
}

pub struct VecEmitter {
    pub diagnostics: Vec<Diagnostic>,
}

impl Emitter for VecEmitter { /* push */ }
```

Rules take `&mut dyn Emitter` (or generic `E: Emitter`). **Rules never print.**

### 6.4 `Reporter` (`engine/reporter.rs`)

```rust
pub trait Reporter {
    fn report(&mut self, diagnostics: &[Diagnostic]);
}

/// Matches today’s run_lint printing + caller handles exit via Result.
pub struct HumanReporter;

impl Reporter for HumanReporter {
    fn report(&mut self, diagnostics: &[Diagnostic]) {
        // empty: print "lint: no violations\n" to stdout
        // non-empty: for d in diagnostics { eprintln!("error[{}]: {}", d.rule, d.message); }
    }
}
```

`run_lint`:

1. `diagnostics = runner::collect(...)`
2. `HumanReporter.report(&diagnostics)`
3. if empty Ok; else bail with count

Do **not** invent logging frameworks (`tracing` for the linter itself is out of scope).

### 6.5 `FileContext` (`engine/context.rs`)

Immutable view for file-phase rules:

```rust
pub struct FileContext<'a> {
    pub workspace_root: &'a Path,
    pub relative_path: &'a str,      // as today in violation messages
    pub filename_stem: &'a str,      // file_stem, e.g. "mem_store", "mod"
    pub is_mod_rs: bool,             // mod.rs | lib.rs | main.rs
    pub crate_key: &'a str,          // crate_key_from_path
    pub file: &'a syn::File,
}
```

Helpers that multiple rules need (`to_snake_case`, `parent_dir`, `crate_key_from_path`) live in `context.rs` or `engine/util.rs` — **one place**, used by rules via `pub(crate)` functions. Do not duplicate snake_case in every rule file.

### 6.6 `WorkspaceIndex` (`engine/index.rs`)

Built during the walk for multi-file rules:

```rust
pub struct WorkspaceIndex {
    /// crate_key -> (type_name -> relative path of definition)
    pub type_defs: HashMap<String, HashMap<String, String>>,
    /// crate_key -> (type_name -> list of relative paths with impl)
    pub type_impls: HashMap<String, HashMap<String, Vec<String>>>,
}
```

Population logic moves from current `run()` accumulation loops — **same keys and paths** as today.

### 6.7 `Rule` trait (`rules/mod.rs` or `engine/rule.rs`)

```rust
pub trait Rule: Send + Sync {
    /// Stable id, e.g. "test-isolation"
    fn id(&self) -> &'static str;

    /// File phase. Default: no-op.
    fn check_file(&self, ctx: &FileContext<'_>, emitter: &mut dyn Emitter) {}

    /// Workspace phase. Default: no-op.
    fn check_workspace(&self, index: &WorkspaceIndex, emitter: &mut dyn Emitter) {}
}
```

**Registration** in `rules/mod.rs`:

```rust
pub fn all_rules() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(behavior_type_file::BehaviorTypeFile),
        Box::new(behavior_tests::BehaviorTests),
        Box::new(test_isolation::TestIsolation),
        Box::new(cross_file_impl::CrossFileImpl),
        // empty-tests: either merged into BehaviorTests or its own struct —
        // match how logic is split in current lib.rs
    ]
}
```

Each rule is a unit struct (`pub struct BehaviorTypeFile;`) with `impl Rule for BehaviorTypeFile`.

### 6.8 Runner (`engine/runner.rs`)

```rust
pub fn collect(
    workspace_root: &Path,
    config: &Config,
) -> Vec<Diagnostic> // or Vec<Violation>
```

Algorithm (mandatory order):

1. `paths = walk::rs_files(workspace_root, &config.lint_roots)`
2. `index = WorkspaceIndex::default()`
3. `parsed: Vec<(path meta, File)> = empty`
4. For each path: read, `syn::parse_file`, on failure **continue**; else update index (struct/enum/trait defs + impl files), push to `parsed`
5. `emitter = VecEmitter::default()`
6. For each parsed file: build `FileContext`; for each rule in `all_rules()`: `rule.check_file(&ctx, &mut emitter)`
7. For each rule: `rule.check_workspace(&index, &mut emitter)`
8. Return `emitter.diagnostics`

`collect_violations` = `runner::collect` mapped to `Violation` if types differ.

---

## 7. Rule modules — move existing logic only

For each rule file:

1. Cut the corresponding `check_*` function(s) and helpers **only used by that rule** from `lib.rs`.
2. Paste into the rule module; call from `check_file` / `check_workspace`.
3. Preserve message strings and rule id tags.
4. Add unit tests in the **same file** under `#[cfg(test)]` (this crate follows structural rules too where applicable).

| Module | Must contain |
|--------|----------------|
| `behavior_type_file.rs` | Former `check_behavior_type_file_naming` |
| `behavior_tests.rs` | Former `check_behavior_tests` **and** empty-test detection if currently co-located |
| `test_isolation.rs` | Former `check_test_isolation` + `check_use_tree`; uses `test_path_visitor` |
| `cross_file_impl.rs` | Former `check_cross_file_impls` via `check_workspace` only |

**Do not** change detection heuristics (what counts as behavior, snake_case rules, etc.) except to fix compile errors from the move.

---

## 8. Dependencies

**Allowed (already in crate / workspace):**

- `syn` (full features as today)
- `anyhow`

**Do not add** unless unavoidable for the refactor:

- serde, toml, clap, tracing, rayon, ignore, walkdir (std walk is enough and already used)

If you believe `walkdir` is cleaner, **do not add it** in this PR — keep the existing manual stack walk moved to `walk.rs`.

---

## 9. Testing requirements

### 9.1 Crate tests

- Keep or migrate existing tests in `lib.rs` that still apply (snake_case helpers, etc.) next to the helper or in `engine/context` tests.
- **Each rule module** must have `#[cfg(test)]` tests that:
  - Parse a **minimal** `&str` fixture with `syn::parse_file`.
  - Run **only that rule** against a synthetic `FileContext` / index.
  - Assert diagnostic count and rule id (and substring of message if useful).

Example pattern (implementer may adjust names):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    // build FileContext from parse_file("...").unwrap()
    // let mut em = VecEmitter::default();
    // BehaviorTypeFile.check_file(&ctx, &mut em);
    // assert_eq!(em.diagnostics.len(), 1);
    // assert_eq!(em.diagnostics[0].rule, "behavior-type-file");
}
```

### 9.2 Integration / xtask

- `cargo test -p structural-lint` passes.
- `cargo xtask lint` (core-rs) still reports **no violations** on the clean tree.
- Optional: one test that `collect_violations(workspace_root, &["crates"])` returns empty when run from core-rs — only if already present or cheap; not required if xtask is the gate.

### 9.3 Do not

- Snapshot entire core-rs lint output as a huge golden file.
- Call the network or write outside the crate.

---

## 10. xtask / README

- **xtask:** remain a thin caller of `structural_lint::run_lint`; no engine logic.
- **README.md:** update “owns” section to mention engine + per-rule modules; keep usage example identical.

---

## 11. Implementation order (mandatory sequence)

Execute in this order. Do not skip ahead to new features.

1. Add `engine/` modules with types/traits **and empty/stub runner** that still can call old functions if needed for a transitional compile.
2. Move walk + index building into `walk.rs` / `index.rs`.
3. Extract rules one at a time into `rules/*.rs`, register in `all_rules()`, delete old functions from `lib.rs`.
4. Point `collect_violations` / `run_lint` at `runner::collect` + `HumanReporter`.
5. Add per-rule unit tests.
6. Run `cargo test -p structural-lint` and `cargo xtask lint`.
7. Update README.

**Preferred:** single PR, behavior-preserving.  
**Acceptable:** two PRs — (1) engine shell + one rule moved, (2) remaining rules — each PR must keep xtask lint green.

---

## 12. Optional stretch (only if 1–11 done and green)

Do these **only** if explicitly time-left and tests green; otherwise stop.

1. `Config` rule enable map: `HashMap<&'static str, bool>` default all true; no file I/O.
2. Second reporter used only in tests (`RecordingReporter`).
3. Public `pub fn collect_with_config(root, &Config) -> Vec<Violation>` in addition to existing API.

Do **not** implement testing-strategy rules (tests vocabulary, seals, proofs) in stretch.

---

## 13. Definition of done (checklist)

- [ ] Module tree matches §5 (no extra top-level modules).
- [ ] Vocabulary matches §3 in public/engine code.
- [ ] `run_lint` / `collect_violations` / `Violation` preserved (§4).
- [ ] Rule ids unchanged (§4.2).
- [ ] Rules do not print; only emitters/reporters print (§6).
- [ ] Each rule isolated + unit-tested (§7, §9).
- [ ] `cargo test -p structural-lint` green.
- [ ] `cargo xtask lint` green on core-rs.
- [ ] README updated.
- [ ] No new product rules; no new heavy deps (§2, §8).
- [ ] This DESIGN-SPEC.md retained.

---

## 14. Anti-patterns (reject in review)

- “While refactoring, tighten snake_case / isolation heuristics.”
- Adding `Severity::Warn` and changing exit behavior.
- Putting filesystem testing-layout rules in this PR “because the engine is ready.”
- Duplicating `to_snake_case` in four rule files.
- Using `println!` inside a rule for debugging left in the merge.
- Renaming `Violation` to `LintError` in the public API.
- Introducing async, threads, or global mutable registries for rules.

---

## 15. Reference: current entry usage (do not break)

```rust
// core-rs/xtask/src/main.rs
structural_lint::run_lint(&workspace_root(), &["crates"])?;
```

```rust
// Future services-rs (out of scope for this PR, for implementer awareness only):
// structural_lint::run_lint(&root, &["apps", "crates", "xtask"])?;
```

---

## 16. One-paragraph summary for the implementer

Refactor `structural-lint` into an ESLint/Biome-style engine: walk roots, parse once, run isolated `Rule` implementations that only `emit` diagnostics, then report with a human stderr reporter. Preserve `run_lint`, `collect_violations`, `Violation`, rule id strings, and violation semantics exactly. Put code under `engine/*` and `rules/*` as specified; unit-test each rule with `syn::parse_str` fixtures. Do not add new lint rules, config files, autofix, or dependencies. Stop when xtask lint and crate tests are green.
