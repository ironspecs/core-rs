//! Structural linter that enforces four invariant rules on `.rs` files under
//! caller-supplied lint roots.
//!
//! This file owns the public façade, all four check functions, and the file
//! walking logic. It contains no behavior-bearing types — only free functions
//! and the plain-data [`Violation`] struct.
//!
//! # Rules
//!
//! 1. **behavior-type-file** — every behavior-bearing type (`struct` with
//!    `impl`, `enum` with `impl`, or `trait`) lives in its own file named
//!    after the type.
//! 2. **behavior-tests** — files with behavior methods must have a
//!    `#[cfg(test)]` module with at least one non-empty test.
//! 3. **empty-tests** — a test module with no non-empty tests is flagged.
//! 4. **test-isolation** — tests only reference their own module. Checked in
//!    two layers: (a) `use` trees are walked for `crate::<other>` and
//!    `super::super` climbs; (b) a [`syn::visit::Visit`] pass walks every
//!    expression, type, and pattern [`syn::Path`] for the same two patterns
//!    outside of `use` statements.
//! 5. **cross-file-impl** — a type's definition and all its `impl` blocks
//!    must be in the same file.
//!
//! Violations print to stderr tagged `error[<rule>]`. There are no exemptions
//! or suppress attributes.
//!
//! # Public API
//!
//! - [`run_lint`] — parse all `.rs` files under the given roots, run all
//!   checks, print violations to stderr, and bail with the count.
//! - [`collect_violations`] — same pass, returns structured data.
//! - [`Violation`] — plain data carrying a rule tag and a message.

mod test_path_visitor;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::Result;
use syn::{File, Item, ItemImpl, UseTree};

