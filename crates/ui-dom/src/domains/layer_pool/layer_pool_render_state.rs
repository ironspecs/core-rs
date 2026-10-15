//! Render-state behavior for the retained layer pool.
//!
//! This file exists so Render-world apply/install behavior lives beside the
//! Render-owned state instead of being orchestrated through free functions in
//! the shared `layer_pool` owner module.

pub(super) struct RenderSurfaceApplyResult {
    pub(super) rect_buffer_grew: bool,
    pub(super) text_runs_ready: bool,
}
