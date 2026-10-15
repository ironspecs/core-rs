//! Staging owner boundary for paint ops between `ui-dom`'s paint stage and the
//! Bevy render-world handoff.
//!
//! This module owns the retained staging storage that holds one Bevy tick's
//! outgoing paint ops until the render world's extract moves them once per
//! tick. It is sealed by design (PLAN.md §4.1, C6): no validation, no `World`,
//! no Bevy types in the handler API, no policy beyond queue policy, and no
//! metrics.
//!
//! Invariants and rules:
//! - staged ops move end-to-end; the handler never clones an apply on the
//!   paint-to-executor path.
//! - the `RenderOpsHandlers` registry is main-world-only bookkeeping that tells
//!   the extract which handlers to drain, in registration order (deterministic).
//! - no revision, sequence, or time fields may be added anywhere in this module.

mod render_ops_batch;
mod render_ops_handler;
mod staged_external_rgba_upload;
mod staged_layer_apply;

pub use render_ops_batch::RenderOpsBatch;
pub use render_ops_handler::{RenderOpsHandler, StageOutcome};
pub use staged_external_rgba_upload::StagedExternalRgbaUpload;
pub use staged_layer_apply::StagedLayerApply;

/// Main-world registry of every `RenderOpsHandler` the render world's extract
/// should drain, in registration order (deterministic).
#[derive(bevy::prelude::Resource, Default)]
pub struct RenderOpsHandlers(pub(crate) Vec<std::sync::Arc<RenderOpsHandler>>);

/// Registers one handler for the per-tick extract drain. Idempotent per handler
/// (dedup by `Arc::ptr_eq`). Lazily initializes the registry when absent —
/// the same pattern `LayerPoolRenderJobQueue` uses today.
pub fn register_render_ops_handler(
    world: &mut bevy::prelude::World,
    handler: &std::sync::Arc<RenderOpsHandler>,
) {
    world.init_resource::<RenderOpsHandlers>();
    let mut registry = world.resource_mut::<RenderOpsHandlers>();
    if !registry
        .0
        .iter()
        .any(|existing| std::sync::Arc::ptr_eq(existing, handler))
    {
        registry.0.push(std::sync::Arc::clone(handler));
    }
}
