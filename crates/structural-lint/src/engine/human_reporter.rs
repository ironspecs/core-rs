//! Human-readable stderr/stdout reporter matching today's `run_lint` output.
//!
//! [`HumanReporter`] reproduces today's `run_lint` output; formatting is a
//! pure assoc fn `render` so the output contract is unit-tested without I/O.
//! The `Report` trait method delegates to `render` and writes to
//! stdout/stderr.
//!
//! # Local invariants
//!
//! - `println!` to stdout on empty; `eprintln!` to stderr per violation.
//! - This struct holds no state — it is a pure output formatter.
//! - The word "error" in the prefix is part of the output contract; do not
//!   change it.

use crate::engine::diagnostic::Diagnostic;
use crate::engine::reporter::Reporter;

/// Reporter that prints diagnostics in today's `error[rule]: message` format.
pub(crate) struct HumanReporter;

impl HumanReporter {
    /// Render `diagnostics` as `(stdout line, stderr lines)`.
    ///
    /// Empty -> `Some("lint: no violations")` on stdout, no stderr.
    /// Non-empty -> no stdout, one `error[rule]: message` stderr line per item.
    fn render(diagnostics: &[Diagnostic]) -> (Option<String>, Vec<String>) {
        if diagnostics.is_empty() {
            return (Some("lint: no violations".to_string()), Vec::new());
        }
        let stderr = diagnostics
            .iter()
            .map(|d| format!("error[{}]: {}", d.rule, d.message))
            .collect();
        (None, stderr)
    }
}

impl Reporter for HumanReporter {
    fn report(&mut self, diagnostics: &[Diagnostic]) {
        let (stdout, stderr) = Self::render(diagnostics);
        if let Some(line) = stdout {
            println!("{line}");
        }
        for line in stderr {
            eprintln!("{line}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_empty_emits_success_line() {
        let (out, err) = HumanReporter::render(&[]);
        assert_eq!(out.as_deref(), Some("lint: no violations"));
        assert!(err.is_empty());
    }

    #[test]
    fn render_violations_emit_error_lines() {
        let d1 = Diagnostic {
            rule: "behavior-tests",
            message: "m1".into(),
        };
        let d2 = Diagnostic {
            rule: "empty-tests",
            message: "m2".into(),
        };
        let (out, err) = HumanReporter::render(&[d1, d2]);
        assert!(out.is_none());
        assert_eq!(
            err,
            vec!["error[behavior-tests]: m1", "error[empty-tests]: m2"]
        );
    }
}
