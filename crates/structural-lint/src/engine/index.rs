//! Workspace-wide index of type definitions and `impl` blocks for multi-file
//! rules.
//!
//! [`WorkspaceIndex`] is plain data (`#[derive(Default)]`, no hand-written
//! `impl`). It is populated by the free function [`ingest`] during the parse
//! pass and consumed by workspace-phase rules (currently `cross-file-impl`).
//! Free functions are not behavior-bearing under rule 2.
//!
//! # Local invariants
//!
//! - Keys are `(crate_key, type_name)`; the same crate key used in
//!   `FileContext::crate_key` and derived from `crate_key_from_path`.
//! - `type_defs` maps a type to its *first* definition file within a crate.
//! - `type_impls` maps a type to the list of files containing `impl` blocks.
//! - Do not add methods to [`WorkspaceIndex`] — keep it plain data.

use std::collections::HashMap;

use syn::Item;

use crate::engine::util::extract_impl_type_name;

/// Accumulated type-def and impl-location data for the entire workspace.
#[derive(Default)]
pub(crate) struct WorkspaceIndex {
    /// `crate_key → (type_name → relative path of definition)`.
    pub(crate) type_defs: HashMap<String, HashMap<String, String>>,
    /// `crate_key → (type_name → list of relative paths with impl)`.
    pub(crate) type_impls: HashMap<String, HashMap<String, Vec<String>>>,
}

/// Ingest one parsed file into the workspace index.
///
/// Registers `struct`, `enum`, and `trait` definitions in `type_defs` and
/// `impl` blocks in `type_impls`, keyed by crate key and type name.
pub(crate) fn ingest(index: &mut WorkspaceIndex, file: &syn::File, rel: &str, crate_key: &str) {
    let crate_key = crate_key.to_string();
    for item in &file.items {
        match item {
            Item::Struct(s) => {
                index
                    .type_defs
                    .entry(crate_key.clone())
                    .or_default()
                    .entry(s.ident.to_string())
                    .or_insert(rel.to_string());
            }
            Item::Enum(e) => {
                index
                    .type_defs
                    .entry(crate_key.clone())
                    .or_default()
                    .entry(e.ident.to_string())
                    .or_insert(rel.to_string());
            }
            Item::Trait(t) => {
                index
                    .type_defs
                    .entry(crate_key.clone())
                    .or_default()
                    .entry(t.ident.to_string())
                    .or_insert(rel.to_string());
            }
            Item::Impl(impl_block) => {
                if let Some(name) = extract_impl_type_name(impl_block) {
                    index
                        .type_impls
                        .entry(crate_key.clone())
                        .or_default()
                        .entry(name)
                        .or_default()
                        .push(rel.to_string());
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ingest_records_defs_and_impls() {
        let code = "struct Foo; impl Foo { fn x() {} }";
        let file = syn::parse_file(code).unwrap();
        let mut index = WorkspaceIndex::default();
        ingest(&mut index, &file, "c/x/src/foo.rs", "c/x");

        assert_eq!(index.type_defs["c/x"]["Foo"], "c/x/src/foo.rs");
        assert_eq!(index.type_impls["c/x"]["Foo"], vec!["c/x/src/foo.rs"]);
    }
}
