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
//! - Files whose stem ends with `_seam_tests` are exempt (rule-3 contract).
//! - `check_use_tree` walks `UseTree` nodes for `crate::<other>` and
//!   `super::super` in import statements.
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
    // Seam-test files (rule-3 contract): files whose stem ends with
    // `_seam_tests` exist solely for cross-module testing and are exempt
    // from test-isolation enforcement. All other rules still apply.
    if filename.ends_with("_seam_tests") {
        return;
    }

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
                // Flag crate::<other_module>::...
                if path.ident == "crate"
                    && let UseTree::Path(inner) = &*path.tree
                {
                    let module = inner.ident.to_string();
                    if module != current && module != "self" {
                        emitter.emit(Diagnostic {
                            rule: TestIsolation::ID,
                            message: format!(
                                "test in `{rel}` imports `crate::{module}::...` — tests must only test their own file"
                            ),
                        });
                    }
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
    fn seam_test_file_exempt_from_test_isolation() {
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
        assert!(em.diagnostics.is_empty());
    }

    #[test]
    fn stem_containing_but_not_ending_with_seam_tests_not_exempt() {
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
            "crates/x/src/seam_tests_helper.rs",
            "seam_tests_helper",
            &mut em,
        );
        assert_eq!(em.diagnostics.len(), 1);
        assert_eq!(em.diagnostics[0].rule, "test-isolation");
    }
}
