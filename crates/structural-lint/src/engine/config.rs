//! Per-run configuration for the structural linter.
//!
//! [`Config`] owns the lint roots for a single invocation. It is
//! behavior-bearing (has an `impl` with `with_roots`), so this file is named
//! after the type and carries its own unit tests per rule 2.
//!
//! # Local invariants
//!
//! - `lint_roots` are relative directory names under the workspace root.
//! - Callers pass roots as `&[&str]`; [`Config::with_roots`] converts to owned
//!   `String`s.

/// Shared configuration for one lint run.
pub(crate) struct Config {
    /// Relative directory names under the workspace root to lint.
    pub(crate) lint_roots: Vec<String>,
}

impl Config {
    /// Build a config from a slice of root names.
    ///
    /// Each root is a directory name relative to the workspace root (e.g.
    /// `"crates"`). An empty slice results in zero files walked.
    #[must_use]
    pub(crate) fn with_roots(roots: &[&str]) -> Self {
        Self {
            lint_roots: roots.iter().map(|s| (*s).to_string()).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_roots_converts_slice_to_owned() {
        let config = Config::with_roots(&["a", "b"]);
        assert_eq!(config.lint_roots, vec!["a".to_string(), "b".to_string()]);
    }
}
