//! Bevy font asset glue for `ui-dom` retained text rendering.
//!
//! This file exists because `ui-dom` needs Bevy-loaded font bytes, but the raw
//! asset type and asset loader are host glue rather than domain ownership.
//!
//! Strict invariants:
//! - this file owns only the raw Bevy font asset and its loader.
//! - font family policy and font-catalog policy do not belong here.
//! - startup loading policy stays outside this loader boundary.

use std::sync::Arc;

use ::bevy::asset::io::Reader;
use ::bevy::{
    asset::{Asset, AssetLoader, LoadContext},
    reflect::TypePath,
};

#[derive(Asset, TypePath, Clone, Debug)]
pub(crate) struct UiDomFontAsset {
    pub(crate) bytes: Arc<[u8]>,
}

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
