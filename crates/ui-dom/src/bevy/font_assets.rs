//! Bevy font asset glue for `ui-dom` retained text rendering.
//!
//! This file owns only the raw `UiDomFontAsset` type; the asset loader lives in
//! `ui_dom_font_asset_loader.rs`.
//!
//! Strict invariants:
//! - this file owns only the raw Bevy font asset type.
//! - font family policy and font-catalog policy do not belong here.

use std::sync::Arc;

use ::bevy::{asset::Asset, reflect::TypePath};

pub(crate) use crate::bevy::ui_dom_font_asset_loader::UiDomFontAssetLoader;

#[derive(Asset, TypePath, Clone, Debug)]
pub(crate) struct UiDomFontAsset {
    pub(crate) bytes: Arc<[u8]>,
}
