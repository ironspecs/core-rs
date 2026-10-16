//! `UiDomRenderSupport` reports which GPU paint runtime surfaces one `ui-dom` install wired up.
//! This file owns the read-only install-support signal shared across the retained paint boundary.
//!
//! The signal is a copyable, compare-safe resource so both the main runtime and the render
//! runtime can observe the same installed capability without reaching into plugin wiring.
//!
//! Strict invariants:
//! - `UiDomRenderSupport` is a report, not a control path; it never mutates install state.
//! - the flag is set once by the render-pair installer and read everywhere else.

use ::bevy::prelude::Resource;

/// Reports the GPU paint runtime support installed for `ui-dom`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Resource)]
pub struct UiDomRenderSupport {
    /// True when `ui-dom` has a direct GPU layer-paint executor wired up.
    pub direct_gpu_executor_available: bool,
}

impl UiDomRenderSupport {
    #[must_use]
    pub fn direct_gpu() -> Self {
        Self {
            direct_gpu_executor_available: true,
        }
    }
}
