//! `UiDomTextFontCatalog` retains the loaded font-asset handles for the `ui-dom` text runtime.
//! This file owns the font-handle lookup that bridges retained text runs to loaded font assets.
//!
//! The catalog is a crate-private resource built at startup from the repo font paths; it also
//! carries the shared source-of-truth `TextFontCatalog` so the render-extract phase can enumerate
//! consumer-installed faces (text seam S3). The embedded handles are immutable after insertion;
//! consumer faces are reconciled per-extract from the shared catalog.
//!
//! Strict invariants:
//! - embedded handles are ordered by `FontFaceId` position and never reordered after build.
//! - the catalog exposes handles by borrow, never by owned transfer.
//! - the shared catalog is the single source of truth for "which faces exist".

use ::bevy::asset::Handle;
use ::bevy::prelude::Resource;

use crate::bevy::font_assets::UiDomFontAsset;
use crate::domains::text_layout::TextFontCatalog;

#[derive(Clone, Debug, Resource)]
pub(crate) struct UiDomTextFontCatalog {
    pub(crate) embedded_handles: Vec<Handle<UiDomFontAsset>>,
    pub(crate) font_catalog: TextFontCatalog,
}

impl UiDomTextFontCatalog {
    #[must_use]
    pub(crate) fn embedded_handles(&self) -> &[Handle<UiDomFontAsset>] {
        &self.embedded_handles
    }

    #[must_use]
    pub(crate) fn font_catalog(&self) -> &TextFontCatalog {
        &self.font_catalog
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_reports_handles_in_order_and_holds_the_shared_catalog() {
        let mut assets = ::bevy::asset::Assets::<UiDomFontAsset>::default();
        let first_handle = assets.add(UiDomFontAsset {
            bytes: std::sync::Arc::from(&b"a"[..]),
        });
        let second_handle = assets.add(UiDomFontAsset {
            bytes: std::sync::Arc::from(&b"a"[..]),
        });

        let catalog = UiDomTextFontCatalog {
            embedded_handles: vec![first_handle.clone(), second_handle.clone()],
            font_catalog: TextFontCatalog::repo_default(),
        };

        assert_eq!(
            catalog.embedded_handles(),
            [first_handle.clone(), second_handle.clone()]
        );
        assert_eq!(catalog.font_catalog().installed_face_ids().len(), 4);
    }
}
