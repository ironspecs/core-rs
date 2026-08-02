//! In-memory [`Emitter`] that collects diagnostics into a `Vec`.
//!
//! [`VecEmitter`] is behavior-bearing (it has `impl Emitter` with method
//! bodies), so this file is named after the struct and carries its own unit
//! tests per rule 2. It is the default emitter for the runner and for rule
//! unit tests.
//!
//! # Local invariants
//!
//! - `diagnostics` is pub so test code and the runner can inspect it directly.
//! - `emit` pushes — no dedup, no filtering, no side effects.
//! - The struct implements `Default` for ergonomic `VecEmitter::default()`.

use crate::engine::diagnostic::Diagnostic;
use crate::engine::emitter::Emitter;

/// An [`Emitter`] that appends every diagnostic to a public `Vec`.
#[derive(Default)]
pub(crate) struct VecEmitter {
    /// All diagnostics emitted during a run.
    pub(crate) diagnostics: Vec<Diagnostic>,
}

impl Emitter for VecEmitter {
    fn emit(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_diagnostics_in_order() {
        let mut em = VecEmitter::default();
        em.emit(Diagnostic {
            rule: "first",
            message: "m1".into(),
        });
        em.emit(Diagnostic {
            rule: "second",
            message: "m2".into(),
        });
        assert_eq!(em.diagnostics.len(), 2);
        assert_eq!(em.diagnostics[0].rule, "first");
        assert_eq!(em.diagnostics[0].message, "m1");
        assert_eq!(em.diagnostics[1].rule, "second");
        assert_eq!(em.diagnostics[1].message, "m2");
    }
}
