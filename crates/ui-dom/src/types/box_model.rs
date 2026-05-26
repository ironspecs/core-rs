//! Shared geometry primitives for authored DOM boxes.
//!
//! This file represents the box-model data used to move layout results into
//! paint, hit testing, and presentation. Its purpose is to keep geometry facts
//! small, explicit, and reusable across the crate without embedding layout
//! policy into the shapes themselves.
//!
//! Invariants and rules:
//!
//! - values here are pure geometry, not business logic
//! - these types must stay cheap to copy and easy to inspect in tests
//! - no Bevy-specific or app-specific meaning should be stored here
//!
//! Advice and code philosophy:
//!
//! - keep geometry data literal and unsurprising
//! - push interpretation upward into layout/paint/composite code
use serde::{Deserialize, Serialize};

use crate::types::primitives::DomRect;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct BoxEdgeInsets {
    pub left_px: f32,
    pub top_px: f32,
    pub right_px: f32,
    pub bottom_px: f32,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct DomBoxModel {
    pub border_box: DomRect,
    pub margin: BoxEdgeInsets,
    pub border: BoxEdgeInsets,
    pub padding: BoxEdgeInsets,
}

impl DomBoxModel {
    /// Creates one geometry-only box model that paint can interpret later.
    ///
    /// This constructor exists so callers can build an explicit box snapshot
    /// without depending on layout internals. It is intended for shared geometry
    /// setup, not for hiding layout policy inside the type.
    #[must_use]
    pub fn new(
        border_box: DomRect,
        margin: BoxEdgeInsets,
        border: BoxEdgeInsets,
        padding: BoxEdgeInsets,
    ) -> Self {
        Self {
            border_box,
            margin,
            border,
            padding,
        }
    }

    /// Provides the zero/default geometry state for nodes without effective layout.
    ///
    /// This is the canonical empty box used before layout has assigned real
    /// geometry or when a node has no effective size. Callers should prefer it
    /// over inventing ad hoc zero rectangles.
    #[must_use]
    pub fn zero() -> Self {
        Self::default()
    }
}
