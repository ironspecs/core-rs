//! Stable `ui-dom` facade for Bevy render integration and mixed runtime wiring.
//!
//! The Bevy-specific implementation lives under `src/bevy/`, while this file
//! keeps the support types plus the mixed install/runtime wiring that still
//! knows about both Bevy and `ui-dom` domain owners. Retained paint work and
//! retained surface-order policy live under `domains/layer_pool/`, and
//! icon-atlas schema validation stays under `domains/icon_atlas.rs`.

use std::collections::HashSet;

use ::bevy::{
    app::SubApp,
    asset::{AssetApp, AssetEventSystems, AssetId, AssetServer, Handle},
    prelude::*,
    render::{
        ExtractSchedule, Render, RenderApp, RenderSystems,
        render_asset::{RenderAssetPlugin, prepare_assets},
        texture::GpuImage,
    },
    sprite_render::{
        EntitiesNeedingSpecialization, MeshMaterial2d, PreparedMaterial2d,
        check_entities_needing_specialization,
    },
};

use crate::bevy::base_material::load_base_material_shader;
use crate::bevy::font_assets::UiDomFontAssetLoader;
use crate::bevy::icon_assets::{UiIconAtlasManifestAsset, UiIconAtlasManifestAssetLoader};
use crate::bevy::ordered_surface_render::install_ui_dom_ordered_surface_render_runtime;
use crate::domains::{
    LayerConfig, LayerPool, LayerPoolRenderExecutor, LayerPoolRenderJobQueue,
    apply_layer_pool_render_work, extract_layer_pool_render_work, text_layout,
    text_layout::TextFontFamilyId,
};

pub(crate) use crate::bevy::base_material::UiDomBaseMaterial;
pub(crate) use crate::bevy::font_assets::UiDomFontAsset;

#[cfg(test)]
use crate::types::PixelDensity;

/// Reports the GPU paint runtime support installed for `ui-dom`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Resource)]
pub struct UiDomRenderSupport {
    /// True when `ui-dom` has a direct GPU layer-paint executor wired up.
    pub direct_gpu_executor_available: bool,
}

impl UiDomRenderSupport {
    #[must_use]
    pub fn direct_gpu() -> Self {
        Self {
            direct_gpu_executor_available: true,
        }
    }
}

#[derive(Clone, Debug, Resource)]
pub(crate) struct UiDomTextFontCatalog {
    ui_sans_handles: Vec<Handle<UiDomFontAsset>>,
}

impl UiDomTextFontCatalog {
    #[must_use]
    pub(crate) fn required_asset_ids_by_family(
        &self,
    ) -> std::collections::HashMap<TextFontFamilyId, Vec<AssetId<UiDomFontAsset>>> {
        std::collections::HashMap::from([(
            TextFontFamilyId::UiSans,
            self.ui_sans_handles.iter().map(Handle::id).collect(),
        )])
    }

    #[must_use]
    pub(crate) fn ui_sans_handles(&self) -> &[Handle<UiDomFontAsset>] {
        &self.ui_sans_handles
    }
}

#[derive(Default, Resource)]
pub(crate) struct UiDomTextFontExtractState {
    extracted_font_asset_ids: HashSet<AssetId<UiDomFontAsset>>,
}

impl UiDomTextFontExtractState {
    #[must_use]
    pub(crate) fn already_extracted(&self, asset_id: AssetId<UiDomFontAsset>) -> bool {
        self.extracted_font_asset_ids.contains(&asset_id)
    }

    pub(crate) fn mark_extracted(&mut self, asset_id: AssetId<UiDomFontAsset>) {
        self.extracted_font_asset_ids.insert(asset_id);
    }
}

#[derive(Resource)]
struct UiDomMainRuntimeInstalled;

#[derive(Resource)]
struct UiDomRenderRuntimeInstalled;

/// Installs one `ui-dom` runtime pair from a single retained-layer policy.
pub fn install_ui_dom_runtime_pair(app: &mut App, layer_config: LayerConfig) {
    assert!(
        app.get_sub_app(RenderApp).is_some(),
        "install_ui_dom_runtime_pair should run only after the Bevy RenderApp exists",
    );
    install_ui_dom_main_runtime(app, layer_config);
    let render_app = app
        .get_sub_app_mut(RenderApp)
        .expect("render app should exist after the install precondition");
    install_ui_dom_render_runtime(render_app, layer_config);
}

