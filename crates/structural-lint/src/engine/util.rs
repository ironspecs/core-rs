//! Shared path/AST helpers used by rules and the engine.
//!
//! Pure free functions — not behavior-bearing under rule 2. All helpers are
//! `pub(crate)` so they are visible to rules and the engine but not to
//! external callers.
//!
//! # Local invariants
//!
//! - `to_snake_case` implements the exact casing rules documented in the crate
//!   README. Changing its behavior changes rule‑1 violation messages.

use syn::ItemImpl;

/// Convert a `PascalCase` identifier to `snake_case`.
///
/// Consecutive uppercase letters are treated as an acronym (no underscore
/// between them). An uppercase letter after a lowercase letter introduces
/// a word boundary (underscore inserted).
pub(crate) fn to_snake_case(s: &str) -> String {
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

/// Return the parent directory portion of a relative path.
///
/// Returns `""` when there is no directory separator.
pub(crate) fn parent_dir(path: &str) -> &str {
    match path.rsplit_once('/') {
        Some((dir, _)) => dir,
        None => "",
    }
}

/// Derive a crate key from a relative source path.
///
/// Strips everything from the first `/src/` onward; if no `/src/` is found the
/// entire path is returned unmodified.
pub(crate) fn crate_key_from_path(path: &str) -> String {
    if let Some(pos) = path.find("/src/") {
        path[..pos].to_string()
    } else {
        path.to_string()
    }
}

/// Extract the type name from an `impl` block's self type.
///
/// Returns the last path segment of the `self_ty` (e.g. `"Foo"` from
/// `impl Foo` or `impl<T> Foo<T>`). Returns `None` for non-path self types.
pub(crate) fn extract_impl_type_name(impl_block: &ItemImpl) -> Option<String> {
    match &*impl_block.self_ty {
        syn::Type::Path(type_path) => type_path.path.segments.last().map(|s| s.ident.to_string()),
        _ => None,
    }
}

/// Check whether an attribute is `#[cfg(test)]`.
pub(crate) fn is_cfg_test(attr: &syn::Attribute) -> bool {
    if !attr.path().is_ident("cfg") {
        return false;
    }
    if let syn::Meta::List(list) = &attr.meta {
        return list.tokens.to_string().contains("test");
    }
    false
}

// ---------------------------------------------------------------------------
// Tests
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
        let syn::Item::Impl(impl_block) = &file.items[0] else {
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
        let syn::Item::Impl(impl_block) = &file.items[0] else {
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
        let syn::Item::Mod(m) = &file.items[0] else {
            panic!("expected mod");
        };
        assert!(m.attrs.iter().any(is_cfg_test));
    }

    #[test]
    fn rejects_plain_test_attr() {
        let code = "#[test] fn t() {}";
        let file = syn::parse_file(code).unwrap();
        let syn::Item::Fn(f) = &file.items[0] else {
            panic!("expected fn");
        };
        assert!(!f.attrs.iter().any(is_cfg_test));
    }
}
