//! Structural linter that enforces five invariant rules on `.rs` files under
//! caller-supplied lint roots.
//!
//! This file is the **public façade only**. It re-exports the public API types
//! and wires `run_lint` and `collect_violations` to the engine. It contains no
//! behavior-bearing types — only free functions and a re-export.
//!
//! # Engine architecture
//!
//! The crate is shaped as:
//!
//! - **`engine/`** — walk → parse → context → workspace index → rule runner →
//!   emitter → reporter. All orchestration, shared helpers, and traits live
//!   here.
//! - **`rules/`** — isolated `Rule` implementations, one per file. Rules
//!   observe a [`FileContext`] or [`WorkspaceIndex`] and **only `emit`
//!   [`Diagnostic`]s**; they never print.
//! - **`test_path_visitor.rs`** — support visitor for the `test-isolation`
//!   rule (kept at crate root for backward compatibility).
//!
//! The public API (`run_lint`, `collect_violations`, `Violation`) is unchanged
//! by the refactor.
//!
//! # Rules
//!
//! 1. **behavior-type-file** — every behavior-bearing type lives in its own
//!    snake_case-named file.
//! 2. **behavior-tests** — files with behavior methods must have a
//!    `#[cfg(test)]` module with at least one non-empty test.
//! 3. **empty-tests** — a test module with no non-empty tests is flagged.
//! 4. **test-isolation** — tests only reference their own module.
//! 5. **cross-file-impl** — a type's definition and all its `impl` blocks
//!    must be in the same file.
//!
//! Violations print to stderr tagged `error[<rule>]`. There are no exemptions
//! or suppress attributes.

mod engine;
mod rules;
mod test_path_visitor;

use std::path::Path;

use anyhow::Result;

use crate::engine::reporter::Reporter;

pub use crate::engine::diagnostic::Diagnostic as Violation;

/// Convenience wrapper over [`collect_violations`]: prints violations to stderr
/// and returns an error with the count when any exist. Prints `lint: no
/// violations` to stdout on success.
pub fn run_lint(workspace_root: &Path, lint_roots: &[&str]) -> Result<()> {
    let diagnostics = collect_violations(workspace_root, lint_roots);
    crate::engine::human_reporter::HumanReporter.report(&diagnostics);
    if diagnostics.is_empty() {
        Ok(())
    } else {
        anyhow::bail!("lint: {} violation(s)", diagnostics.len())
    }
}

/// Parse every `.rs` file under the given `lint_roots` (relative to
/// `workspace_root`), run all checks, and return structured violations.
///
/// An empty `lint_roots` slice results in zero files walked and zero
/// violations.
///
/// The caller controls which directories are linted; vendored, third-party,
/// and generated directories are omitted from the roots list.
#[must_use]
pub fn collect_violations(workspace_root: &Path, lint_roots: &[&str]) -> Vec<Violation> {
    let config = crate::engine::config::Config::with_roots(lint_roots);
    crate::engine::runner::collect(workspace_root, &config)
}
