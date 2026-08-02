//! Filtered recursive walk of `*.rs` files under caller-supplied lint roots.
//!
//! This file owns the filesystem traversal logic. It contains only free
//! functions — no behavior-bearing types — so it requires no unit tests per
//! rule 2. The walk skips `target`, `.git`, and `node_modules` directories and
//! only collects files with the `rs` extension.
//!
//! # Local invariants
//!
//! - Relative paths are produced via `strip_prefix` with an ancestor search.
//!   Changing the ancestor logic changes violation message paths — do not
//!   modify the `strip_prefix` chain without verifying behavior preservation.
//! - Missing root directories are skipped silently (match today's behavior).
//! - The walk is single-threaded; no parallelism.

use std::path::{Path, PathBuf};

/// Collect all `*.rs` file paths under the configured lint roots, relative to
/// the workspace root.
pub(crate) fn rs_files(workspace_root: &Path, lint_roots: &[String]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for root_name in lint_roots {
        let root_dir = workspace_root.join(root_name);
        if !root_dir.is_dir() {
            continue;
        }
        collect_rs_files(&root_dir, root_name.as_str(), &mut files);
    }
    files
}

fn collect_rs_files(start: &Path, root_label: &str, out: &mut Vec<PathBuf>) {
    let mut stack: Vec<PathBuf> = vec![start.to_path_buf()];
    while let Some(current_dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current_dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                // Defensive skip of known non-source directories.
                if name == "target" || name == ".git" || name == "node_modules" {
                    continue;
                }
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                // Store path relative to the workspace root for reporting.
                let rel = path
                    .strip_prefix(
                        path.ancestors()
                            .find(|a| a.ends_with(root_label))
                            .unwrap_or(start)
                            .parent()
                            .unwrap_or(start),
                    )
                    .unwrap_or(&path)
                    .to_path_buf();
                out.push(rel);
            }
        }
    }
}
