//! Crate-private frozen retained-layer config for `ui-dom`.
//!
//! This file owns the read-only wrapper that freezes one `LayerConfig` install
//! input so retained-layer owners cannot mutate density after construction. Its
//! purpose is to keep installed copies read-only without inventing a second
//! public configuration concept.
//!
//! Invariants and rules:
//!
//! - this file owns only the `FrozenLayerConfig` struct and its `impl` block
//! - the wrapper is crate-private and read-only after construction
//! - retained-layer density is chosen before install and replaced only by
//!   creating new owners

use super::layer_config::LayerConfig;

use crate::types::PixelDensity;

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
