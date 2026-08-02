//! Consumer of the finished diagnostic list.
//!
//! The [`Reporter`] trait owns all output formatting. It receives the complete
//! `&[Diagnostic]` after all rules have run. The trait has no default method
//! bodies, so it is not behavior-bearing under rule 2 and requires no unit
//! tests.
//!
//! # Local invariants
//!
//! - The trait must not grow a default body for `report` — that would make it
//!   behavior-bearing.
//! - Concrete reporters live in their own `snake_case` files per rule 1.

use crate::engine::diagnostic::Diagnostic;

/// Formats and outputs a finished list of diagnostics.
pub(crate) trait Reporter {
    /// Output all diagnostics in the reporter's format.
    fn report(&mut self, diagnostics: &[Diagnostic]);
}
