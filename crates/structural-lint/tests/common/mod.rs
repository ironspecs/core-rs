//! Shared helpers for integration tests.
//!
//! This file owns ONLY module re-exports — no behavior-bearing types (rule 1).
//! The workspace scratch helper lives in its own correctly-named file.

pub mod workspace;

pub use workspace::Workspace;
