//! Plain-data struct for a single lint finding.
//!
//! `Diagnostic` is the internal vocabulary name (DESIGN-SPEC §3, §6.2). The
//! crate's public API re-exports it as `Violation` (`pub use ... as Violation`),
//! so `Diagnostic` and `Violation` are the SAME type — `Violation` is the
//! stable public name preserved for API compatibility.
//!
//! # Local invariants
//!
//! - Plain data: no `impl` block. Not behavior-bearing -> no unit test required.
//! - `rule` is always one of the five stable rule-id strings:
//!   `behavior-type-file`, `behavior-tests`, `empty-tests`, `test-isolation`,
//!   `cross-file-impl`.

/// A structural invariant violation produced by a rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Stable rule id, e.g. `"behavior-type-file"`.
    pub rule: &'static str,
    /// Human-readable description of the violation.
    pub message: String,
}