fn install_ui_dom_main_runtime(app: &mut App, layer_config: LayerConfig) {
    app.insert_resource(UiDomRenderSupport::direct_gpu())
        .insert_resource(LayerPool::new(layer_config));
    if app.world().contains_resource::<UiDomMainRuntimeInstalled>() {
        return;
    }
    app.insert_resource(UiDomMainRuntimeInstalled)
        .init_asset::<UiDomFontAsset>()
        .register_asset_loader(UiDomFontAssetLoader)
        .init_asset::<UiIconAtlasManifestAsset>()
        .register_asset_loader(UiIconAtlasManifestAssetLoader)
        .init_resource::<UiDomTextFontExtractState>()
        .init_resource::<LayerPoolRenderJobQueue>()
        .init_resource::<crate::domains::RenderOpsHandlers>()
        .add_systems(Startup, load_ui_dom_text_fonts);
    install_ui_dom_surface_material_support(app);
}

fn install_ui_dom_render_runtime(render_app: &mut SubApp, layer_config: LayerConfig) {
    render_app.insert_resource(LayerPoolRenderExecutor::new(layer_config));
    if render_app
        .world()
        .contains_resource::<UiDomRenderRuntimeInstalled>()
    {
        return;
    }
    render_app
        .insert_resource(UiDomRenderRuntimeInstalled)
        .add_systems(ExtractSchedule, extract_layer_pool_render_work)
        .add_systems(
            Render,
            trim_layer_pool_text_render_maintenance
                .in_set(RenderSystems::PrepareResources)
                .after(prepare_assets::<GpuImage>),
        )
        .add_systems(
            Render,
            apply_layer_pool_render_work
                .in_set(RenderSystems::PrepareResources)
                .after(trim_layer_pool_text_render_maintenance),
        );
    install_ui_dom_ordered_surface_render_runtime(render_app);
}

fn install_ui_dom_surface_material_support(app: &mut App) {
    app.init_asset::<UiDomBaseMaterial>()
        .init_resource::<EntitiesNeedingSpecialization<UiDomBaseMaterial>>()
        .register_type::<MeshMaterial2d<UiDomBaseMaterial>>()
        .add_plugins(RenderAssetPlugin::<PreparedMaterial2d<UiDomBaseMaterial>>::default())
        .add_systems(
            PostUpdate,
            check_entities_needing_specialization::<UiDomBaseMaterial>.after(AssetEventSystems),
        );
}

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

#[allow(clippy::needless_pass_by_value)]
fn load_ui_dom_text_fonts(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.insert_resource(UiDomTextFontCatalog {
        ui_sans_handles: text_layout::text_render::ui_sans_repo_font_asset_paths()
            .iter()
            .map(|path| asset_server.load(*path))
            .collect(),
    });
}

#[allow(clippy::needless_pass_by_value)]
fn trim_layer_pool_text_render_maintenance(mut executor: ResMut<LayerPoolRenderExecutor>) {
    executor.trim_text_render_maintenance();
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::bevy::{
        DefaultPlugins,
        prelude::{App, PluginGroup},
        sprite_render::Material2dPlugin,
        window::WindowPlugin,
    };

    #[test]
    fn install_ui_dom_runtime_pair_requires_render_app() {
        let mut app = App::new();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            install_ui_dom_runtime_pair(&mut app, LayerConfig::default());
        }));

        assert!(result.is_err());
    }

    #[test]
    fn install_ui_dom_runtime_pair_replaces_resources_without_duplicate_plugins() {
        let mut app = App::new();
        app.add_plugins(DefaultPlugins.build().disable::<WindowPlugin>());

        install_ui_dom_runtime_pair(&mut app, LayerConfig::new(PixelDensity::X1));
        install_ui_dom_runtime_pair(&mut app, LayerConfig::new(PixelDensity::X4));

        assert_eq!(
            app.world().resource::<LayerPool>().pixel_density(),
            PixelDensity::X4
        );
        assert!(
            app.get_added_plugins::<Material2dPlugin<UiDomBaseMaterial>>()
                .is_empty(),
            "ui-dom should not install the default sorted Material2d queue path"
        );
        assert_eq!(
            app.get_sub_app(RenderApp)
                .expect("render app")
                .world()
                .resource::<LayerPoolRenderExecutor>()
                .pixel_density(),
            PixelDensity::X4
        );
    }

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
