//! Detects direct recursion in function bodies, methods, and trait default
//! methods using `syn::visit::Visit`. A free-function call to itself by a
//! single-segment path is flagged; a method call through `self.foo()` is
//! flagged. Qualified paths and non-self receivers are never flagged.
//!
//! The [`RecursionFinder`] is a behavior-bearing type: this file owns its
//! definition, its `Visit` impl, and its unit tests. It belongs to the
//! `structural-lint` crate.

use syn::visit::Visit;
use syn::{ExprCall, ImplItemFn, TraitItemFn};

/// Detects self-recursion in a single function or method body.
///
/// Construct with [`RecursionFinder::free_fn`] for free functions or
/// [`RecursionFinder::method`] for methods / trait-default methods.
/// Drive by passing the function block to [`syn::visit::visit_block`].
pub struct RecursionFinder {
    /// The name to match.
    name: String,
    /// Whether `self.foo()` is the signature to look for.
    method_mode: bool,
    /// True after the first matching call site is visited.
    found: bool,
}

impl RecursionFinder {
    /// Creates a finder for a free function named `name`.
    /// It flags single-segment path calls `name()`.
    pub fn free_fn(name: String) -> Self {
        Self {
            name,
            method_mode: false,
            found: false,
        }
    }

    /// Creates a finder for a method named `name`.
    /// It flags `self.name()` (`ExprMethodCall`) only.
    pub fn method(name: String) -> Self {
        Self {
            name,
            method_mode: true,
            found: false,
        }
    }

    /// Returns true once a matching recursive call has been found.
    pub fn found(&self) -> bool {
        self.found
    }
}

impl<'ast> Visit<'ast> for RecursionFinder {
    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if self.method_mode {
            // In method mode we do NOT check ExprCall; we check below
            // via visit_expr_method_call. Still recurse into arguments.
            syn::visit::visit_expr_call(self, node);
            return;
        }

        // Free-function mode: check single-segment path `name()`.
        if let syn::Expr::Path(path_expr) = &*node.func
            && path_expr.path.segments.len() == 1
            && path_expr.path.segments[0].ident == self.name
        {
            self.found = true;
        }
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        if self.method_mode && node.method == self.name {
            // Check that the receiver is exactly `self` (ExprPath with
            // single segment "self").
            if let syn::Expr::Path(receiver_path) = &*node.receiver
                && receiver_path.path.segments.len() == 1
                && receiver_path.path.segments[0].ident == "self"
            {
                self.found = true;
            }
        }
        syn::visit::visit_expr_method_call(self, node);
    }
}

/// Shorthand: drive a free-function recursion check against `func`.
pub fn check_free_fn(func: &syn::ItemFn) -> bool {
    let name = func.sig.ident.to_string();
    let mut finder = RecursionFinder::free_fn(name);
    syn::visit::visit_block(&mut finder, &func.block);
    finder.found()
}

/// Shorthand: drive a method recursion check against `method`.
pub fn check_method(method: &ImplItemFn) -> bool {
    let name = method.sig.ident.to_string();
    let mut finder = RecursionFinder::method(name);
    syn::visit::visit_block(&mut finder, &method.block);
    finder.found()
}

/// Shorthand: drive a trait-default method recursion check.
pub fn check_trait_method(method: &TraitItemFn) -> bool {
    let name = method.sig.ident.to_string();
    let Some(default_block) = &method.default else {
        return false;
    };
    let mut finder = RecursionFinder::method(name);
    syn::visit::visit_block(&mut finder, default_block);
    finder.found()
}

#[cfg(test)]
mod tests {
    use super::{RecursionFinder, check_free_fn, check_method, check_trait_method};

    // Helper: parse a single fn item from source.
    fn parse_fn(src: &str) -> syn::ItemFn {
        let file = syn::parse_file(src).expect("parse test source");
        match &file.items[0] {
            syn::Item::Fn(f) => f.clone(),
            _ => panic!("expected fn item"),
        }
    }

