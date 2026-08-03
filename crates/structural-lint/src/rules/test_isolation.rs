//! Rule `test-isolation`: inline test modules must only reference their own
//! module.
//!
//! This file owns the [`TestIsolation`] struct, its `impl Rule`, the private
//! `check` and `check_use_tree` free functions, and all unit tests. The rule
//! runs in the file phase and delegates path walking to [`TestPathVisitor`],
//! which emits diagnostics directly into the rule's [`Emitter`].
//!
//! # Local invariants
//!
//! - Rule id is `"test-isolation"` — do not rename.
//! - `check_use_tree` walks `UseTree` nodes for cross-module `crate::...`
//!   imports (terminal names, module paths, renames, and brace groups — see
//!   `check_crate_child`) and `super::super` climbs in import statements.
//! - `TestPathVisitor` walks expression/type/pattern paths for the same
//!   patterns outside of use statements, emitting directly. There is no
//!   double-reporting.

use syn::{File, Item, UseTree};

use crate::engine::context::FileContext;
use crate::engine::diagnostic::Diagnostic;
use crate::engine::emitter::Emitter;
use crate::engine::rule::Rule;
use crate::engine::util::is_cfg_test;
use crate::test_path_visitor::TestPathVisitor;

/// Rule: tests must only reference their own module.
pub(crate) struct TestIsolation;

impl TestIsolation {
    pub(crate) const ID: &'static str = "test-isolation";
}

impl Rule for TestIsolation {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn check_file(&self, ctx: &FileContext<'_>, emitter: &mut dyn Emitter) {
        check(ctx.file, ctx.relative_path, ctx.filename_stem, emitter);
    }
}

fn check(file: &File, rel: &str, filename: &str, emitter: &mut dyn Emitter) {
    for item in &file.items {
        if let Item::Mod(m) = item {
            if !m.attrs.iter().any(is_cfg_test) {
                continue;
            }
            if let Some((_, items)) = &m.content {
                // Layer (a): walk use trees for crate::<other> and
                // super::super climbs in import statements.
                for inner in items {
                    if let Item::Use(use_item) = inner {
                        check_use_tree(&use_item.tree, filename, rel, emitter);
                    }
                }

                // Layer (b): walk every syn::Path in expression, type,
                // and pattern position for the same two patterns.
                // TestPathVisitor emits diagnostics directly into the
                // emitter.
                let mut visitor =
                    TestPathVisitor::new(filename.to_owned(), rel.to_owned(), emitter);
                for inner in items {
                    syn::visit::visit_item(&mut visitor, inner);
                }
            }
        }
    }
}

fn check_use_tree(tree: &UseTree, current: &str, rel: &str, emitter: &mut dyn Emitter) {
    let mut stack = vec![tree];
    while let Some(t) = stack.pop() {
        match t {
            UseTree::Path(path) => {
                // Flag any `crate::...` import whose first segment after
                // `crate` is neither the current module nor `self`. Covers
                // terminal re-exported names, nested module paths, renames,
                // and brace groups — see `check_crate_child`.
                if path.ident == "crate" {
                    check_crate_child(&path.tree, current, rel, emitter);
                }
                // Flag super::super::... (climb past immediate parent).
                if path.ident == "super"
                    && let UseTree::Path(inner) = &*path.tree
                    && inner.ident == "super"
                {
                    emitter.emit(Diagnostic {
                        rule: TestIsolation::ID,
                        message: format!(
                            "test in `{rel}` imports `super::super::...` — tests must only test their own file (no super climbs)"
                        ),
                    });
                }
                stack.push(&path.tree);
            }
            UseTree::Group(group) => {
                for t in &group.items {
                    stack.push(t);
                }
            }
            _ => {}
        }
    }
}

