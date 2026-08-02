//! Scratch workspace for integration tests: a temp tree under the crate's
//! `target/test-fixtures/` holding synthetic `.rs` files, removed on drop.
//!
//! `Workspace` is behavior-bearing (`impl Workspace` + `impl Drop`), so this
//! file is named after the type and carries its own unit test per rule 2.
//! Fixtures live under `target/` so the linter's own walk (which prunes
//! `target`) never sees them, and so no writes escape the crate.
//!
//! # Local invariants
//!
//! - The scratch root is unique per process + name to avoid collisions between
//!   parallel test processes and concurrent test runs.
//! - `Drop` removes the whole tree, so panicking tests still clean up via the
//!   unwinding drop.
//! - Paths passed to `collect_violations`/`run_lint` borrow `root()`; the
//!   `Workspace` must outlive the call.

use std::fs;
use std::path::{Path, PathBuf};

/// A synthetic workspace rooted under `target/test-fixtures/`, cleaned up on drop.
pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    /// Create an empty workspace uniquely named by `name`.
    #[must_use]
    pub fn new(name: &str) -> Self {
        let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        root.push("target");
        root.push("test-fixtures");
        root.push(format!("{}-{}", std::process::id(), name));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("scratch dir creation must succeed");
        Self { root }
    }

    /// Create a workspace pre-populated with `files` (each `(rel_path, content)`).
    #[must_use]
    pub fn with_files(name: &str, files: &[(&str, &str)]) -> Self {
        let ws = Self::new(name);
        for (rel, content) in files {
            ws.write(rel, content);
        }
        ws
    }

    /// Write `content` to the file at `rel` (e.g. `"fakecrate/src/foo.rs"`),
    /// creating parent directories as needed.
    pub fn write(&self, rel: &str, content: &str) {
        let full = self.root.join(rel);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).expect("parent dir creation must succeed");
        }
        fs::write(&full, content).expect("scratch file write must succeed");
    }

    /// The workspace root — pass as `workspace_root` to the lint entry points.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_files_creates_all_files() {
        let ws = Workspace::with_files(
            "workspace_helper",
            &[
                ("fakecrate/src/a.rs", "pub struct A;"),
                ("fakecrate/src/sub/b.rs", "pub struct B;"),
            ],
        );
        assert!(ws.root().join("fakecrate/src/a.rs").is_file());
        assert!(ws.root().join("fakecrate/src/sub/b.rs").is_file());
        assert_eq!(
            fs::read_to_string(ws.root().join("fakecrate/src/a.rs")).unwrap(),
            "pub struct A;",
        );
    }
}
