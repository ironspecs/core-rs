//! `RenderOpsHandler` is the sealed staging owner between paint and the
//! render-world handoff.
//!
//! This file owns the retained staging storage and the queue policy that
//! `RenderOpsHandler` applies to staged applies and external uploads. It holds
//! one Bevy tick's outgoing ops in persistent `Vec`s, drained with
//! `drain(..)` so capacity persists across frames.
//!
//! The handler is `Arc`-shared and internally synchronized (the same pattern
//! as `RootDomContext::style_runtime`), and deliberately dumb: no validation,
//! no `World`, no Bevy types in its API, no policy beyond queue policy, and no
//! metrics. Validation stays in the `LayerPool` facade path; external
//! producers hold real leases.
//!
//! Strict invariants:
//! - stage acceptance is infallible and terminal: the handler never rejects,
//!   never reorders beyond upsert-by-lease, and never inspects op content.
//! - best-effort applies defer past the queue limit; deferred work is promoted
//!   only after a drain, so promoted work leaves the handler in a later
//!   `take_staged`, never in the batch that just left.
//! - `invalidate_lease` clears that lease across all three vectors, so a staged
//!   apply can never execute against a destroyed or repurposed surface.
//! - read access (`with_staged_*`) never mutates, never allocates, and never
//!   touches the render world.
//! - no revision, sequence, or time fields may be added.

use std::sync::Mutex;

use crate::domains::{LayerLease, LayerPoolPolicy};

use super::{RenderOpsBatch, StagedExternalRgbaUpload, StagedLayerApply};

/// Maximum number of best-effort applies staged before further best-effort
/// applies are deferred. Mirrors the queue-pressure limit the render job queue
/// enforced before the handler absorbed queue policy.
const BEST_EFFORT_UPLOAD_QUEUE_LIMIT: usize = 1;

/// Sealed, `Arc`-shared staging owner for one Bevy tick's outgoing paint ops.
#[derive(Debug, Default)]
pub struct RenderOpsHandler {
    state: Mutex<RenderOpsHandlerState>,
}

/// Retained staging storage: staged applies, staged external uploads, and
/// deferred best-effort applies awaiting promotion.
#[derive(Debug, Default)]
struct RenderOpsHandlerState {
    staged_applies: Vec<StagedLayerApply>,
    staged_external_uploads: Vec<StagedExternalRgbaUpload>,
    deferred_best_effort_applies: Vec<StagedLayerApply>,
}

/// Result of staging one apply: immediate or deferred by queue policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StageOutcome {
    /// The apply was staged for the current drain.
    Staged,
    /// The apply was deferred for promotion after a future drain.
    Deferred,
}

impl RenderOpsHandler {
    /// Creates one empty, freshly synchronized handler.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Accepts one owned apply and applies queue policy (upsert-by-lease;
    /// `BestEffort` defers past the queue limit). Infallible.
    #[must_use]
    pub fn stage(&self, apply: StagedLayerApply, policy: LayerPoolPolicy) -> StageOutcome {
        let mut state = self
            .state
            .lock()
            .expect("render ops handler mutex should not poison");
        if policy == LayerPoolPolicy::BestEffort
            && state.staged_applies.len() >= BEST_EFFORT_UPLOAD_QUEUE_LIMIT
        {
            upsert_apply(&mut state.deferred_best_effort_applies, apply);
            return StageOutcome::Deferred;
        }
        upsert_apply(&mut state.staged_applies, apply);
        StageOutcome::Staged
    }

    /// Accepts one owned external RGBA upload (upsert-by-lease). Never deferred.
    pub fn stage_external_rgba(&self, upload: StagedExternalRgbaUpload) {
        let mut state = self
            .state
            .lock()
            .expect("render ops handler mutex should not poison");
        if let Some(existing) = state
            .staged_external_uploads
            .iter_mut()
            .find(|existing| existing.lease == upload.lease)
        {
            *existing = upload;
            return;
        }
        state.staged_external_uploads.push(upload);
    }

    /// Removes staged applies, staged uploads, and deferred applies for one
    /// lease, so a staged apply can never execute against a destroyed or
    /// repurposed surface.
    pub fn invalidate_lease(&self, lease: LayerLease) {
        let mut state = self
            .state
            .lock()
            .expect("render ops handler mutex should not poison");
        state.staged_applies.retain(|apply| apply.lease != lease);
        state
            .staged_external_uploads
            .retain(|upload| upload.lease != lease);
        state
            .deferred_best_effort_applies
            .retain(|apply| apply.lease != lease);
    }

    /// Zero-allocation read access to staged applies, in stage order.
    /// Read-only; never mutates.
    pub fn with_staged_applies<R>(&self, f: impl FnOnce(&[StagedLayerApply]) -> R) -> R {
        let state = self
            .state
            .lock()
            .expect("render ops handler mutex should not poison");
        f(&state.staged_applies)
    }

