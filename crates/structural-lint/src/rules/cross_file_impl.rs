//! Rule `cross-file-impl`: a type's definition and all its `impl` blocks must
//! be in the same file.
//!
//! This file owns the [`CrossFileImpl`] struct, its `impl Rule`, the private
//! `check` free function, and all unit tests. This is the only workspace-phase
//! rule — it does not implement `check_file`.
//!
//! # Local invariants
//!
//! - Rule id is `"cross-file-impl"` — do not rename.
//! - Only flags impls in a file different from the definition when both files
//!   share the same parent directory. Cross-directory impls are not flagged.
//! - Message strings are stable; the format is `"`{type}` defined in
//!   `{def}` but has impl in `{impl}` — definition and impl must be in the
//!   same file"`.

use crate::engine::diagnostic::Diagnostic;
use crate::engine::emitter::Emitter;
use crate::engine::index::WorkspaceIndex;
use crate::engine::rule::Rule;
use crate::engine::util::parent_dir;

/// Rule: definition and all impl blocks must be in the same file.
pub(crate) struct CrossFileImpl;

impl CrossFileImpl {
    pub(crate) const ID: &'static str = "cross-file-impl";
}

impl Rule for CrossFileImpl {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn check_workspace(&self, index: &WorkspaceIndex, emitter: &mut dyn Emitter) {
        check(index, emitter);
    }
}

fn check(index: &WorkspaceIndex, emitter: &mut dyn Emitter) {
    for (crate_key, defs) in &index.type_defs {
        if let Some(impls) = index.type_impls.get(crate_key) {
            for (type_name, impl_files) in impls {
                if let Some(def_file) = defs.get(type_name) {
                    for impl_file in impl_files {
                        if impl_file != def_file && parent_dir(impl_file) == parent_dir(def_file) {
                            emitter.emit(Diagnostic {
                                rule: CrossFileImpl::ID,
                                message: format!(
                                    "`{type_name}` defined in `{def_file}` but has impl in `{impl_file}` — definition and impl must be in the same file"
                                ),
                            });
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
use crate::engine::vec_emitter::VecEmitter;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn flags_cross_file_impl_same_crate() {
        let mut type_defs: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut type_impls: HashMap<String, HashMap<String, Vec<String>>> = HashMap::new();
        type_defs
            .entry("crates/x".to_owned())
            .or_default()
            .insert("Foo".to_owned(), "crates/x/src/foo.rs".to_owned());
        type_impls
            .entry("crates/x".to_owned())
            .or_default()
            .insert("Foo".to_owned(), vec!["crates/x/src/bar.rs".to_owned()]);
        let mut em = VecEmitter::default();
        let index = WorkspaceIndex {
            type_defs,
            type_impls,
        };
        check(&index, &mut em);
        assert_eq!(em.diagnostics.len(), 1);
        assert_eq!(em.diagnostics[0].rule, "cross-file-impl");
    }

    #[test]
    fn silent_when_impl_in_same_file() {
        let mut type_defs: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut type_impls: HashMap<String, HashMap<String, Vec<String>>> = HashMap::new();
        type_defs
            .entry("crates/x".to_owned())
            .or_default()
            .insert("Foo".to_owned(), "crates/x/src/foo.rs".to_owned());
        type_impls
            .entry("crates/x".to_owned())
            .or_default()
            .insert("Foo".to_owned(), vec!["crates/x/src/foo.rs".to_owned()]);
        let mut em = VecEmitter::default();
        let index = WorkspaceIndex {
            type_defs,
            type_impls,
        };
        check(&index, &mut em);
        assert!(em.diagnostics.is_empty());
    }
}
