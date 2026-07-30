//! xtask — repo automation entry point for the core-rs foundation workspace.
//!
//! This file owns command dispatch and lint roots. It contains only free
//! functions and plain-data constants; no behavior-bearing types
//! (struct/enum/trait with impl) live here.
//!
//! The lint engine lives in the `structural-lint` crate; this binary supplies
//! the repo's lint roots.
//!
//! Invariants:
//! - Every supported command has a match arm in `main` and a help line.
//! - `workspace_root()` is the single source of truth for the repo root path.

use std::path::PathBuf;

use anyhow::bail;

/// Lint roots for this repository — owned by the caller per the `structural-lint`
/// crate contract. The lint engine walks every `.rs` file under these roots.
const LINT_ROOTS: &[&str] = &["crates"];

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("lint") => structural_lint::run_lint(&workspace_root(), LINT_ROOTS)?,
        Some("help") | None => println!("cargo xtask lint"),
        Some(other) => bail!("unsupported xtask command: {other}"),
    }
    Ok(())
}

/// Returns the workspace root — the parent of the xtask crate's manifest
/// directory. This is a structural invariant: xtask always lives one level
/// below the workspace root.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask is always one level below workspace root")
        .to_path_buf()
}