    /// Zero-allocation read access to staged external uploads, in stage order.
    /// Read-only; never mutates.
    pub fn with_staged_external_uploads<R>(
        &self,
        f: impl FnOnce(&[StagedExternalRgbaUpload]) -> R,
    ) -> R {
        let state = self
            .state
            .lock()
            .expect("render ops handler mutex should not poison");
        f(&state.staged_external_uploads)
    }

    /// Moves everything currently staged out to the caller — the manual move
    /// for hosts/tests without a render app.
    ///
    /// Deferred best-effort applies are promoted into staged storage only
    /// after the drain, so promoted work leaves the handler in a later
    /// `take_staged`, never in the batch that just left.
    #[must_use]
    pub fn take_staged(&self) -> RenderOpsBatch {
        let mut state = self
            .state
            .lock()
            .expect("render ops handler mutex should not poison");
        let batch = RenderOpsBatch {
            applies: state.staged_applies.drain(..).collect(),
            external_uploads: state.staged_external_uploads.drain(..).collect(),
        };
        while state.staged_applies.len() < BEST_EFFORT_UPLOAD_QUEUE_LIMIT {
            if state.deferred_best_effort_applies.is_empty() {
                break;
            }
            let promoted = state.deferred_best_effort_applies.remove(0);
            state.staged_applies.push(promoted);
        }
        batch
    }
}

/// Upserts one apply into a staged vector, replacing the same-lease entry in
/// place when present and pushing otherwise.
fn upsert_apply(staged: &mut Vec<StagedLayerApply>, apply: StagedLayerApply) {
    if let Some(existing) = staged
        .iter_mut()
        .find(|existing| existing.lease == apply.lease)
    {
        *existing = apply;
        return;
    }
    staged.push(apply);
}

