//! Integration tests for the public `run_lint` entry point.
//!
//! Covers the Result contract (Ok on a clean workspace; Err carrying the
//! violation count on violations) through the public API. The exact stdout /
//! stderr text is unit-tested at `engine/human_reporter.rs::render`; these
//! tests verify the wiring of `run_lint -> HumanReporter -> bail!`.

mod common;

use common::Workspace;
use structural_lint::run_lint;

const ROOTS: &[&str] = &["fakecrate"];

#[test]
fn run_lint_clean_workspace_returns_ok() {
    let ws = Workspace::with_files(
        "run_lint_clean",
        &[
            ("fakecrate/src/lib.rs", "pub mod good;\n"),
            (
                "fakecrate/src/good.rs",
                "pub struct Good;\nimpl Good { pub fn new() -> Self { Self } }\n\
                 #[cfg(test)]\nmod tests { use super::*; #[test] fn t() { assert!(true); } }\n",
            ),
        ],
    );
    let result = run_lint(ws.root(), ROOTS);
    assert!(result.is_ok(), "clean workspace must return Ok: {result:?}");
}

#[test]
fn run_lint_violating_workspace_returns_err_with_count() {
    let ws = Workspace::with_files(
        "run_lint_violating",
        &[("fakecrate/src/lib.rs", "pub struct InLib;\nimpl InLib {}\n")],
    );
    let err = run_lint(ws.root(), ROOTS).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("lint: 1 violation(s)"),
        "bail message must carry the count: {msg}",
    );
}

#[test]
fn run_lint_multiple_violations_count_in_error() {
    // Two behavior-bearing types in lib.rs -> two behavior-type-file violations.
    let ws = Workspace::with_files(
        "run_lint_multi",
        &[(
            "fakecrate/src/lib.rs",
            "pub struct A;\nimpl A {}\npub struct B;\nimpl B {}\n",
        )],
    );
    let err = run_lint(ws.root(), ROOTS).unwrap_err();
    assert!(
        format!("{err}").contains("lint: 2 violation(s)"),
        "bail message must carry the count",
    );
}
