//! `UiDomTextFontExtractState` tracks which font assets the render-extract phase already exported.
//! This file owns the per-extract dedup sets that keep font uploads idempotent across frames,
//! including the consumer-face id → asset-id mapping the render world reuses across frames.
//!
//! The state is a crate-private resource built fresh by each extract pass; it exists only to avoid
//! re-exporting the same font asset id every frame and to keep a consumer face's one `UiDomFontAsset`
//! stable once it has been minted.
//!
//! Strict invariants:
//! - embedded membership is by `AssetId` only; consumer membership is by `FontFaceId`.
//! - `mark_extracted`/`mark_consumer_face` must run only after `already_extracted`/`consumer_face_asset_id`
//!   have already been consulted.

use std::collections::{HashMap, HashSet};

use ::bevy::asset::AssetId;
use ::bevy::prelude::Resource;

use crate::bevy::font_assets::UiDomFontAsset;
use crate::domains::text_layout::FontFaceId;

#[derive(Default, Resource)]
pub(crate) struct UiDomTextFontExtractState {
    extracted_font_asset_ids: HashSet<AssetId<UiDomFontAsset>>,
    consumer_face_asset_ids: HashMap<FontFaceId, AssetId<UiDomFontAsset>>,
}

impl UiDomTextFontExtractState {
    #[must_use]
    pub(crate) fn already_extracted(&self, asset_id: AssetId<UiDomFontAsset>) -> bool {
        self.extracted_font_asset_ids.contains(&asset_id)
    }

    pub(crate) fn mark_extracted(&mut self, asset_id: AssetId<UiDomFontAsset>) {
        self.extracted_font_asset_ids.insert(asset_id);
    }

    #[must_use]
    pub(crate) fn consumer_face_asset_id(
        &self,
        face_id: FontFaceId,
    ) -> Option<AssetId<UiDomFontAsset>> {
        self.consumer_face_asset_ids.get(&face_id).copied()
    }

    pub(crate) fn mark_consumer_face(
        &mut self,
        face_id: FontFaceId,
        asset_id: AssetId<UiDomFontAsset>,
    ) {
        self.consumer_face_asset_ids.insert(face_id, asset_id);
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

    #[test]
    fn consumer_face_asset_id_roundtrips_by_face_id() {
        let mut assets = ::bevy::asset::Assets::<UiDomFontAsset>::default();
        let asset_id = assets
            .add(UiDomFontAsset {
                bytes: std::sync::Arc::from(&b"a"[..]),
            })
            .id();
        let face_id = FontFaceId::from_raw(7);

        let mut state = UiDomTextFontExtractState::default();
        assert_eq!(state.consumer_face_asset_id(face_id), None);
        state.mark_consumer_face(face_id, asset_id);
        assert_eq!(state.consumer_face_asset_id(face_id), Some(asset_id));
    }
}
