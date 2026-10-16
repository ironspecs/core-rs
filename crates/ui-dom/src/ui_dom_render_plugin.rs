//! `UiDomRenderPlugin` is the public Bevy plugin that installs the `ui-dom` GPU paint runtime.
//! This file owns the plugin entry point and its fixed retained-layer policy wiring.
//!
//! The install wiring itself (main runtime, render runtime, and surface material support) stays
//! in `render_plugin`; this file only declares the plugin and hands the chosen `LayerConfig` down.
//!
//! Strict invariants:
//! - one `UiDomRenderPlugin` installs one retained-layer policy for the whole app.
//! - the plugin never stores handles or app-local state; it is a pure install input.

use ::bevy::prelude::{App, Plugin};

use crate::bevy::base_material::load_base_material_shader;
use crate::domains::LayerConfig;
use crate::render_plugin::install_ui_dom_runtime_pair;

#[cfg(test)]
use crate::domains::LayerPool;
#[cfg(test)]
use crate::types::PixelDensity;

/// Installs the direct GPU paint runtime used by `ui-dom`.
#[derive(Clone, Debug)]
pub struct UiDomRenderPlugin {
    layer_config: LayerConfig,
}

impl UiDomRenderPlugin {
    /// Installs `ui-dom` GPU rendering with one fixed retained-layer policy.
    #[must_use]
    pub const fn new(layer_config: LayerConfig) -> Self {
        Self { layer_config }
    }
}

impl Plugin for UiDomRenderPlugin {
    fn build(&self, app: &mut App) {
        load_base_material_shader(app);
        install_ui_dom_runtime_pair(app, self.layer_config);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::bevy::{
        DefaultPlugins,
        prelude::{App, PluginGroup},
        window::WindowPlugin,
    };

    #[test]
    fn plugin_build_installs_runtime_pair() {
        let mut app = App::new();
        app.add_plugins(DefaultPlugins.build().disable::<WindowPlugin>());

        app.add_plugins(UiDomRenderPlugin::new(LayerConfig::new(PixelDensity::X2)));

        assert_eq!(
            app.world().resource::<LayerPool>().pixel_density(),
            PixelDensity::X2
        );
    }
}
