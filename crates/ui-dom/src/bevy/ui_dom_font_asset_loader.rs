//! `UiDomFontAssetLoader` loads raw font bytes (`.ttf`/`.otf`) into a `UiDomFontAsset`.
//! This file owns the Bevy asset-loader seam for `ui-dom` retained text rendering.
//!
//! The raw asset type `UiDomFontAsset` stays in `font_assets.rs`; this loader only decodes
//! bytes into that asset.
//!
//! Strict invariants:
//! - this file owns only the font asset loader.
//! - font family policy and startup loading policy do not belong here.

use std::sync::Arc;

use ::bevy::asset::io::Reader;
use ::bevy::{
    asset::{AssetLoader, LoadContext},
    reflect::TypePath,
};

use crate::bevy::font_assets::UiDomFontAsset;

#[derive(Default, TypePath)]
pub(crate) struct UiDomFontAssetLoader;

impl AssetLoader for UiDomFontAssetLoader {
    type Asset = UiDomFontAsset;
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
        Ok(UiDomFontAsset {
            bytes: Arc::<[u8]>::from(bytes),
        })
    }

    fn extensions(&self) -> &[&str] {
        &["ttf", "otf"]
    }
}
