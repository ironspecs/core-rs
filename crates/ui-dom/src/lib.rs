//! Public crate surface for `ui-dom`.
//!
//! This file represents the crate-level boundary that re-exports the shared
//! DOM types and runtime domains used by app crates. Its purpose is to keep
//! the public `ui-dom` surface small, predictable, and free of app-local
//! policy.
//!
//! Invariants and rules:
//!
//! - this file owns only crate wiring and re-exports
//! - public surface decisions here must reflect the real intended crate
//!   boundary, not temporary migration seams
//! - implementation details should stay in `types/`, `domains/`, crate-private
//!   support modules at `src/`, and the Bevy-specific `bevy/` boundary
//!
//! Advice and code philosophy:
//!
//! - keep the crate surface honest and unsurprising
//! - prefer re-exporting stable shared concepts instead of app-specific helpers
//! - if a symbol does not belong in the long-term public API, do not surface it here
#![forbid(unsafe_code)]
#![forbid(clippy::as_conversions)]
#![forbid(clippy::cast_possible_truncation)]
#![forbid(clippy::cast_possible_wrap)]
#![forbid(clippy::cast_precision_loss)]
#![forbid(clippy::cast_sign_loss)]
#![forbid(clippy::checked_conversions)]

mod bevy;
pub mod domains;
mod element_controls;
mod numeric;
mod render_plugin;
mod resolved_numeric_control;
#[allow(unpredictable_function_pointer_comparisons)]
pub mod types;
mod ui_dom_render_plugin;
mod ui_dom_render_support;
mod ui_dom_text_font_catalog;
mod ui_dom_text_font_extract_state;

pub use domains::*;
pub use render_plugin::*;
pub use types::*;