use test_path_visitor::TestPathVisitor;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A structural invariant violation.
///
/// `rule` is the invariant tag (e.g. `"behavior-type-file"`).
/// `message` is the human-readable description (e.g.
/// `"error[behavior-type-file]: ..."`).
pub struct Violation {
    /// The invariant tag, e.g. `"behavior-type-file"`.
    pub rule: &'static str,
    /// The human-readable violation message.
    pub message: String,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse every `.rs` file under the given `lint_roots` (relative to
/// `workspace_root`), run all checks, and return structured violations.
///
/// An empty `lint_roots` slice results in zero files walked and zero
/// violations.
///
/// The caller controls which directories are linted; vendored, third-party,
/// and generated directories are omitted from the roots list.
#[must_use]
pub fn collect_violations(workspace_root: &Path, lint_roots: &[&str]) -> Vec<Violation> {
    let mut violations: Vec<Violation> = Vec::new();

    // Two-pass state for cross-file-impl.
    let mut type_defs: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut type_impls: HashMap<String, HashMap<String, Vec<String>>> = HashMap::new();
    let mut parsed_files: Vec<(String, String, bool, File)> = Vec::new();

    for rel_path in walk_rs_files_at_roots(workspace_root, lint_roots) {
        let Ok(content) = std::fs::read_to_string(workspace_root.join(&rel_path)) else {
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

        // Collect type definitions (pass 1).
        for item in &file.items {
            match item {
                Item::Struct(s) => {
                    type_defs
                        .entry(crate_key.clone())
                        .or_default()
                        .entry(s.ident.to_string())
                        .or_insert(rel_str.clone());
                }
                Item::Enum(e) => {
                    type_defs
                        .entry(crate_key.clone())
                        .or_default()
                        .entry(e.ident.to_string())
                        .or_insert(rel_str.clone());
                }
                Item::Trait(t) => {
                    type_defs
                        .entry(crate_key.clone())
                        .or_default()
                        .entry(t.ident.to_string())
                        .or_insert(rel_str.clone());
                }
                Item::Impl(impl_block) => {
                    if let Some(name) = extract_impl_type_name(impl_block) {
                        type_impls
                            .entry(crate_key.clone())
                            .or_default()
                            .entry(name)
                            .or_default()
                            .push(rel_str.clone());
                    }
                }
                _ => {}
            }
        }

        parsed_files.push((rel_str, filename, is_mod, file));
    }

    // Phase 2: checks that need the cross-file data.
    check_cross_file_impls(&type_defs, &type_impls, &mut violations);

    // Phase 3: per-file checks.
    for (rel, filename, is_mod, file) in &parsed_files {
        check_behavior_type_file_naming(file, rel, filename, *is_mod, &mut violations);
        check_behavior_tests(file, rel, &mut violations);
        check_test_isolation(file, rel, filename, &mut violations);
    }

    violations
}

/// Convenience wrapper over [`collect_violations`]: prints violations to stderr
/// and returns an error with the count when any exist. Prints `lint: no
/// violations` to stdout on success.
pub fn run_lint(workspace_root: &Path, lint_roots: &[&str]) -> Result<()> {
    let violations = collect_violations(workspace_root, lint_roots);

    if violations.is_empty() {
        println!("lint: no violations");
        return Ok(());
    }

    for v in &violations {
        eprintln!("error[{}]: {}", v.rule, v.message);
    }

    anyhow::bail!("lint: {} violation(s)", violations.len());
}

// ---------------------------------------------------------------------------
// Path walking — caller-supplied lint roots
// ---------------------------------------------------------------------------

fn walk_rs_files_at_roots(workspace_root: &Path, lint_roots: &[&str]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for root_name in lint_roots {
        let root_dir = workspace_root.join(root_name);
        if !root_dir.is_dir() {
            continue;
        }
        collect_rs_files(&root_dir, root_name, &mut files);
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn crate_key_from_path(path: &str) -> String {
    if let Some(pos) = path.find("/src/") {
        path[..pos].to_string()
    } else {
        path.to_string()
    }
}

fn parent_dir(path: &str) -> &str {
    match path.rsplit_once('/') {
        Some((dir, _)) => dir,
        None => "",
    }
}

fn to_snake_case(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut result = String::new();
    for (i, &ch) in chars.iter().enumerate() {
        if ch.is_uppercase() {
            if i > 0 && chars[i - 1].is_lowercase() {
                result.push('_');
            }
            result.extend(ch.to_lowercase());
        } else {
            result.push(ch);
        }
    }
    result
}

fn extract_impl_type_name(impl_block: &ItemImpl) -> Option<String> {
    match &*impl_block.self_ty {
        syn::Type::Path(type_path) => type_path.path.segments.last().map(|s| s.ident.to_string()),
        _ => None,
    }
}

fn is_cfg_test(attr: &syn::Attribute) -> bool {
    if !attr.path().is_ident("cfg") {
        return false;
    }
    if let syn::Meta::List(list) = &attr.meta {
        return list.tokens.to_string().contains("test");
    }
    false
}

// ---------------------------------------------------------------------------
// Rule 1: behavior-type-file
// ---------------------------------------------------------------------------

fn check_behavior_type_file_naming(
    file: &File,
    rel: &str,
    filename: &str,
    is_mod: bool,
    violations: &mut Vec<Violation>,
) {
    let mut seen: HashSet<String> = HashSet::new();
    for item in &file.items {
        if let Item::Impl(impl_block) = item {
            if let Some(name) = extract_impl_type_name(impl_block) {
                if !seen.insert(name.clone()) {
                    continue;
                }
                let expected = to_snake_case(&name);
                if is_mod {
                    violations.push(Violation {
                        rule: "behavior-type-file",
                        message: format!(
                            "`{name}` has impl in `{rel}` — behavior-bearing types must not be in mod/lib/main files"
                        ),
                    });
                } else if filename != expected {
                    violations.push(Violation {
                        rule: "behavior-type-file",
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
                violations.push(Violation {
                    rule: "behavior-type-file",
                    message: format!(
                        "trait `{name}` in `{rel}` — behavior-bearing types must not be in mod/lib/main files"
                    ),
                });
            } else if filename != expected {
                violations.push(Violation {
                    rule: "behavior-type-file",
                    message: format!("trait `{name}` in `{rel}` — expected file `{expected}.rs`"),
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Rule 2 + 3: behavior-tests + empty-tests
// ---------------------------------------------------------------------------

fn check_behavior_tests(file: &File, rel: &str, violations: &mut Vec<Violation>) {
    let has_behavior_methods = file.items.iter().any(|item| {
        if let Item::Impl(impl_block) = item {
            impl_block
                .items
                .iter()
                .any(|item| matches!(item, syn::ImplItem::Fn(_)))
        } else if let Item::Trait(item_trait) = item {
            item_trait
                .items
                .iter()
                .any(|ti| matches!(ti, syn::TraitItem::Fn(fn_item) if fn_item.default.is_some()))
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
        violations.push(Violation {
            rule: "behavior-tests",
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
        violations.push(Violation {
            rule: "empty-tests",
            message: format!("`{rel}` has a test module with no meaningful test functions"),
        });
    }
}

// ---------------------------------------------------------------------------
// Rule 4: test-isolation
// ---------------------------------------------------------------------------

fn check_test_isolation(file: &File, rel: &str, filename: &str, violations: &mut Vec<Violation>) {
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
                        check_use_tree(&use_item.tree, filename, rel, violations);
                    }
                }

                // Layer (b): walk every syn::Path in expression, type,
                // and pattern position for the same two patterns.
                let mut visitor = TestPathVisitor::new(filename.to_owned(), rel.to_owned());
                for inner in items {
                    syn::visit::visit_item(&mut visitor, inner);
                }
                for v in visitor.violations() {
                    // TestPathVisitor produces messages with the "error[...]: "
                    // prefix; extract rule from it.
                    violations.push(violation_from_path_msg(&v));
                }
            }
        }
    }
}

/// Extract a [`Violation`] from a `TestPathVisitor` message string.
///
/// The message format is `"error[test-isolation]: <rest>"` — the rule is
/// always `"test-isolation"` for path-visitor violations.
fn violation_from_path_msg(msg: &str) -> Violation {
    let message = msg
        .strip_prefix("error[test-isolation]: ")
        .unwrap_or(msg)
        .to_string();
    Violation {
        rule: "test-isolation",
        message,
    }
}

fn check_use_tree(tree: &UseTree, current: &str, rel: &str, violations: &mut Vec<Violation>) {
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
                        violations.push(Violation {
                            rule: "test-isolation",
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
                    violations.push(Violation {
                        rule: "test-isolation",
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

// ---------------------------------------------------------------------------
// Rule 5: cross-file-impl
// ---------------------------------------------------------------------------

fn check_cross_file_impls(
    type_defs: &HashMap<String, HashMap<String, String>>,
    type_impls: &HashMap<String, HashMap<String, Vec<String>>>,
    violations: &mut Vec<Violation>,
) {
    for (crate_key, defs) in type_defs {
        if let Some(impls) = type_impls.get(crate_key) {
            for (type_name, impl_files) in impls {
                if let Some(def_file) = defs.get(type_name) {
                    for impl_file in impl_files {
                        if impl_file != def_file && parent_dir(impl_file) == parent_dir(def_file) {
                            violations.push(Violation {
                                rule: "cross-file-impl",
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

// ---------------------------------------------------------------------------
// Tests (this file's contract: all check functions work correctly)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- snake_case ----

    #[test]
    fn snake_case_basic() {
        assert_eq!(to_snake_case("AccountId"), "account_id");
        assert_eq!(to_snake_case("ClientId"), "client_id");
    }

    #[test]
    fn snake_case_acronym() {
        assert_eq!(to_snake_case("OAuthError"), "oauth_error");
        assert_eq!(to_snake_case("ApiError"), "api_error");
    }

    #[test]
    fn snake_case_single_word() {
        assert_eq!(to_snake_case("Client"), "client");
        assert_eq!(to_snake_case("Foo"), "foo");
    }

    // ---- extract_impl_type_name ----

    #[test]
    fn extracts_simple_path() {
        let code = "impl MyType { fn doit(&self) {} }";
        let file = syn::parse_file(code).unwrap();
        let Item::Impl(impl_block) = &file.items[0] else {
            panic!("expected impl");
        };
        assert_eq!(
            extract_impl_type_name(impl_block),
            Some("MyType".to_owned())
        );
    }

    #[test]
    fn extracts_generic_last_segment() {
        let code = "impl<T> Foo<T> { fn bar() {} }";
        let file = syn::parse_file(code).unwrap();
        let Item::Impl(impl_block) = &file.items[0] else {
            panic!("expected impl");
        };
        assert_eq!(extract_impl_type_name(impl_block), Some("Foo".to_owned()));
    }

    // ---- crate_key_from_path ----

    #[test]
    fn crate_key_from_src_path() {
        assert_eq!(
            crate_key_from_path("crates/structural-lint/src/foo.rs"),
            "crates/structural-lint"
        );
    }

    #[test]
    fn crate_key_no_src_uses_full_path() {
        assert_eq!(
            crate_key_from_path("lib/desktop_dist.rs"),
            "lib/desktop_dist.rs"
        );
    }

    // ---- parent_dir ----

    #[test]
    fn parent_dir_standard() {
        assert_eq!(
            parent_dir("crates/structural-lint/src/foo.rs"),
            "crates/structural-lint/src"
        );
    }

    #[test]
    fn parent_dir_no_separator() {
        assert_eq!(parent_dir("lib"), "");
    }

    // ---- is_cfg_test ----

    #[test]
    fn detects_cfg_test_attr() {
        let code = "#[cfg(test)] mod tests { }";
        let file = syn::parse_file(code).unwrap();
        let Item::Mod(m) = &file.items[0] else {
            panic!("expected mod");
        };
        assert!(m.attrs.iter().any(is_cfg_test));
    }

    #[test]
    fn rejects_plain_test_attr() {
        let code = "#[test] fn t() {}";
        let file = syn::parse_file(code).unwrap();
        let Item::Fn(f) = &file.items[0] else {
            panic!("expected fn");
        };
        assert!(!f.attrs.iter().any(is_cfg_test));
    }

    // ---- behavior-type-file: impl in mod file flagged ----

    #[test]
    fn flags_impl_in_mod_file() {
        let code = "impl Foo { fn bar() {} }";
        let file = syn::parse_file(code).unwrap();
        let mut violations = Vec::new();
        check_behavior_type_file_naming(&file, "crates/x/src/mod.rs", "mod", true, &mut violations);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, "behavior-type-file");
    }

    #[test]
    fn flags_trait_in_mod_file() {
        let code = "pub trait Foo { }";
        let file = syn::parse_file(code).unwrap();
        let mut violations = Vec::new();
        check_behavior_type_file_naming(&file, "crates/x/src/mod.rs", "mod", true, &mut violations);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, "behavior-type-file");
    }

    // ---- behavior-type-file: wrong filename ----

    #[test]
    fn flags_wrong_filename_for_impl() {
        let code = "impl MyType { fn x() {} }";
        let file = syn::parse_file(code).unwrap();
        let mut violations = Vec::new();
        check_behavior_type_file_naming(
            &file,
            "crates/x/src/other.rs",
            "other",
            false,
            &mut violations,
        );
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("expected file `my_type.rs`"));
    }

    // ---- behavior-type-file: correct filename silent ----

    #[test]
    fn silent_when_filename_matches_impl() {
        let code = "impl my_type { fn x() {} }";
        let file = syn::parse_file(code).unwrap();
        let mut violations = Vec::new();
        check_behavior_type_file_naming(
            &file,
            "crates/x/src/my_type.rs",
            "my_type",
            false,
            &mut violations,
        );
        assert!(violations.is_empty());
    }

    // ---- behavior-tests ----

    #[test]
    fn flags_behavior_file_without_test_module() {
        let code = "impl Foo { fn bar(&self) {} }";
        let file = syn::parse_file(code).unwrap();
        let mut violations = Vec::new();
        check_behavior_tests(&file, "crates/x/src/foo.rs", &mut violations);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, "behavior-tests");
    }

    #[test]
    fn silent_when_behavior_file_has_non_empty_tests() {
        let code = r"
            impl Foo { fn bar(&self) {} }
            #[cfg(test)]
            mod tests {
                #[test]
                fn it_works() { assert!(true); }
            }
        ";
        let file = syn::parse_file(code).unwrap();
        let mut violations = Vec::new();
        check_behavior_tests(&file, "crates/x/src/foo.rs", &mut violations);
        assert!(violations.is_empty());
    }

    #[test]
    fn flags_empty_test_module() {
        let code = r"
            impl Foo { fn bar(&self) {} }
            #[cfg(test)]
            mod tests {
                #[test]
                fn empty() {}
            }
        ";
        let file = syn::parse_file(code).unwrap();
        let mut violations = Vec::new();
        check_behavior_tests(&file, "crates/x/src/foo.rs", &mut violations);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, "empty-tests");
    }

    // ---- test-isolation ----

    #[test]
    fn flags_cross_module_test_import() {
        let code = r"
            #[cfg(test)]
            mod tests {
                use crate::other::SomeType;
            }
        ";
        let file = syn::parse_file(code).unwrap();
        let mut violations = Vec::new();
        check_test_isolation(&file, "crates/x/src/my_mod.rs", "my_mod", &mut violations);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, "test-isolation");
    }

    #[test]
    fn silent_when_test_imports_same_module() {
        let code = r"
            #[cfg(test)]
            mod tests {
                use crate::my_mod::SomeType;
            }
        ";
        let file = syn::parse_file(code).unwrap();
        let mut violations = Vec::new();
        check_test_isolation(&file, "crates/x/src/my_mod.rs", "my_mod", &mut violations);
        assert!(violations.is_empty());
    }

    #[test]
    fn silent_when_test_imports_super() {
        let code = r"
            #[cfg(test)]
            mod tests {
                use super::*;
            }
        ";
        let file = syn::parse_file(code).unwrap();
        let mut violations = Vec::new();
        check_test_isolation(&file, "crates/x/src/my_mod.rs", "my_mod", &mut violations);
        assert!(violations.is_empty());
    }

    #[test]
    fn flags_super_super_in_use_tree() {
        let code = r"
            #[cfg(test)]
            mod tests {
                use super::super::SomeType;
            }
        ";
        let file = syn::parse_file(code).unwrap();
        let mut violations = Vec::new();
        check_test_isolation(&file, "crates/x/src/my_mod.rs", "my_mod", &mut violations);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, "test-isolation");
        assert!(violations[0].message.contains("super::super"));
    }

    #[test]
    fn flags_inline_crate_cross_module_in_fn_body() {
        // Path visitor: `crate::other::Thing` in an expression position.
        let code = r"
            #[cfg(test)]
            mod tests {
                #[test]
                fn t() { let _x = crate::other::Thing; }
            }
        ";
        let file = syn::parse_file(code).unwrap();
        let mut violations = Vec::new();
        check_test_isolation(&file, "crates/x/src/my_mod.rs", "my_mod", &mut violations);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, "test-isolation");
        assert!(violations[0].message.contains("crate::other"));
    }

    #[test]
    fn allows_inline_crate_same_module_in_fn_body() {
        // Path visitor: `crate::my_mod::Thing` — same file, allowed.
        let code = r"
            #[cfg(test)]
            mod tests {
                #[test]
                fn t() { let _x = crate::my_mod::Thing; }
            }
        ";
        let file = syn::parse_file(code).unwrap();
        let mut violations = Vec::new();
        check_test_isolation(&file, "crates/x/src/my_mod.rs", "my_mod", &mut violations);
        assert!(violations.is_empty());
    }

    #[test]
    fn flags_super_super_in_type_position() {
        // Path visitor: `super::super::Thing` as a type annotation in a
        // test function.
        let code = r"
            #[cfg(test)]
            mod tests {
                #[test]
                fn t() { let _x: super::super::Thing = todo!(); }
            }
        ";
        let file = syn::parse_file(code).unwrap();
        let mut violations = Vec::new();
        check_test_isolation(&file, "crates/x/src/my_mod.rs", "my_mod", &mut violations);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, "test-isolation");
        assert!(violations[0].message.contains("super::super"));
    }

    #[test]
    fn does_not_flag_production_code_outside_test_mod() {
        // A `use crate::other::X` at the file top-level (outside any
        // #[cfg(test)] module) is production code — not flagged.
        let code = r"
            use crate::other::SomeType;

            #[cfg(test)]
            mod tests {
                #[test]
                fn t() { assert!(true); }
            }
        ";
        let file = syn::parse_file(code).unwrap();
        let mut violations = Vec::new();
        check_test_isolation(&file, "crates/x/src/my_mod.rs", "my_mod", &mut violations);
        assert!(violations.is_empty());
    }

    #[test]
    fn no_double_report_for_use_tree_paths() {
        // A use tree like `use crate::other::Thing;` inside a test module
        // should produce exactly one violation (from the use-tree walk).
        // The path visitor never sees UseTree nodes, so there is no
        // double-report.
        let code = r"
            #[cfg(test)]
            mod tests {
                use crate::other::Thing;
            }
        ";
        let file = syn::parse_file(code).unwrap();
        let mut violations = Vec::new();
        check_test_isolation(&file, "crates/x/src/my_mod.rs", "my_mod", &mut violations);
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn flags_both_use_tree_and_inline_independently() {
        // If both a use-tree violation and an inline-path violation exist
        // in the same test module, each is reported once.
        let code = r"
            #[cfg(test)]
            mod tests {
                use super::super::Imported;
                #[test]
                fn t() { let _x: crate::other::Inline = todo!(); }
            }
        ";
        let file = syn::parse_file(code).unwrap();
        let mut violations = Vec::new();
        check_test_isolation(&file, "crates/x/src/my_mod.rs", "my_mod", &mut violations);
        assert_eq!(violations.len(), 2);
        let has_super = violations
            .iter()
            .any(|v| v.message.contains("super::super"));
        let has_crate = violations
            .iter()
            .any(|v| v.message.contains("crate::other"));
        assert!(has_super, "missing super::super violation");
        assert!(has_crate, "missing crate::other violation");
    }

    #[test]
    fn flags_super_super_in_use_tree_group() {
        // `use super::super::{a, b}` inside a test module — flagged.
        let code = r"
            #[cfg(test)]
            mod tests {
                use super::super::{TypeA, TypeB};
            }
        ";
        let file = syn::parse_file(code).unwrap();
        let mut violations = Vec::new();
        check_test_isolation(&file, "crates/x/src/my_mod.rs", "my_mod", &mut violations);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("super::super"));
    }

    #[test]
    fn allows_super_glob_in_use_tree() {
        // `use super::*` — allowed (single super).
        let code = r"
            #[cfg(test)]
            mod tests {
                use super::*;
            }
        ";
        let file = syn::parse_file(code).unwrap();
        let mut violations = Vec::new();
        check_test_isolation(&file, "crates/x/src/my_mod.rs", "my_mod", &mut violations);
        assert!(violations.is_empty());
    }

    #[test]
    fn allows_single_super_in_use_tree() {
        // `use super::foo` — allowed (only one level of super).
        let code = r"
            #[cfg(test)]
            mod tests {
                use super::Foo;
            }
        ";
        let file = syn::parse_file(code).unwrap();
        let mut violations = Vec::new();
        check_test_isolation(&file, "crates/x/src/my_mod.rs", "my_mod", &mut violations);
        assert!(violations.is_empty());
    }

    // ---- test-isolation: seam-test exemption ----

    #[test]
    fn seam_test_file_exempt_from_test_isolation() {
        // A file stem ending with `_seam_tests` is exempt from
        // test-isolation by rule-3 contract — its entire purpose is
        // cross-module testing.
        let code = r"
            #[cfg(test)]
            mod tests {
                use crate::other::SomeType;
            }
        ";
        let file = syn::parse_file(code).unwrap();
        let mut violations = Vec::new();
        check_test_isolation(
            &file,
            "crates/x/src/my_mod_seam_tests.rs",
            "my_mod_seam_tests",
            &mut violations,
        );
        assert!(violations.is_empty());
    }

    #[test]
    fn stem_containing_but_not_ending_with_seam_tests_not_exempt() {
        // A file stem that merely contains `_seam_tests` but does not
        // end with it (e.g. `seam_tests_helper`) is NOT exempt.
        let code = r"
            #[cfg(test)]
            mod tests {
                use crate::other::SomeType;
            }
        ";
        let file = syn::parse_file(code).unwrap();
        let mut violations = Vec::new();
        check_test_isolation(
            &file,
            "crates/x/src/seam_tests_helper.rs",
            "seam_tests_helper",
            &mut violations,
        );
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, "test-isolation");
    }

    // ---- cross-file-impl ----

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
        let mut violations = Vec::new();
        check_cross_file_impls(&type_defs, &type_impls, &mut violations);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule, "cross-file-impl");
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
        let mut violations = Vec::new();
        check_cross_file_impls(&type_defs, &type_impls, &mut violations);
        assert!(violations.is_empty());
    }
}
