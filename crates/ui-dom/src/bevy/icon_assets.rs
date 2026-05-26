//! Bevy icon-atlas asset glue for `ui-dom` retained icon rendering.
//!
//! This file exists because `ui-dom` needs Bevy-loaded icon-atlas manifest and
//! image handles, but the raw asset type, asset loader, and asset-server
//! request shape are host glue rather than domain ownership. Manifest schema
//! validation stays in `domains/icon_atlas.rs`.
//!
//! Strict invariants:
//! - this file owns only the raw Bevy icon-atlas asset and request helpers
//! - icon-atlas schema validation and density policy do not belong here
//! - the helper here requests committed icon-atlas assets; it does not inspect
//!   or validate their contents

use std::sync::Arc;

use ::bevy::asset::io::Reader;
use ::bevy::{
    asset::{Asset, AssetLoader, AssetServer, LoadContext},
    image::Image,
    prelude::Handle,
    reflect::TypePath,
};
use ui_icons::{UiIconDensity, ui_icon_atlas_manifest_asset_path, ui_icon_atlas_png_asset_path};

#[derive(Asset, TypePath, Clone, Debug)]
pub(crate) struct UiIconAtlasManifestAsset {
    pub(crate) json: Arc<str>,
}

#[derive(Default, TypePath)]
pub(crate) struct UiIconAtlasManifestAssetLoader;

impl AssetLoader for UiIconAtlasManifestAssetLoader {
    type Asset = UiIconAtlasManifestAsset;
    type Settings = ();
    type Error = std::io::Error;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &Self::Settings,
        _load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let json = String::from_utf8(bytes).map_err(std::io::Error::other)?;
        Ok(UiIconAtlasManifestAsset {
            json: Arc::<str>::from(json),
        })
    }

    fn extensions(&self) -> &[&str] {
        &["json"]
    }
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
