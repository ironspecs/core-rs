//! Render-owned font registration state for the retained layer pool.
//!
//! This file owns the bookkeeping that tracks which font assets a render
//! executor has loaded against the asset ids each retained face requires. Its
//! purpose is to keep font-load readiness beside the state it reads instead of
//! hiding it inside the text-run render owner file.
//!
//! Invariants and rules:
//!
//! - this file owns only the `FontRegistrationState` struct and its `impl` block
//! - a face is ready only when every required asset id has been registered
//! - readiness is keyed per-FACE by `FontFaceId` (text seam S3: the catalog
//!   face id, not the removed `TextFontFamilyId` placeholder)

use std::collections::{HashMap, HashSet};

use bevy::asset::AssetId;

use crate::domains::layer_ops::FontFaceId;
use crate::render_plugin::UiDomFontAsset;

#[derive(Clone, Debug, Default)]
pub(super) struct FontRegistrationState {
    required_font_asset_ids_by_face: HashMap<FontFaceId, Vec<AssetId<UiDomFontAsset>>>,
    pub(super) registered_font_asset_ids: HashSet<AssetId<UiDomFontAsset>>,
}

impl FontRegistrationState {
    #[must_use]
    pub(super) fn ready(&self, face_id: FontFaceId) -> bool {
        let Some(required_font_asset_ids) = self.required_font_asset_ids_by_face.get(&face_id)
        else {
            return false;
        };
        !required_font_asset_ids.is_empty()
            && required_font_asset_ids
                .iter()
                .all(|asset_id| self.registered_font_asset_ids.contains(asset_id))
    }

    pub(super) fn set_required_font_asset_ids(
        &mut self,
        face_id: FontFaceId,
        required_font_asset_ids: Vec<AssetId<UiDomFontAsset>>,
    ) {
        self.required_font_asset_ids_by_face
            .insert(face_id, required_font_asset_ids);
    }

    pub(super) fn mark_registered(&mut self, asset_id: AssetId<UiDomFontAsset>) {
        self.registered_font_asset_ids.insert(asset_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_is_ready_only_when_all_required_assets_registered() {
        let mut state = FontRegistrationState::default();
        assert!(!state.ready(FontFaceId::EMBEDDED_IBM_PLEX_SANS));

        let mut assets = bevy::asset::Assets::<UiDomFontAsset>::default();
        let id_a = assets
            .add(UiDomFontAsset {
                bytes: std::sync::Arc::from(&b"a"[..]),
            })
            .id();
        let id_b = assets
            .add(UiDomFontAsset {
                bytes: std::sync::Arc::from(&b"a"[..]),
            })
            .id();

        state.set_required_font_asset_ids(FontFaceId::EMBEDDED_IBM_PLEX_SANS, vec![id_a, id_b]);
        assert!(!state.ready(FontFaceId::EMBEDDED_IBM_PLEX_SANS));

        state.mark_registered(id_a);
        assert!(!state.ready(FontFaceId::EMBEDDED_IBM_PLEX_SANS));

        state.mark_registered(id_b);
        assert!(state.ready(FontFaceId::EMBEDDED_IBM_PLEX_SANS));
    }
}
