//! Plain-data struct representing one successfully-parsed file, passed to
//! file-phase rules.
//!
//! [`FileContext`] is not behavior-bearing (no `impl` block) — it is
//! constructed with a struct literal by the runner. Rule 2 does not require
//! tests here.
//!
//! # Local invariants
//!
//! - Do not add an `impl` block or constructor function.
//! - Fields are `pub(crate)` so the runner can construct the struct directly.

/// Immutable view of one successfully-parsed file, passed to file-phase rules.
pub(crate) struct FileContext<'a> {
    /// Path of this file relative to the workspace root.
    pub(crate) relative_path: &'a str,
    /// The file's stem, e.g. `"my_type"` or `"mod"`.
    pub(crate) filename_stem: &'a str,
    /// True when the filename is `mod.rs`, `lib.rs`, or `main.rs`.
    pub(crate) is_mod_rs: bool,
    /// The parsed syntax tree for this file.
    pub(crate) file: &'a syn::File,
}
