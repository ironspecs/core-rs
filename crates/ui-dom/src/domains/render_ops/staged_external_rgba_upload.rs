//! `StagedExternalRgbaUpload` is one staged external RGBA upload payload.
//!
//! This file owns the payload shape only: raw RGBA bytes plus the resolved
//! surface and extent they are written into. It is the external-policy upload
//! counterpart to `StagedLayerApply`, staged through the same handler and
//! drained by the same extract.
//!
//! Strict invariants:
//! - the payload carries raw bytes and resolved handles only; no validation
//!   state, no root attribution, and no revision/sequence/time fields.
//! - uploads are never deferred by queue policy (unlike applies).

use bevy::{asset::Handle, image::Image, math::UVec2};

use crate::domains::LayerLease;

/// One staged external RGBA upload: resolved surface, extent, and raw bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct StagedExternalRgbaUpload {
    /// Retained layer this upload targets.
    pub lease: LayerLease,
    /// Resolved surface the bytes are written into.
    pub image_handle: Handle<Image>,
    /// Pixel extent of the uploaded image.
    pub image_extent_px: UVec2,
    /// Owned RGBA bytes (4 bytes per pixel, tightly packed).
    pub rgba_bytes: Vec<u8>,
}
