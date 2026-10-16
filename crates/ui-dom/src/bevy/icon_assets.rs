//! Bevy icon-atlas asset glue for `ui-dom` retained icon rendering.
//!
//! This file owns the raw `UiIconAtlasManifestAsset` type and the asset-server request helpers;
//! the asset loader lives in `ui_icon_atlas_manifest_asset_loader.rs`. Manifest schema
//! validation stays in `domains/icon_atlas.rs`.
//!
//! Strict invariants:
//! - this file owns only the raw Bevy icon-atlas asset and request helpers.
//! - icon-atlas schema validation and density policy do not belong here.
//! - the helper here requests committed icon-atlas assets; it does not inspect or validate their
//!   contents.

use std::sync::Arc;

use ::bevy::{
    asset::{Asset, AssetServer},
    image::Image,
    prelude::Handle,
    reflect::TypePath,
};
use ui_icons::{UiIconDensity, ui_icon_atlas_manifest_asset_path, ui_icon_atlas_png_asset_path};

pub(crate) use crate::bevy::ui_icon_atlas_manifest_asset_loader::UiIconAtlasManifestAssetLoader;

#[derive(Asset, TypePath, Clone, Debug)]
pub(crate) struct UiIconAtlasManifestAsset {
    pub(crate) json: Arc<str>,
}

#[derive(Clone)]
pub(crate) struct RequestedUiIconAtlasAssets {
    pub(crate) retained_density: UiIconDensity,
    pub(crate) manifest_handle: Handle<UiIconAtlasManifestAsset>,
    pub(crate) image_handle: Handle<Image>,
}

#[must_use]
pub(crate) fn requested_ui_icon_atlas_assets(
    asset_server: &AssetServer,
    retained_density: UiIconDensity,
) -> RequestedUiIconAtlasAssets {
    RequestedUiIconAtlasAssets {
        retained_density,
        manifest_handle: asset_server.load(ui_icon_atlas_manifest_asset_path(retained_density)),
        image_handle: asset_server.load(ui_icon_atlas_png_asset_path(retained_density)),
    }
}