#[cfg(test)]
use crate::domains::layer_ops::{LayerPaintPlan, RectPaintOp};
#[cfg(test)]
use crate::domains::layer_pool::{LayerConfig, LayerPool};

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::UVec2;
    use bevy::{asset::Handle, image::Image, prelude::World};

    fn create_test_leases(count: usize) -> Vec<LayerLease> {
        let mut world = World::new();
        world.insert_resource(LayerPool::new(LayerConfig::default()));
        (0..count)
            .map(|_| LayerPool::create_layer_lease_in_world(&mut world, (0.0, 0.0, 10.0, 10.0)))
            .collect()
    }

    fn two_leases() -> [LayerLease; 2] {
        let leases = create_test_leases(2);
        [leases[0], leases[1]]
    }

    fn rect_op(position_px: [f32; 2]) -> RectPaintOp {
        RectPaintOp {
            position_px,
            size_px: [8.0, 8.0],
            fill_rgba: [1.0, 1.0, 1.0, 1.0],
            corner_radii_px: [0.0; 4],
            border_widths_px: [0.0; 4],
            border_top_rgba: [0.0; 4],
            border_right_rgba: [0.0; 4],
            border_bottom_rgba: [0.0; 4],
            border_left_rgba: [0.0; 4],
        }
    }

    fn apply(lease: LayerLease, plan: LayerPaintPlan) -> StagedLayerApply {
        StagedLayerApply {
            lease,
            image_handle: Handle::<Image>::default(),
            layer_zero_in_image_px: [0.0, 0.0],
            plan,
        }
    }

    fn upload(lease: LayerLease) -> StagedExternalRgbaUpload {
        StagedExternalRgbaUpload {
            lease,
            image_handle: Handle::<Image>::default(),
            image_extent_px: UVec2::new(2, 2),
            rgba_bytes: Vec::new(),
        }
    }

    #[test]
    fn stage_replaces_same_lease_with_latest_plan() {
        let handler = RenderOpsHandler::new();
        let lease = create_test_leases(1)[0];

        let mut original = LayerPaintPlan::default();
        original.rects.push(rect_op([1.0, 2.0]));
        let mut latest = LayerPaintPlan::default();
        latest.rects.push(rect_op([30.0, 40.0]));

        assert_eq!(
            handler.stage(apply(lease, original), LayerPoolPolicy::Normal),
            StageOutcome::Staged,
        );
        assert_eq!(
            handler.stage(apply(lease, latest.clone()), LayerPoolPolicy::Normal),
            StageOutcome::Staged,
        );

        handler.with_staged_applies(|applies| {
            assert_eq!(applies.len(), 1);
            assert_eq!(applies[0].plan, latest);
        });
    }

    #[test]
    fn best_effort_defers_past_queue_limit_and_promotes_on_drain() {
        let handler = RenderOpsHandler::new();
        let [first, second] = two_leases();

        assert_eq!(
            handler.stage(
                apply(first, LayerPaintPlan::default()),
                LayerPoolPolicy::BestEffort
            ),
            StageOutcome::Staged,
        );
        assert_eq!(
            handler.stage(
                apply(second, LayerPaintPlan::default()),
                LayerPoolPolicy::BestEffort
            ),
            StageOutcome::Deferred,
        );

        let first_batch = handler.take_staged();
        assert_eq!(first_batch.applies.len(), 1);
        assert_eq!(first_batch.applies[0].lease, first);

        let second_batch = handler.take_staged();
        assert_eq!(second_batch.applies.len(), 1);
        assert_eq!(second_batch.applies[0].lease, second);

        let third_batch = handler.take_staged();
        assert!(third_batch.applies.is_empty());
    }

    #[test]
    fn normal_policy_never_defers_past_queue_limit() {
        let handler = RenderOpsHandler::new();
        let [first, second] = two_leases();

        assert_eq!(
            handler.stage(
                apply(first, LayerPaintPlan::default()),
                LayerPoolPolicy::Normal
            ),
            StageOutcome::Staged,
        );
        assert_eq!(
            handler.stage(
                apply(second, LayerPaintPlan::default()),
                LayerPoolPolicy::Normal
            ),
            StageOutcome::Staged,
        );

        handler.with_staged_applies(|applies| {
            assert_eq!(applies.len(), 2);
        });
    }

    #[test]
    fn invalidate_lease_removes_that_leases_staged_and_deferred_entries() {
        let handler = RenderOpsHandler::new();
        let [victim, other] = two_leases();

        assert_eq!(
            handler.stage(
                apply(victim, LayerPaintPlan::default()),
                LayerPoolPolicy::Normal
            ),
            StageOutcome::Staged,
        );
        handler.stage_external_rgba(upload(victim));
        assert_eq!(
            handler.stage(
                apply(victim, LayerPaintPlan::default()),
                LayerPoolPolicy::BestEffort
            ),
            StageOutcome::Deferred,
        );

        assert_eq!(
            handler.stage(
                apply(other, LayerPaintPlan::default()),
                LayerPoolPolicy::Normal
            ),
            StageOutcome::Staged,
        );
        handler.stage_external_rgba(upload(other));

        handler.invalidate_lease(victim);

        handler.with_staged_applies(|applies| {
            assert_eq!(applies.len(), 1);
            assert_eq!(applies[0].lease, other);
        });
        handler.with_staged_external_uploads(|uploads| {
            assert_eq!(uploads.len(), 1);
            assert_eq!(uploads[0].lease, other);
        });

        let batch = handler.take_staged();
        assert_eq!(batch.applies.len(), 1);
        assert_eq!(batch.applies[0].lease, other);
        assert_eq!(batch.external_uploads.len(), 1);
        assert_eq!(batch.external_uploads[0].lease, other);

        // The victim's deferred apply is gone, so nothing promotes later.
        let empty = handler.take_staged();
        assert!(empty.applies.is_empty());
        assert!(empty.external_uploads.is_empty());
    }

    #[test]
    fn take_staged_drains_both_staged_vectors() {
        let handler = RenderOpsHandler::new();
        let lease = create_test_leases(1)[0];

        assert_eq!(
            handler.stage(
                apply(lease, LayerPaintPlan::default()),
                LayerPoolPolicy::Normal
            ),
            StageOutcome::Staged,
        );
        handler.stage_external_rgba(upload(lease));

        let batch = handler.take_staged();
        assert_eq!(batch.applies.len(), 1);
        assert_eq!(batch.external_uploads.len(), 1);

        let second = handler.take_staged();
        assert!(second.applies.is_empty());
        assert!(second.external_uploads.is_empty());
    }

    #[test]
    fn with_staged_applies_observes_applies_in_stage_order() {
        let handler = RenderOpsHandler::new();
        let [first, second] = two_leases();

        assert_eq!(
            handler.stage(
                apply(first, LayerPaintPlan::default()),
                LayerPoolPolicy::Normal
            ),
            StageOutcome::Staged,
        );
        assert_eq!(
            handler.stage(
                apply(second, LayerPaintPlan::default()),
                LayerPoolPolicy::Normal
            ),
            StageOutcome::Staged,
        );

        handler.with_staged_applies(|applies| {
            assert_eq!(applies.len(), 2);
            assert_eq!(applies[0].lease, first);
            assert_eq!(applies[1].lease, second);
        });
    }

    #[test]
    fn re_staging_a_lease_keeps_its_stage_position() {
        let handler = RenderOpsHandler::new();
        let [first, second] = two_leases();

        let mut first_plan_v1 = LayerPaintPlan::default();
        first_plan_v1.rects.push(rect_op([1.0, 1.0]));
        let mut first_plan_v2 = LayerPaintPlan::default();
        first_plan_v2.rects.push(rect_op([9.0, 9.0]));

        assert_eq!(
            handler.stage(apply(first, first_plan_v1), LayerPoolPolicy::Normal),
            StageOutcome::Staged,
        );
        assert_eq!(
            handler.stage(
                apply(second, LayerPaintPlan::default()),
                LayerPoolPolicy::Normal
            ),
            StageOutcome::Staged,
        );
        assert_eq!(
            handler.stage(apply(first, first_plan_v2.clone()), LayerPoolPolicy::Normal),
            StageOutcome::Staged,
        );

        handler.with_staged_applies(|applies| {
            assert_eq!(applies.len(), 2);
            assert_eq!(
                applies[0].lease, first,
                "re-staged lease should keep its original stage position"
            );
            assert_eq!(applies[1].lease, second);
            assert_eq!(applies[0].plan, first_plan_v2);
        });
    }
}
