//! Detects test-isolation violations in expression, type, and pattern paths
//! within `#[cfg(test)]` modules.
//!
//! The [`TestPathVisitor`] walks every [`syn::Path`] in the body of a test
//! module and flags two violations:
//!
//! - Paths starting with `crate::<other_module>::...` where `other_module`
//!   is not the current file's module name — a cross-file reference.
//! - Paths starting with `super::super::...` — a climb past the immediate
//!   parent that silently scales with module depth.
//!
//! Paths inside macro-invocation tokens are not reachable by the visitor and
//! are accepted as a known gap. Use-tree paths are handled separately by the
//! use-tree walk in `check_test_isolation`; the `syn::Visit` infrastructure
//! never calls [`visit_path`](syn::visit::Visit::visit_path) for
//! [`syn::UseTree`] nodes, so there is no double-report risk.
//!
//! [`TestPathVisitor`] is a behavior-bearing type: this file owns its
//! definition, its `Visit` impl, and its unit tests. It belongs to the
//! `structural-lint` crate.

use syn::visit::Visit;

/// Walks [`syn::Path`] nodes inside a `#[cfg(test)]` module and collects
/// test-isolation violation messages.
///
/// Construct with [`TestPathVisitor::new`], apply to each item of the test
/// module with [`syn::visit::visit_item`], then drain violations with
/// [`TestPathVisitor::violations`].
pub struct TestPathVisitor {
    /// The current file's module name (stem of the filename, e.g. `my_mod`).
    current_module: String,
    /// Relative path of the file for error messages.
    rel: String,
    /// Collected violation messages.
    violations: Vec<String>,
}

impl TestPathVisitor {
    /// Creates a visitor that flags paths in files whose module name is
    /// `current_module` and reports the file as `rel`.
    pub fn new(current_module: String, rel: String) -> Self {
        Self {
            current_module,
            rel,
            violations: Vec::new(),
        }
    }

    /// Consumes the visitor and returns collected violations.
    pub fn violations(self) -> Vec<String> {
        self.violations
    }
}

impl<'ast> Visit<'ast> for TestPathVisitor {
    fn visit_path(&mut self, path: &'ast syn::Path) {
        let segs = &path.segments;

        // Flag crate::<module>::... where module is not the current file.
        if segs.len() >= 2 && segs[0].ident == "crate" {
            let module = segs[1].ident.to_string();
            if module != self.current_module && module != "self" {
                self.violations.push(format!(
                    "error[test-isolation]: test in `{}` uses `crate::{}::...` — tests must only test their own file",
                    self.rel, module
                ));
            }
        }

        // Flag super::super::... (climb past the immediate parent module).
        if segs.len() >= 2 && segs[0].ident == "super" && segs[1].ident == "super" {
            self.violations.push(format!(
                "error[test-isolation]: test in `{}` imports `super::super::...` — tests must only test their own file (no super climbs)",
                self.rel
            ));
        }

        // Continue into generic arguments and other child paths.
        syn::visit::visit_path(self, path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse source, run the visitor, return violations.
    fn violations_for(source: &str, current_module: &str) -> Vec<String> {
        let file = syn::parse_file(source).expect("parse test source");
        let mut visitor =
            TestPathVisitor::new(current_module.to_owned(), "test_file.rs".to_owned());
        syn::visit::visit_file(&mut visitor, &file);
        visitor.violations()
    }

    // ----- crate:: paths -----

    #[test]
    fn flags_crate_cross_module_path() {
        let v = violations_for("fn f() { crate::other::Thing::new(); }", "my_mod");
        assert_eq!(v.len(), 1);
        assert!(v[0].contains("crate::other"));
    }

    #[test]
    fn allows_crate_same_module_path() {
        let v = violations_for("fn f() { crate::my_mod::Thing::new(); }", "my_mod");
        assert!(v.is_empty());
    }

    #[test]
    fn allows_crate_self_path() {
        let v = violations_for("fn f() { crate::self::Thing::new(); }", "my_mod");
        assert!(v.is_empty());
    }

    #[test]
    fn allows_bare_crate_path() {
        // `crate` alone — single segment, no module check possible.
        let v = violations_for("fn f() { let x = crate; }", "my_mod");
        assert!(v.is_empty());
    }

    #[test]
    fn flags_crate_in_type_position() {
        let v = violations_for(
            "fn f() { let _x: crate::other::Thing = todo!(); }",
            "my_mod",
        );
        assert_eq!(v.len(), 1);
        assert!(v[0].contains("crate::other"));
    }

    // ----- super::super paths -----

    #[test]
    fn flags_super_super_path() {
        let v = violations_for("fn f() { super::super::Thing::new(); }", "my_mod");
        assert_eq!(v.len(), 1);
        assert!(v[0].contains("super::super"));
    }

    #[test]
    fn allows_single_super_path() {
        let v = violations_for("fn f() { let x = super::Thing; }", "my_mod");
        assert!(v.is_empty());
    }

    #[test]
    fn flags_super_super_in_type_position() {
        let v = violations_for(
            "fn f() { let _x: super::super::Thing = todo!(); }",
            "my_mod",
        );
        assert_eq!(v.len(), 1);
        assert!(v[0].contains("super::super"));
    }

    #[test]
    fn flags_triple_super_path() {
        // super::super::super — first two are super::super.
        let v = violations_for("fn f() { super::super::super::Thing::new(); }", "my_mod");
        assert_eq!(v.len(), 1);
        assert!(v[0].contains("super::super"));
    }

    // ----- no false positives -----

    #[test]
    fn no_false_positives_on_plain_paths() {
        let v = violations_for(
            "fn f() {
                let x: std::collections::HashMap<i32, String> = std::collections::HashMap::new();
            }",
            "my_mod",
        );
        assert!(v.is_empty());
    }

    #[test]
    fn no_false_positives_on_same_module_self() {
        let v = violations_for("fn f() { Self::method(); }", "my_mod");
        assert!(v.is_empty());
    }

    // ----- no overlap with use trees -----

    #[test]
    fn visit_path_not_called_for_use_tree_nodes() {
        // A use tree like `use crate::other::Thing;` should NOT produce
        // a path-visitor violation — use trees are not syn::Path nodes.
        // The visitor walks items but visit_item for use items only visits
        // the UseTree, which never calls visit_path.
        let source = "
            mod test_mod {
                use crate::other::Thing;
            }
        ";
        let v = violations_for(source, "my_mod");
        // The visitor should report zero violations for the use tree.
        assert!(
            v.is_empty(),
            "visit_path must not be called for UseTree nodes"
        );
    }

    // ----- structural: constructor returns no violations initially -----

    #[test]
    fn new_visitor_is_empty() {
        let visitor = TestPathVisitor::new("my_mod".to_owned(), "f.rs".to_owned());
        assert!(visitor.violations().is_empty());
    }
}
