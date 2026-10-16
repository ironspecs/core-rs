//! Render-owned icon atlas load state for the retained layer pool.
//!
//! This file owns the `IconAtlasLoadState` enum that tracks the icon atlas
//! asset load lifecycle (empty, loading, ready, failed) and its pending-install
//! handoff. Its purpose is to keep the load state machine beside the handoff
//! method instead of growing the shared `layer_pool` owner module.
//!
//! Invariants and rules:
//!
//! - this file owns only the `IconAtlasLoadState` enum and its `impl` block
//! - the loading payload (`LoadingIconAtlasState`) and the install payload
//!   (`PendingIconAtlasInstall`) stay in the shared `layer_pool` owner module

use super::{LoadingIconAtlasState, PendingIconAtlasInstall};

#[cfg(test)]
use crate::bevy::icon_assets::RequestedUiIconAtlasAssets;
#[cfg(test)]
use crate::domains::icon_atlas::{
    ValidatedUiIconAtlasEntry, ValidatedUiIconAtlasLookup, ValidatedUiIconAtlasManifest,
};

pub(super) enum IconAtlasLoadState {
    Empty,
    Loading(LoadingIconAtlasState),
    Ready,
    Failed,
}

impl IconAtlasLoadState {
    pub(super) fn take_pending_install(&mut self) -> Option<PendingIconAtlasInstall> {
        let IconAtlasLoadState::Loading(loading_state) = self else {
            return None;
        };
        loading_state.pending_install.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::Handle;
    use std::sync::Arc;
    use ui_icons::UiIconDensity;

    fn pending_install() -> PendingIconAtlasInstall {
        PendingIconAtlasInstall {
            validated_manifest: ValidatedUiIconAtlasManifest {
                atlas_width_px: 64,
                atlas_height_px: 64,
                design_box_size_px: 24,
                lookup: ValidatedUiIconAtlasLookup::from_entries_for_tests(vec![
                    ValidatedUiIconAtlasEntry {
                        packed_rect_px: [0, 0, 24, 24],
                    },
                ]),
            },
            alpha_mask_bytes: Arc::<[u8]>::from(Vec::<u8>::new()),
            atlas_width_px: 64,
            atlas_height_px: 64,
        }
    }

    fn loading_state_with_pending_install() -> IconAtlasLoadState {
        IconAtlasLoadState::Loading(LoadingIconAtlasState {
            requested_assets: RequestedUiIconAtlasAssets {
                retained_density: UiIconDensity::X1,
                manifest_handle: Handle::default(),
                image_handle: Handle::default(),
            },
            validated_manifest: None,
            pending_install: Some(pending_install()),
        })
    }

    #[test]
    fn take_pending_install_consumes_the_pending_install_once() {
        let mut empty = IconAtlasLoadState::Empty;
        assert!(empty.take_pending_install().is_none());

        let mut loading = loading_state_with_pending_install();
        assert!(loading.take_pending_install().is_some());
        assert!(loading.take_pending_install().is_none());
    }
}
