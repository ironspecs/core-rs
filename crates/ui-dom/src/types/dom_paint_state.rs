//! Retained paint-repaint state for one authored DOM node record.
//!
//! This file owns the `DomPaintState` enum and its `impl` blocks. Its purpose
//! is to keep the node's repaint invalidation state — clean, full-layer
//! repaint, or a bounded set of damaged layer-root-local rects — in one honest
//! record without owning paint scheduling or layer traversal policy.
//!
//! Invariants and rules:
//!
//! - this file owns only the `DomPaintState` enum and its `impl` blocks
//! - the damaged-rect bucket is bounded (`ArrayVec` capacity 8); overflowing
//!   damage coalesces to `FullLayerRepaint` at the call site
//! - `None` is the only clean state; every other variant is paint-dirty

use arrayvec::ArrayVec;

#[derive(Clone, Debug, Default, PartialEq)]
pub enum DomPaintState {
    #[default]
    None,
    FullLayerRepaint,
    DamagedRectsRepaint(ArrayVec<crate::types::DomRect, 8>),
}

impl DomPaintState {
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        !matches!(self, Self::None)
    }
}
