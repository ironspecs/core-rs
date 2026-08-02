//! Integration test: the structural-lint crate lints its own `src/` clean.
//!
//! Exercises the public API (`collect_violations`, `Violation` field access)
//! and guards self-compliance with rules 1/2/3/5. Lives in `tests/` so it
//! observes the crate through its public interface only. Deterministic: it
//! reads the crate's own committed `src/`, with no external I/O.

use std::path::PathBuf;

use structural_lint::collect_violations;

#[test]
fn crate_src_is_self_compliant() {
    let manifest_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let violations = collect_violations(&manifest_root, &["src"]);
    assert!(
        violations.is_empty(),
        "structural-lint flags its own source:\n{}",
        violations
            .iter()
            .map(|v| format!("  error[{}]: {}", v.rule, v.message))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
