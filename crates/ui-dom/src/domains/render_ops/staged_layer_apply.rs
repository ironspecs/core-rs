//! `StagedLayerApply` is the single staged retained-layer repaint payload.
//!
//! This file owns the payload shape only: the owned paint ops plus everything
//! the render world needs to draw them into the correct retained surface. It
//! is the exact unit `RenderOpsHandler` stages and the render-world extract
//! drains once per tick.
//!
//! Strict invariants:
//! - a `StagedLayerApply` moves end-to-end and is never cloned on the
//!   paint-to-executor path.
//! - the lease is the only identity; the payload carries no root attribution,
//!   no validation state, and no revision/sequence/time fields.

use bevy::{asset::Handle, image::Image};

use crate::domains::LayerLease;
use crate::domains::layer_ops::LayerPaintPlan;

/// One staged layer repaint: owned ops plus everything the render world
/// needs to draw them into the right surface. Moves end-to-end; never cloned.
#[derive(Clone, Debug, PartialEq)]
pub struct StagedLayerApply {
    /// Retained layer this repaint targets.
    pub lease: LayerLease,
    /// Resolved surface the repaint draws into.
    pub image_handle: Handle<Image>,
    /// Surface-local origin of this layer's zero point, in image pixels.
    pub layer_zero_in_image_px: [f32; 2],
    /// Owned paint ops produced for this repaint.
    pub plan: LayerPaintPlan,
}
