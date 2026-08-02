//! Rule registry — declares all rule modules and the `all_rules()` factory.
//!
//! This file owns the `all_rules()` free function (not behavior-bearing under
//! rule 2) and the `pub(crate) mod` declarations for each rule module. It
//! contains no type definitions. Each rule module holds exactly one `Rule`
//! implementation and its unit tests.
//!
//! # Local invariants
//!
//! - The order in `all_rules()` is significant: file-phase rules are listed
//!   first and run before workspace-phase rules.
//! - Adding a new rule requires adding its module declaration here AND its
//!   `Box::new(...)` to the `all_rules()` vec.
//! - This file must never contain a `struct`, `enum`, `trait`, or `impl`.

pub(crate) mod behavior_tests;
pub(crate) mod behavior_type_file;
pub(crate) mod cross_file_impl;
pub(crate) mod test_isolation;

use crate::engine::rule::Rule;

/// Returns all structural-lint rules registered for a run.
///
/// File-phase rules come first; workspace-phase rules follow at the end.
/// The order within each phase determines the order diagnostics are emitted
/// for a given file.
pub(crate) fn all_rules() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(behavior_type_file::BehaviorTypeFile),
        Box::new(behavior_tests::BehaviorTests),
        Box::new(test_isolation::TestIsolation),
        Box::new(cross_file_impl::CrossFileImpl),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_four_rules() {
        let rules = all_rules();
        assert_eq!(rules.len(), 4);
    }

    #[test]
    fn rule_ids_match_expected_order() {
        let rules = all_rules();
        let ids: Vec<&str> = rules.iter().map(|r| r.id()).collect();
        assert_eq!(
            ids,
            vec![
                "behavior-type-file",
                "behavior-tests",
                "test-isolation",
                "cross-file-impl"
            ]
        );
    }
}
