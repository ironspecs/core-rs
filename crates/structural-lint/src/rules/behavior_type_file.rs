//! Rule `behavior-type-file`: every behavior-bearing type must live in its own
//! file named after the type.
//!
//! This file owns the [`BehaviorTypeFile`] struct, its `impl Rule`, the private
//! `check` free function, and all unit tests for this rule. The rule runs in
//! the file phase.
//!
//! # Local invariants
//!
//! - Rule id is `"behavior-type-file"` — do not rename.
//! - Message strings are stable: changing them changes violation output.
//! - The `seen` `HashSet` keeps dedup per file so duplicate impl blocks on the
//!   same type produce exactly one violation.

use std::collections::HashSet;

use syn::{File, Item};

use crate::engine::context::FileContext;
use crate::engine::diagnostic::Diagnostic;
use crate::engine::emitter::Emitter;
use crate::engine::rule::Rule;
use crate::engine::util::{extract_impl_type_name, to_snake_case};

/// Rule: behavior-bearing types must be in their own snake_case-named file.
pub(crate) struct BehaviorTypeFile;

impl BehaviorTypeFile {
    pub(crate) const ID: &'static str = "behavior-type-file";
}

impl Rule for BehaviorTypeFile {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn check_file(&self, ctx: &FileContext<'_>, emitter: &mut dyn Emitter) {
        check(
            ctx.file,
            ctx.relative_path,
            ctx.filename_stem,
            ctx.is_mod_rs,
            emitter,
        );
    }
}

fn check(file: &File, rel: &str, filename: &str, is_mod: bool, emitter: &mut dyn Emitter) {
    let mut seen: HashSet<String> = HashSet::new();
    for item in &file.items {
        if let Item::Impl(impl_block) = item {
            if let Some(name) = extract_impl_type_name(impl_block) {
                if !seen.insert(name.clone()) {
                    continue;
                }
                let expected = to_snake_case(&name);
                if is_mod {
                    emitter.emit(Diagnostic {
                        rule: BehaviorTypeFile::ID,
                        message: format!(
                            "`{name}` has impl in `{rel}` — behavior-bearing types must not be in mod/lib/main files"
                        ),
                    });
                } else if filename != expected {
                    emitter.emit(Diagnostic {
                        rule: BehaviorTypeFile::ID,
                        message: format!(
                            "`{name}` has impl in `{rel}` — expected file `{expected}.rs`"
                        ),
                    });
                }
            }
        } else if let Item::Trait(item_trait) = item {
            let name = item_trait.ident.to_string();
            if !seen.insert(name.clone()) {
                continue;
            }
            let expected = to_snake_case(&name);
            if is_mod {
                emitter.emit(Diagnostic {
                    rule: BehaviorTypeFile::ID,
                    message: format!(
                        "trait `{name}` in `{rel}` — behavior-bearing types must not be in mod/lib/main files"
                    ),
                });
            } else if filename != expected {
                emitter.emit(Diagnostic {
                    rule: BehaviorTypeFile::ID,
                    message: format!("trait `{name}` in `{rel}` — expected file `{expected}.rs`"),
                });
            }
        }
    }
}

#[cfg(test)]
use crate::engine::vec_emitter::VecEmitter;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_impl_in_mod_file() {
        let file = syn::parse_file("impl Foo { fn bar() {} }").unwrap();
        let mut em = VecEmitter::default();
        check(&file, "crates/x/src/mod.rs", "mod", true, &mut em);
        assert_eq!(em.diagnostics.len(), 1);
        assert_eq!(em.diagnostics[0].rule, "behavior-type-file");
    }

    #[test]
    fn flags_trait_in_mod_file() {
        let file = syn::parse_file("pub trait Foo { }").unwrap();
        let mut em = VecEmitter::default();
        check(&file, "crates/x/src/mod.rs", "mod", true, &mut em);
        assert_eq!(em.diagnostics.len(), 1);
        assert_eq!(em.diagnostics[0].rule, "behavior-type-file");
    }

    #[test]
    fn flags_wrong_filename_for_impl() {
        let file = syn::parse_file("impl MyType { fn x() {} }").unwrap();
        let mut em = VecEmitter::default();
        check(&file, "crates/x/src/other.rs", "other", false, &mut em);
        assert_eq!(em.diagnostics.len(), 1);
        assert!(
            em.diagnostics[0]
                .message
                .contains("expected file `my_type.rs`")
        );
    }

    #[test]
    fn silent_when_filename_matches_impl() {
        let file = syn::parse_file("impl my_type { fn x() {} }").unwrap();
        let mut em = VecEmitter::default();
        check(&file, "crates/x/src/my_type.rs", "my_type", false, &mut em);
        assert!(em.diagnostics.is_empty());
    }
}
