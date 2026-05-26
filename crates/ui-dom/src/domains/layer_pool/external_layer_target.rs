//! Public retained image capability for caller-rendered DOM layers.
//!
//! This type gives host code a render target without exposing `BevyLayer` or
//! layer-pool bookkeeping internals.

use bevy::{asset::Handle, image::Image, math::UVec2};

use super::LayerLease;

/// Caller-renderable retained image for one live DOM layer.
#[derive(Clone, Debug, PartialEq)]
pub struct ExternalLayerTarget {
    /// Validated retained layer lease for the target image.
    pub lease: LayerLease,
    /// Bevy image handle composited by `ui-dom` for this layer.
    pub image_handle: Handle<Image>,
    /// Current texture extent of `image`.
    pub image_extent_px: UVec2,
    /// Pixel offset from image top-left to layer-local origin.
    pub origin_offset_px: [f32; 2],
}
