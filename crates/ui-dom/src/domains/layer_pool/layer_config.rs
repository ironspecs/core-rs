//! `LayerConfig` is the immutable retained-layer policy captured by one `ui-dom` install.
//!
//! This file exists so retained-layer density policy stays with the retained
//! layer-pool owner boundary instead of hiding inside Bevy plugin wiring.
//!
//! The public install input stays here because both `LayerPool` and
//! `LayerPoolRenderExecutor` freeze copies from the same retained-layer policy.
//! The crate-private frozen wrapper keeps installed copies read-only after
//! construction without inventing a second public configuration concept.
//!
//! Strict invariants:
//! - `LayerConfig` is an install input, not a live synchronization channel
//! - `FrozenLayerConfig` is crate-private and read-only after construction
//! - retained-layer density must be chosen before install and replaced only by
//!   creating new owners

use bevy::prelude::Resource;

use crate::types::PixelDensity;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FrozenLayerConfig(LayerConfig);

impl FrozenLayerConfig {
    #[must_use]
    pub(crate) const fn new(layer_config: LayerConfig) -> Self {
        Self(layer_config)
    }

    #[must_use]
    pub(crate) const fn pixel_density(self) -> PixelDensity {
        self.0.pixel_density
    }
}
