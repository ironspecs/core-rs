//! `UiDomTextFontCatalog` retains the loaded font-asset handles for the `ui-dom` text runtime.
//! This file owns the font-handle lookup that bridges retained text runs to loaded font assets.
//!
//! The catalog is a crate-private resource built at startup from the repo font paths; it stays
//! immutable after insertion so extract code can clone handles without shared mutation.
//!
//! Strict invariants:
//! - handles are grouped by `TextFontFamilyId` and never reordered after build.
//! - the catalog exposes handles by borrow, never by owned transfer.

use ::bevy::asset::{AssetId, Handle};
use ::bevy::prelude::Resource;

use crate::bevy::font_assets::UiDomFontAsset;
use crate::domains::text_layout::TextFontFamilyId;

#[derive(Clone, Debug, Resource)]
pub(crate) struct UiDomTextFontCatalog {
    pub(crate) ui_sans_handles: Vec<Handle<UiDomFontAsset>>,
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
