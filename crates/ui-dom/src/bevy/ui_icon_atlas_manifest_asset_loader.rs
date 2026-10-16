//! `UiIconAtlasManifestAssetLoader` loads icon-atlas manifest JSON into a `UiIconAtlasManifestAsset`.
//! This file owns the Bevy asset-loader seam for `ui-dom` retained icon rendering.
//!
//! The raw asset type and request helpers stay in `icon_assets.rs`; this loader only decodes
//! manifest JSON bytes into that asset. Manifest schema validation stays in `domains/icon_atlas.rs`.
//!
//! Strict invariants:
//! - this file owns only the icon-atlas manifest asset loader.
//! - the loader does not inspect or validate manifest contents.

use std::sync::Arc;

use ::bevy::asset::io::Reader;
use ::bevy::{
    asset::{AssetLoader, LoadContext},
    reflect::TypePath,
};

use crate::bevy::icon_assets::UiIconAtlasManifestAsset;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_manifest_loader_advertises_json_extension() {
        let loader = UiIconAtlasManifestAssetLoader;
        assert_eq!(loader.extensions(), ["json"]);
    }
}
