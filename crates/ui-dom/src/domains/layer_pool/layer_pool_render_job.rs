//! `LayerPoolRenderJob` is the internal retained render-work command shape.
//! This file exists so queued retained commands stay a small explicit command vocabulary rather than an untyped mix of queue-side assumptions.
//!
//! The enum stays private to the layer-pool boundary because callers should enqueue through higher-level APIs, not construct render jobs directly.
//! Splitting the command shape out keeps the queue file focused on queue behavior rather than job identity.
//!
//! Staged paint ops moved to `domains/render_ops`; this queue now carries retained
//! COMMANDS only (invalidation and surface-order publication).
//!
//! Strict invariants:
//! - this job vocabulary is internal to the layer-pool boundary.
//! - jobs represent queued retained commands, not arbitrary caller intent.
//! - adding a new job variant changes the retained execution contract and should be reviewed as such.

#[allow(clippy::wildcard_imports)]
use super::*;

#[derive(Clone, Debug)]
pub(super) enum LayerPoolRenderJob {
    Invalidate {
        lease: LayerLease,
    },
    ReplaceRootSurfaceOrder {
        attached_root_parent: MainEntity,
        parent_z_order: f32,
        ordered_surface_entities: Vec<MainEntity>,
    },
    DetachRootSurfaceOrder {
        attached_root_parent: MainEntity,
    },
}
