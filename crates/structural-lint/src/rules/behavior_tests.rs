//! Rules `behavior-tests` and `empty-tests`: behavior files must have a test
//! module with at least one non-empty test.
//!
//! This file owns the [`BehaviorTests`] struct, its `impl Rule`, the private
//! `check` free function, and all unit tests. Both rule ids fire from the same
//! check: `"behavior-tests"` when a behavior file has no `#[cfg(test)]` module,
//! and `"empty-tests"` when it has a test module with no non-empty functions.
//!
//! # Local invariants
//!
//! - Rule id for missing tests is `"behavior-tests"`; for empty tests it is
//!   `"empty-tests"`. Do not rename either.
//! - A "behavior method" is any `ImplItem::Fn` in an `impl` block or a
//!   `TraitItem::Fn` with a default body. This definition must not drift.

use syn::{File, ImplItem, Item, TraitItem};

use crate::engine::context::FileContext;
use crate::engine::diagnostic::Diagnostic;
use crate::engine::emitter::Emitter;
use crate::engine::rule::Rule;
use crate::engine::util::is_cfg_test;

/// Rule: behavior files must contain a `#[cfg(test)]` module with real tests.
pub(crate) struct BehaviorTests;

impl BehaviorTests {
    pub(crate) const ID: &'static str = "behavior-tests";
    pub(crate) const EMPTY_ID: &'static str = "empty-tests";
}

impl Rule for BehaviorTests {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn check_file(&self, ctx: &FileContext<'_>, emitter: &mut dyn Emitter) {
        check(ctx.file, ctx.relative_path, emitter);
    }
}

fn check(file: &File, rel: &str, emitter: &mut dyn Emitter) {
    let has_behavior_methods = file.items.iter().any(|item| {
        if let Item::Impl(impl_block) = item {
            impl_block
                .items
                .iter()
                .any(|item| matches!(item, ImplItem::Fn(_)))
        } else if let Item::Trait(item_trait) = item {
            item_trait
                .items
                .iter()
                .any(|ti| matches!(ti, TraitItem::Fn(fn_item) if fn_item.default.is_some()))
        } else {
            false
        }
    });

    if !has_behavior_methods {
        return;
    }

    let has_tests = file
        .items
        .iter()
        .any(|item| matches!(item, Item::Mod(m) if m.attrs.iter().any(is_cfg_test)));

    if !has_tests {
        emitter.emit(Diagnostic {
            rule: BehaviorTests::ID,
            message: format!("`{rel}` has behavior methods but no #[cfg(test)] module"),
        });
        return;
    }

    // Check for at least one non-empty test function.
    let has_meaningful = file.items.iter().any(|item| {
        if let Item::Mod(m) = item {
            if !m.attrs.iter().any(is_cfg_test) {
                return false;
            }
            let Some((_, items)) = &m.content else {
                return false;
            };
            items.iter().any(|inner| match inner {
                Item::Fn(fn_item) => !fn_item.block.stmts.is_empty(),
                _ => false,
            })
        } else {
            false
        }
    });

    if !has_meaningful {
        emitter.emit(Diagnostic {
            rule: BehaviorTests::EMPTY_ID,
            message: format!("`{rel}` has a test module with no meaningful test functions"),
        });
    }
}

#[cfg(test)]
use crate::engine::vec_emitter::VecEmitter;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_behavior_file_without_test_module() {
        let file = syn::parse_file("impl Foo { fn bar(&self) {} }").unwrap();
        let mut em = VecEmitter::default();
        check(&file, "crates/x/src/foo.rs", &mut em);
        assert_eq!(em.diagnostics.len(), 1);
        assert_eq!(em.diagnostics[0].rule, "behavior-tests");
    }

    #[test]
    fn silent_when_behavior_file_has_non_empty_tests() {
        let file = syn::parse_file(
            r"
            impl Foo { fn bar(&self) {} }
            #[cfg(test)]
            mod tests {
                #[test]
                fn it_works() { assert!(true); }
            }
        ",
        )
        .unwrap();
        let mut em = VecEmitter::default();
        check(&file, "crates/x/src/foo.rs", &mut em);
        assert!(em.diagnostics.is_empty());
    }

    #[test]
    fn flags_empty_test_module() {
        let file = syn::parse_file(
            r"
            impl Foo { fn bar(&self) {} }
            #[cfg(test)]
            mod tests {
                #[test]
                fn empty() {}
            }
        ",
        )
        .unwrap();
        let mut em = VecEmitter::default();
        check(&file, "crates/x/src/foo.rs", &mut em);
        assert_eq!(em.diagnostics.len(), 1);
        assert_eq!(em.diagnostics[0].rule, "empty-tests");
    }
}
