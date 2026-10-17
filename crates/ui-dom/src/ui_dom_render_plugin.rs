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
        render::RenderPlugin,
        window::WindowPlugin,
    };

    fn noop_render_creation() -> bevy::render::settings::RenderCreation {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::NOOP,
            backend_options: wgpu::BackendOptions {
                noop: wgpu::NoopBackendOptions { enable: true },
                ..Default::default()
            },
            ..Default::default()
        });
        let adapter = bevy::tasks::block_on(
            instance.request_adapter(&wgpu::RequestAdapterOptions::default()),
        )
        .expect("noop adapter should exist");
        let adapter_info = adapter.get_info();
        let (device, queue) =
            bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .expect("noop device should be granted");
        bevy::render::settings::RenderCreation::manual(
            bevy::render::renderer::RenderDevice::from(device),
            bevy::render::renderer::RenderQueue(std::sync::Arc::new(
                bevy::render::renderer::WgpuWrapper::new(queue),
            )),
            bevy::render::renderer::RenderAdapterInfo(bevy::render::renderer::WgpuWrapper::new(
                adapter_info,
            )),
            bevy::render::renderer::RenderAdapter(std::sync::Arc::new(
                bevy::render::renderer::WgpuWrapper::new(adapter),
            )),
            bevy::render::renderer::RenderInstance(std::sync::Arc::new(
                bevy::render::renderer::WgpuWrapper::new(instance),
            )),
        )
    }

    #[test]
    fn plugin_build_installs_runtime_pair() {
        let mut app = App::new();
        app.add_plugins(
            DefaultPlugins
                .build()
                .set(RenderPlugin {
                    render_creation: noop_render_creation(),
                    ..Default::default()
                })
                .disable::<WindowPlugin>(),
        );

        app.add_plugins(UiDomRenderPlugin::new(LayerConfig::new(PixelDensity::X2)));

        assert_eq!(
            app.world().resource::<LayerPool>().pixel_density(),
            PixelDensity::X2
        );
    }
}