/// Emit one test-isolation diagnostic per branch under `crate::` whose first
/// segment is neither the current module nor `self`.
///
/// Covers terminal re-exported names (`crate::X`), nested module paths
/// (`crate::module::...`), renames (`crate::X as Y`), and brace groups
/// (`crate::{A, B}`, `crate::{module::X, Y}`). The group case is the
/// "group instead of a module" spelling that previously bypassed the
/// single-shape check; every branch inside the braces is examined.
///
/// `crate::*` (a glob) is intentionally not flagged: a glob has no named
/// first segment, and at the crate root it may bind the current module's own
/// items (e.g. in `lib.rs` / `main.rs`). That is a known residual in the same
/// escape family.
fn check_crate_child(tree: &UseTree, current: &str, rel: &str, emitter: &mut dyn Emitter) {
    match tree {
        UseTree::Path(p) => {
            if p.ident != current && p.ident != "self" {
                emitter.emit(Diagnostic {
                    rule: TestIsolation::ID,
                    message: format!(
                        "test in `{rel}` imports `crate::{}::...` — tests must only test their own file",
                        p.ident
                    ),
                });
            }
        }
        UseTree::Name(n) => {
            if n.ident != current && n.ident != "self" {
                emitter.emit(Diagnostic {
                    rule: TestIsolation::ID,
                    message: format!(
                        "test in `{rel}` imports `crate::{}` — tests must only test their own file",
                        n.ident
                    ),
                });
            }
        }
        UseTree::Rename(r) => {
            if r.ident != current && r.ident != "self" {
                emitter.emit(Diagnostic {
                    rule: TestIsolation::ID,
                    message: format!(
                        "test in `{rel}` imports `crate::{}` — tests must only test their own file",
                        r.ident
                    ),
                });
            }
        }
        UseTree::Group(g) => {
            for item in &g.items {
                check_crate_child(item, current, rel, emitter);
            }
        }
        UseTree::Glob(_) => {}
    }
}

