//! Orchestration: walk, parse, index, then run file-phase and workspace-phase
//! rules.
//!
//! This file owns the [`collect`] free function and the plain-data
//! [`ParsedFile`] helper struct. Neither is behavior-bearing under rule 2
//! (free functions do not count; `ParsedFile` has no `impl` block), so this
//! file requires no unit tests. Cross-phase correctness is verified by the
//! crate-level gates (`cargo xtask lint`).
//!
//! # Local invariants
//!
//! - Phase order is **mandatory**: file-phase rules run first, then workspace-
//!   phase rules. This matches the order in `all_rules()`.
//! - Unparseable files are silently skipped (`syn::parse_file` failure →
//!   `continue`).
//! - The emitter is a `VecEmitter` whose `diagnostics` are returned directly.

use std::path::Path;

use syn::File;

use crate::engine::config::Config;
use crate::engine::context::FileContext;
use crate::engine::diagnostic::Diagnostic;
use crate::engine::index::{WorkspaceIndex, ingest};
use crate::engine::util::crate_key_from_path;
use crate::engine::vec_emitter::VecEmitter;
use crate::engine::walk::rs_files;
use crate::rules::all_rules;

/// One successfully parsed file held in memory for the file phase.
struct ParsedFile {
    rel: String,
    filename: String,
    is_mod: bool,
    file: File,
}

/// Execute a full lint run and return all diagnostics.
///
/// Walks roots, parses files, builds the workspace index, then runs every
/// registered rule in file phase followed by workspace phase.
pub(crate) fn collect(workspace_root: &Path, config: &Config) -> Vec<Diagnostic> {
    let paths = rs_files(workspace_root, &config.lint_roots);

    let mut index = WorkspaceIndex::default();
    let mut parsed: Vec<ParsedFile> = Vec::new();

    for rel_path in &paths {
        let Ok(content) = std::fs::read_to_string(workspace_root.join(rel_path)) else {
            continue;
        };
        let Ok(file) = syn::parse_file(&content) else {
            continue;
        };

        let filename = rel_path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let is_mod = matches!(filename.as_str(), "mod" | "lib" | "main");

        let rel_str = rel_path.to_string_lossy().to_string();
        let crate_key = crate_key_from_path(&rel_str);

        ingest(&mut index, &file, &rel_str, &crate_key);
        parsed.push(ParsedFile {
            rel: rel_str,
            filename,
            is_mod,
            file,
        });
    }

    let rules = all_rules();
    let mut emitter = VecEmitter::default();

    // File phase: every rule checks every file.
    for pf in &parsed {
        let ctx = FileContext {
            relative_path: &pf.rel,
            filename_stem: &pf.filename,
            is_mod_rs: pf.is_mod,
            file: &pf.file,
        };
        for rule in &rules {
            rule.check_file(&ctx, &mut emitter);
        }
    }

    // Workspace phase: workspace-level rules run once.
    for rule in &rules {
        rule.check_workspace(&index, &mut emitter);
    }

    emitter.diagnostics
}
