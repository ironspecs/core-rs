//! Engine modules for the structural linter.
//!
//! This file owns the `pub(crate) mod` declarations for the engine layer.
//! It contains no behavior-bearing types — only module declarations. The
//! engine modules provide walk, parse, context, indexing, emit, report, and
//! orchestration that rules consume.
//!
//! # Local invariants
//!
//! - This file must never contain a `struct`, `enum`, `trait`, or `impl`.
//! - All engine types live in their own snake_case-named files per rule 1.

pub(crate) mod config;
pub(crate) mod context;
pub mod diagnostic;
pub(crate) mod emitter;
pub(crate) mod human_reporter;
pub(crate) mod index;
pub(crate) mod reporter;
pub(crate) mod rule;
pub(crate) mod runner;
pub(crate) mod util;
pub(crate) mod vec_emitter;
pub(crate) mod walk;
