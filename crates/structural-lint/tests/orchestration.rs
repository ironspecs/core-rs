//! Integration tests for the lint orchestration across real files.
//!
//! These drive `collect_violations` over synthetic workspaces built under
//! `target/test-fixtures/`. They cover the cross-file / module pipeline that
//! inline unit tests cannot:
//!
//! - `walk -> parse -> ingest -> WorkspaceIndex -> cross_file_impl` (the only
//!   path that builds the index from real files and feeds a workspace-phase
//!   rule — no unit test exercises ingest + `check_workspace` together).
//! - Each file-phase rule wired to a real parsed file through the runner.
//! - The clean-workspace happy path (no false positives).
//!
//! Fixtures are built at runtime so the crate's own `cargo xtask lint` (which
//! prunes `target`) never walks deliberately-violating sources.

mod common;

use common::Workspace;
use structural_lint::collect_violations;

const ROOTS: &[&str] = &["fakecrate"];

#[test]
fn clean_workspace_produces_no_violations() {
    let ws = Workspace::with_files(
        "clean",
        &[
            ("fakecrate/src/lib.rs", "pub mod good;\n"),
            (
                "fakecrate/src/good.rs",
                "pub struct Good;\nimpl Good { pub fn new() -> Self { Self } }\n\
                 #[cfg(test)]\nmod tests { use super::*; #[test] fn t() { assert!(true); } }\n",
            ),
        ],
    );
    let violations = collect_violations(ws.root(), ROOTS);
    assert!(
        violations.is_empty(),
        "expected no violations: {violations:?}"
    );
}

#[test]
fn cross_file_impl_pipeline_fires_from_real_files() {
    // THE key pipeline test: ingest builds the index from real files, then
    // cross_file_impl::check_workspace reads it. The cross_file_impl UNIT test
    // hand-builds the index; only this test exercises ingest + check_workspace.
    // Both files share crate_key "fakecrate" (the walk produces
    // "fakecrate/src/{def,shared}.rs"; crate_key_from_path strips at "/src/").
    // shared.rs is correctly named for Shared so behavior-type-file stays quiet.
    let ws = Workspace::with_files(
        "cross_file_impl",
        &[
            ("fakecrate/src/def.rs", "pub struct Shared;\n"),
            ("fakecrate/src/shared.rs", "impl Shared {}\n"),
        ],
    );
    let violations = collect_violations(ws.root(), ROOTS);
    assert_eq!(violations.len(), 1, "expected exactly 1: {violations:?}");
    assert_eq!(violations[0].rule, "cross-file-impl");
    assert!(violations[0].message.contains("Shared"));
    assert!(violations[0].message.contains("def.rs"));
    assert!(violations[0].message.contains("shared.rs"));
}

#[test]
fn behavior_type_file_fires_for_impl_in_lib() {
    let ws = Workspace::with_files(
        "behavior_type_file",
        &[("fakecrate/src/lib.rs", "pub struct InLib;\nimpl InLib {}\n")],
    );
    let violations = collect_violations(ws.root(), ROOTS);
    assert_eq!(violations.len(), 1, "expected exactly 1: {violations:?}");
    assert_eq!(violations[0].rule, "behavior-type-file");
    assert!(violations[0].message.contains("InLib"));
    assert!(violations[0].message.contains("mod/lib/main"));
}

#[test]
fn behavior_tests_fires_for_untested_behavior() {
    // foo.rs is correctly named for Foo (so behavior-type-file is quiet); the
    // impl has a method but no #[cfg(test)] module -> behavior-tests fires.
    let ws = Workspace::with_files(
        "behavior_tests",
        &[(
            "fakecrate/src/foo.rs",
            "pub struct Foo;\nimpl Foo { pub fn f(&self) {} }\n",
        )],
    );
    let violations = collect_violations(ws.root(), ROOTS);
    assert_eq!(violations.len(), 1, "expected exactly 1: {violations:?}");
    assert_eq!(violations[0].rule, "behavior-tests");
    assert!(violations[0].message.contains("foo.rs"));
}

#[test]
fn empty_tests_fires_for_placeholder_test_module() {
    let ws = Workspace::with_files(
        "empty_tests",
        &[(
            "fakecrate/src/foo.rs",
            "pub struct Foo;\nimpl Foo { pub fn f(&self) {} }\n\
             #[cfg(test)]\nmod tests { #[test] fn empty() {} }\n",
        )],
    );
    let violations = collect_violations(ws.root(), ROOTS);
    assert_eq!(violations.len(), 1, "expected exactly 1: {violations:?}");
    assert_eq!(violations[0].rule, "empty-tests");
}

#[test]
fn test_isolation_fires_for_cross_module_test_import() {
    let ws = Workspace::with_files(
        "test_isolation",
        &[(
            "fakecrate/src/foo.rs",
            "pub struct Foo;\n\
             #[cfg(test)]\nmod tests { use crate::other::Thing; }\n",
        )],
    );
    let violations = collect_violations(ws.root(), ROOTS);
    assert_eq!(violations.len(), 1, "expected exactly 1: {violations:?}");
    assert_eq!(violations[0].rule, "test-isolation");
    assert!(violations[0].message.contains("crate::other"));
}

#[test]
fn violation_supports_debug_clone_eq() {
    // The public Violation type must be debug-printable, cloneable, and
    // comparable so downstream consumers (and these tests) can assert on it.
    let ws = Workspace::with_files(
        "violation_traits",
        &[("fakecrate/src/lib.rs", "pub struct InLib;\nimpl InLib {}\n")],
    );
    let violations = collect_violations(ws.root(), ROOTS);
    assert_eq!(violations.len(), 1);

    let cloned = violations[0].clone();
    assert_eq!(violations[0], cloned); // PartialEq

    let debug = format!("{:?}", violations[0]); // Debug
    assert!(
        debug.contains("behavior-type-file"),
        "Debug output must include the rule id: {debug}",
    );
}
