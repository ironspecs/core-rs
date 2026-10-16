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
