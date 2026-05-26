//! Small geometric primitives shared across `ui-dom`.
//!
//! This file represents the tiny point and rectangle shapes used by layout,
//! paint, hit testing, and presentation. Its purpose is to keep the crate on
//! one consistent set of lightweight primitives instead of retyping geometry in
//! each domain, while allowing the public API to stay simpler than the
//! internal implementation helpers.
//!
//! Invariants and rules:
//!
//! - values here are plain geometry, not interpreted layout policy
//! - these types should stay copyable and test-friendly
//! - no external runtime ownership should be encoded here
//! - point helpers may be crate-private even when rects stay public
//!
//! Advice and code philosophy:
//!
//! - keep these primitives minimal and unsurprising
//! - push richer behavior into the domains that consume them
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct PointPx {
    pub x_px: f32,
    pub y_px: f32,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct DomRect {
    pub left_px: f32,
    pub top_px: f32,
    pub width_px: f32,
    pub height_px: f32,
}
