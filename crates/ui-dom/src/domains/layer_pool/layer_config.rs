//! `LayerConfig` is the immutable retained-layer policy captured by one `ui-dom` install.
//!
//! This file exists so retained-layer density policy stays with the retained
//! layer-pool owner boundary instead of hiding inside Bevy plugin wiring.
//!
//! The public install input stays here because both `LayerPool` and
//! `LayerPoolRenderExecutor` freeze copies from the same retained-layer policy.
//! The crate-private frozen wrapper (`FrozenLayerConfig`) lives in its own file
//! and is re-exported here.
//!
//! Strict invariants:
//! - `LayerConfig` is an install input, not a live synchronization channel
//! - retained-layer density must be chosen before install and replaced only by
//!   creating new owners

use bevy::prelude::Resource;

use crate::types::PixelDensity;

pub(crate) use super::frozen_layer_config::FrozenLayerConfig;

/// Immutable retained-layer policy shared across one `ui-dom` GPU install.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Resource)]
pub struct LayerConfig {
    /// Pixel density used for retained surface sizing and text rasterization.
    pub pixel_density: PixelDensity,
}

impl LayerConfig {
    /// Creates one immutable retained-layer policy for a `ui-dom` GPU install.
    #[must_use]
    pub const fn new(pixel_density: PixelDensity) -> Self {
        Self { pixel_density }
    }
}

impl Default for LayerConfig {
    fn default() -> Self {
        Self::new(PixelDensity::X1)
    }
}
