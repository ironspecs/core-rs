//! `UiDomTextFontExtractState` tracks which font assets the render-extract phase already exported.
//! This file owns the per-extract dedup set that keeps font uploads idempotent across frames.
//!
//! The state is a crate-private resource built fresh by each extract pass; it exists only to avoid
//! re-exporting the same font asset id every frame.
//!
//! Strict invariants:
//! - membership is by `AssetId` only; no handles are retained here.
//! - `mark_extracted` must run only after `already_extracted` has already been consulted.

use std::collections::HashSet;

use ::bevy::asset::AssetId;
use ::bevy::prelude::Resource;

use crate::bevy::font_assets::UiDomFontAsset;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mark_extracted_dedups_only_marked_assets() {
        let mut assets = ::bevy::asset::Assets::<UiDomFontAsset>::default();
        let first = assets
            .add(UiDomFontAsset {
                bytes: std::sync::Arc::from(&b"a"[..]),
            })
            .id();
        let second = assets
            .add(UiDomFontAsset {
                bytes: std::sync::Arc::from(&b"a"[..]),
            })
            .id();

        let mut state = UiDomTextFontExtractState::default();
        assert!(!state.already_extracted(first));
        state.mark_extracted(first);
        assert!(state.already_extracted(first));
        assert!(!state.already_extracted(second));
    }
}