#[cfg(test)]
use crate::engine::vec_emitter::VecEmitter;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_cross_module_test_import() {
        let file = syn::parse_file(
            r"
            #[cfg(test)]
            mod tests {
                use crate::other::SomeType;
            }
        ",
        )
        .unwrap();
        let mut em = VecEmitter::default();
        check(&file, "crates/x/src/my_mod.rs", "my_mod", &mut em);
        assert_eq!(em.diagnostics.len(), 1);
        assert_eq!(em.diagnostics[0].rule, "test-isolation");
    }

    #[test]
    fn silent_when_test_imports_same_module() {
        let file = syn::parse_file(
            r"
            #[cfg(test)]
            mod tests {
                use crate::my_mod::SomeType;
            }
        ",
        )
        .unwrap();
        let mut em = VecEmitter::default();
        check(&file, "crates/x/src/my_mod.rs", "my_mod", &mut em);
        assert!(em.diagnostics.is_empty());
    }

    #[test]
    fn silent_when_test_imports_super() {
        let file = syn::parse_file(
            r"
            #[cfg(test)]
            mod tests {
                use super::*;
            }
        ",
        )
        .unwrap();
        let mut em = VecEmitter::default();
        check(&file, "crates/x/src/my_mod.rs", "my_mod", &mut em);
        assert!(em.diagnostics.is_empty());
    }

    #[test]
    fn flags_super_super_in_use_tree() {
        let file = syn::parse_file(
            r"
            #[cfg(test)]
            mod tests {
                use super::super::SomeType;
            }
        ",
        )
        .unwrap();
        let mut em = VecEmitter::default();
        check(&file, "crates/x/src/my_mod.rs", "my_mod", &mut em);
        assert_eq!(em.diagnostics.len(), 1);
        assert_eq!(em.diagnostics[0].rule, "test-isolation");
        assert!(em.diagnostics[0].message.contains("super::super"));
    }

    #[test]
    fn flags_inline_crate_cross_module_in_fn_body() {
        let file = syn::parse_file(
            r"
            #[cfg(test)]
            mod tests {
                #[test]
                fn t() { let _x = crate::other::Thing; }
            }
        ",
        )
        .unwrap();
        let mut em = VecEmitter::default();
        check(&file, "crates/x/src/my_mod.rs", "my_mod", &mut em);
        assert_eq!(em.diagnostics.len(), 1);
        assert_eq!(em.diagnostics[0].rule, "test-isolation");
        assert!(em.diagnostics[0].message.contains("crate::other"));
    }

    #[test]
    fn allows_inline_crate_same_module_in_fn_body() {
        let file = syn::parse_file(
            r"
            #[cfg(test)]
            mod tests {
                #[test]
                fn t() { let _x = crate::my_mod::Thing; }
            }
        ",
        )
        .unwrap();
        let mut em = VecEmitter::default();
        check(&file, "crates/x/src/my_mod.rs", "my_mod", &mut em);
        assert!(em.diagnostics.is_empty());
    }

    #[test]
    fn flags_super_super_in_type_position() {
        let file = syn::parse_file(
            r"
            #[cfg(test)]
            mod tests {
                #[test]
                fn t() { let _x: super::super::Thing = todo!(); }
            }
        ",
        )
        .unwrap();
        let mut em = VecEmitter::default();
        check(&file, "crates/x/src/my_mod.rs", "my_mod", &mut em);
        assert_eq!(em.diagnostics.len(), 1);
        assert_eq!(em.diagnostics[0].rule, "test-isolation");
        assert!(em.diagnostics[0].message.contains("super::super"));
    }

    #[test]
    fn does_not_flag_production_code_outside_test_mod() {
        let file = syn::parse_file(
            r"
            use crate::other::SomeType;

            #[cfg(test)]
            mod tests {
                #[test]
                fn t() { assert!(true); }
            }
        ",
        )
        .unwrap();
        let mut em = VecEmitter::default();
        check(&file, "crates/x/src/my_mod.rs", "my_mod", &mut em);
        assert!(em.diagnostics.is_empty());
    }

    #[test]
    fn no_double_report_for_use_tree_paths() {
        let file = syn::parse_file(
            r"
            #[cfg(test)]
            mod tests {
                use crate::other::Thing;
            }
        ",
        )
        .unwrap();
        let mut em = VecEmitter::default();
        check(&file, "crates/x/src/my_mod.rs", "my_mod", &mut em);
        assert_eq!(em.diagnostics.len(), 1);
    }

    #[test]
    fn flags_both_use_tree_and_inline_independently() {
        let file = syn::parse_file(
            r"
            #[cfg(test)]
            mod tests {
                use super::super::Imported;
                #[test]
                fn t() { let _x: crate::other::Inline = todo!(); }
            }
        ",
        )
        .unwrap();
        let mut em = VecEmitter::default();
        check(&file, "crates/x/src/my_mod.rs", "my_mod", &mut em);
        assert_eq!(em.diagnostics.len(), 2);
        let has_super = em
            .diagnostics
            .iter()
            .any(|v| v.message.contains("super::super"));
        let has_crate = em
            .diagnostics
            .iter()
            .any(|v| v.message.contains("crate::other"));
        assert!(has_super, "missing super::super violation");
        assert!(has_crate, "missing crate::other violation");
    }

    #[test]
    fn flags_super_super_in_use_tree_group() {
        let file = syn::parse_file(
            r"
            #[cfg(test)]
            mod tests {
                use super::super::{TypeA, TypeB};
            }
        ",
        )
        .unwrap();
        let mut em = VecEmitter::default();
        check(&file, "crates/x/src/my_mod.rs", "my_mod", &mut em);
        assert_eq!(em.diagnostics.len(), 1);
        assert!(em.diagnostics[0].message.contains("super::super"));
    }

    #[test]
    fn allows_super_glob_in_use_tree() {
        let file = syn::parse_file(
            r"
            #[cfg(test)]
            mod tests {
                use super::*;
            }
        ",
        )
        .unwrap();
        let mut em = VecEmitter::default();
        check(&file, "crates/x/src/my_mod.rs", "my_mod", &mut em);
        assert!(em.diagnostics.is_empty());
    }

    #[test]
    fn allows_single_super_in_use_tree() {
        let file = syn::parse_file(
            r"
            #[cfg(test)]
            mod tests {
                use super::Foo;
            }
        ",
        )
        .unwrap();
        let mut em = VecEmitter::default();
        check(&file, "crates/x/src/my_mod.rs", "my_mod", &mut em);
        assert!(em.diagnostics.is_empty());
    }

    #[test]
    fn flags_terminal_crate_reexport_in_use() {
        // `use crate::CrossType;` — a crate-root re-export, no module segment.
        // Previously escaped; the first segment after `crate` is cross-module.
        let file = syn::parse_file(
            r"
            #[cfg(test)]
            mod tests {
                use crate::CrossType;
            }
        ",
        )
        .unwrap();
        let mut em = VecEmitter::default();
        check(&file, "crates/x/src/my_mod.rs", "my_mod", &mut em);
        assert_eq!(em.diagnostics.len(), 1);
        assert_eq!(em.diagnostics[0].rule, "test-isolation");
        assert!(em.diagnostics[0].message.contains("crate::CrossType"));
    }

    #[test]
    fn flags_grouped_terminal_crate_reexports() {
        // `use crate::{A, B};` — a brace group of re-exported names.
        // Each cross-module branch is one violation.
        let file = syn::parse_file(
            r"
            #[cfg(test)]
            mod tests {
                use crate::{CrossA, CrossB};
            }
        ",
        )
        .unwrap();
        let mut em = VecEmitter::default();
        check(&file, "crates/x/src/my_mod.rs", "my_mod", &mut em);
        assert_eq!(em.diagnostics.len(), 2);
        assert!(
            em.diagnostics
                .iter()
                .any(|v| v.message.contains("crate::CrossA"))
        );
        assert!(
            em.diagnostics
                .iter()
                .any(|v| v.message.contains("crate::CrossB"))
        );
    }

    #[test]
    fn flags_grouped_module_path_in_use() {
        // `use crate::{other::Thing};` — the grouped spelling of the
        // long-caught `crate::other::Thing`. Previously escaped via braces.
        let file = syn::parse_file(
            r"
            #[cfg(test)]
            mod tests {
                use crate::{other::Thing};
            }
        ",
        )
        .unwrap();
        let mut em = VecEmitter::default();
        check(&file, "crates/x/src/my_mod.rs", "my_mod", &mut em);
        assert_eq!(em.diagnostics.len(), 1);
        assert_eq!(em.diagnostics[0].rule, "test-isolation");
        assert!(em.diagnostics[0].message.contains("crate::other"));
    }

    #[test]
    fn flags_crate_rename_in_use() {
        // `use crate::X as Y;` — rename of a cross-module re-export.
        let file = syn::parse_file(
            r"
            #[cfg(test)]
            mod tests {
                use crate::CrossType as Bound;
            }
        ",
        )
        .unwrap();
        let mut em = VecEmitter::default();
        check(&file, "crates/x/src/my_mod.rs", "my_mod", &mut em);
        assert_eq!(em.diagnostics.len(), 1);
        assert!(em.diagnostics[0].message.contains("crate::CrossType"));
    }

    #[test]
    fn silent_when_group_has_only_self_and_same_module() {
        // `use crate::{self, my_mod::X};` in my_mod.rs — `self` allowed,
        // `my_mod` is the current module — both branches permitted.
        let file = syn::parse_file(
            r"
            #[cfg(test)]
            mod tests {
                use crate::{self, my_mod::X};
            }
        ",
        )
        .unwrap();
        let mut em = VecEmitter::default();
        check(&file, "crates/x/src/my_mod.rs", "my_mod", &mut em);
        assert!(em.diagnostics.is_empty());
    }

    #[test]
    fn seam_tests_filename_does_not_grant_exemption() {
        // The `_seam_tests` filename suffix is NOT a recognized exemption.
        // Inline test modules must obey test-isolation regardless of filename;
        // cross-module integration tests belong in `tests/` (observing the
        // public surface), not in a `src/` file reaching across `crate::`.
        let file = syn::parse_file(
            r"
            #[cfg(test)]
            mod tests {
                use crate::other::SomeType;
            }
        ",
        )
        .unwrap();
        let mut em = VecEmitter::default();
        check(
            &file,
            "crates/x/src/my_mod_seam_tests.rs",
            "my_mod_seam_tests",
            &mut em,
        );
        assert_eq!(em.diagnostics.len(), 1);
        assert_eq!(em.diagnostics[0].rule, "test-isolation");
    }
}
