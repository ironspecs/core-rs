//! Shared geometry primitives for authored DOM boxes.
//!
//! This file represents the box-model data used to move layout results into
//! paint, hit testing, and presentation. Its purpose is to keep geometry facts
//! small, explicit, and reusable across the crate without embedding layout
//! policy into the shapes themselves. The box snapshot (`DomBoxModel`) lives in
//! its own file and is re-exported here.
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

pub use crate::types::dom_box_model::DomBoxModel;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct BoxEdgeInsets {
    pub left_px: f32,
    pub top_px: f32,
    pub right_px: f32,
    pub bottom_px: f32,
}