    // Helper: parse a single impl block (with one method) from source.
    fn parse_impl_method(src: &str) -> syn::ImplItemFn {
        let file = syn::parse_file(src).expect("parse test source");
        let syn::Item::Impl(impl_block) = &file.items[0] else {
            panic!("expected impl item");
        };
        let syn::ImplItem::Fn(f) = &impl_block.items[0] else {
            panic!("expected impl fn item");
        };
        f.clone()
    }

    // Helper: parse a single trait item fn (with default) from source.
    fn parse_trait_method(src: &str) -> syn::TraitItemFn {
        let file = syn::parse_file(src).expect("parse test source");
        let syn::Item::Trait(trait_item) = &file.items[0] else {
            panic!("expected trait item");
        };
        let syn::TraitItem::Fn(f) = &trait_item.items[0] else {
            panic!("expected trait fn item");
        };
        f.clone()
    }

    // ---------- free function tests ----------

    #[test]
    fn finds_direct_recursion() {
        let func = parse_fn("fn foo() { foo() }");
        assert!(check_free_fn(&func));
    }

    #[test]
    fn no_false_positive_on_qualified_call() {
        let func = parse_fn("fn run() { commands::health::run() }");
        assert!(!check_free_fn(&func));
    }

    #[test]
    fn no_false_positive_on_other_receiver() {
        let func = parse_fn("fn foo() { other.foo() }");
        assert!(!check_free_fn(&func));
    }

    #[test]
    fn free_fn_not_recursive_by_default() {
        let func = parse_fn("fn ok() { println!(\"hi\"); }");
        assert!(!check_free_fn(&func));
    }

    #[test]
    fn finds_nested_free_recursion() {
        let func = parse_fn("fn f() { if true { f() } }");
        assert!(check_free_fn(&func));
    }

    // ---------- method tests ----------

    #[test]
    fn finds_self_method_recursion() {
        let m = parse_impl_method("impl T { fn foo(&self) { self.foo() } }");
        assert!(check_method(&m));
    }

    #[test]
    fn method_ignores_other_receiver() {
        let m = parse_impl_method("impl T { fn foo(&self) { other.foo() } }");
        assert!(!check_method(&m));
    }

    #[test]
    fn method_ignores_free_fn_same_name() {
        // `foo()` inside a method named `foo` — different namespace.
        let m = parse_impl_method("impl T { fn foo(&self) { foo() } }");
        assert!(!check_method(&m));
    }

    #[test]
    fn method_ignores_qualified_path() {
        let m = parse_impl_method("impl T { fn foo(&self) { Self::foo() } }");
        assert!(!check_method(&m));
    }

    #[test]
    fn method_not_recursive_by_default() {
        let m = parse_impl_method("impl T { fn bar(&self) { 1 + 1; } }");
        assert!(!check_method(&m));
    }

    // ---------- trait-default method tests ----------

    #[test]
    fn finds_trait_default_self_recursion() {
        let t = parse_trait_method("trait Tr { fn foo(&self) { self.foo() } }");
        assert!(check_trait_method(&t));
    }

    #[test]
    fn trait_method_no_default_is_not_recursive() {
        let t = parse_trait_method("trait Tr { fn foo(&self); }");
        assert!(!check_trait_method(&t));
    }

    #[test]
    fn trait_method_ignores_other_receiver() {
        let t = parse_trait_method("trait Tr { fn foo(&self) { other.foo() } }");
        assert!(!check_trait_method(&t));
    }

    // ---------- constructor tests ----------

    #[test]
    fn free_fn_constructor() {
        let finder = RecursionFinder::free_fn("run".to_owned());
        assert!(!finder.found());
    }

    #[test]
    fn method_constructor() {
        let finder = RecursionFinder::method("tick".to_owned());
        assert!(!finder.found());
    }
}
