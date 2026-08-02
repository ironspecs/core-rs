//! Sink trait for diagnostics produced during a lint run.
//!
//! The [`Emitter`] trait is the single interface rules use to report findings.
//! Rules never print — they only `emit` [`Diagnostic`]s through this trait.
//! The trait has no default method bodies, so it is not behavior-bearing under
//! rule 2 and requires no unit tests.
//!
//! # Local invariants
//!
//! - The trait must not grow a default body for `emit` — that would make it
//!   behavior-bearing and force a unit test here.
//! - Concrete emitters (e.g. [`crate::engine::vec_emitter::VecEmitter`]) are
//!   their own files per rule 1.

use crate::engine::diagnostic::Diagnostic;

/// Accepts diagnostics from rules during a lint run.
pub(crate) trait Emitter {
    /// Accept one diagnostic.
    fn emit(&mut self, diagnostic: Diagnostic);
}
