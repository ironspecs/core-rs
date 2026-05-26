//! `LayerPoolRenderJobQueue` stores pending retained render jobs before the render executor consumes them.
//! This file exists so queue pressure, best-effort deferral, and invalidation replacement behavior remain one explicit policy boundary instead of being duplicated in callers.
//!
//! The queue owns only scheduling behavior; it does not own render state or retained layer records.
//! Keeping the queue in its own file makes it easier to see which behavior is policy about timing and which behavior belongs to the executor or main-world owner.
//!
//! Strict invariants:
//! - the queue stores retained jobs, not render resources.
//! - best-effort deferral policy is enforced here, not by callers.
//! - invalidate jobs must clear conflicting deferred applies for the same lease.

#[allow(clippy::wildcard_imports)]
use super::*;

#[derive(Default, Resource)]
pub(crate) struct LayerPoolRenderJobQueue {
    pub(super) jobs: Vec<LayerPoolRenderJob>,
    pub(super) deferred_best_effort_applies: Vec<PendingLayerApply>,
}

impl LayerPoolRenderJobQueue {
    fn push_apply(&mut self, apply: PendingLayerApply) {
        self.jobs.push(LayerPoolRenderJob::Apply(apply));
    }

    pub(super) fn queue_external_rgba_upload(&mut self, upload: PendingExternalRgbaUpload) {
        if let Some(existing) = self.jobs.iter_mut().find_map(|job| match job {
            LayerPoolRenderJob::ExternalRgbaUpload(existing) if existing.lease == upload.lease => {
                Some(existing)
            }
            _ => None,
        }) {
            *existing = upload;
            return;
        }
        self.jobs
            .push(LayerPoolRenderJob::ExternalRgbaUpload(upload));
    }

    pub(super) fn queue_apply(
        &mut self,
        apply: PendingLayerApply,
        policy: LayerPoolPolicy,
    ) -> bool {
        if policy == LayerPoolPolicy::BestEffort
            && self.queued_apply_count() >= BEST_EFFORT_UPLOAD_QUEUE_LIMIT
        {
            self.upsert_deferred_best_effort_apply(apply);
            return true;
        }
        self.upsert_immediate_apply(apply);
        false
    }

    pub(super) fn push_invalidate(&mut self, lease: LayerLease) {
        self.jobs.push(LayerPoolRenderJob::Invalidate { lease });
        self.deferred_best_effort_applies
            .retain(|apply| apply.lease != lease);
        self.jobs.retain(|job| match job {
            LayerPoolRenderJob::ExternalRgbaUpload(upload) => upload.lease != lease,
            _ => true,
        });
    }

    pub(crate) fn replace_root_surface_order(
        &mut self,
        attached_root_parent: MainEntity,
        parent_z_order: f32,
        ordered_surface_entities: Vec<MainEntity>,
    ) {
        self.remove_root_surface_order_jobs(attached_root_parent);
        self.jobs.push(LayerPoolRenderJob::ReplaceRootSurfaceOrder {
            attached_root_parent,
            parent_z_order,
            ordered_surface_entities,
        });
    }

    pub(crate) fn detach_root_surface_order(&mut self, attached_root_parent: MainEntity) {
        self.remove_root_surface_order_jobs(attached_root_parent);
        self.jobs.push(LayerPoolRenderJob::DetachRootSurfaceOrder {
            attached_root_parent,
        });
    }

    pub(super) fn promote_deferred_best_effort_applies(&mut self) {
        while self.queued_apply_count() < BEST_EFFORT_UPLOAD_QUEUE_LIMIT {
            let Some(apply) = self.take_oldest_deferred_best_effort_apply() else {
                return;
            };
            self.upsert_immediate_apply(apply);
        }
    }

    fn queued_apply_count(&self) -> usize {
        self.jobs
            .iter()
            .filter(|job| matches!(job, LayerPoolRenderJob::Apply(_)))
            .count()
    }

    fn upsert_immediate_apply(&mut self, apply: PendingLayerApply) {
        if let Some(existing) = self.jobs.iter_mut().find_map(|job| match job {
            LayerPoolRenderJob::Apply(existing) if existing.lease == apply.lease => Some(existing),
            _ => None,
        }) {
            *existing = apply;
            return;
        }
        self.push_apply(apply);
    }

    fn upsert_deferred_best_effort_apply(&mut self, apply: PendingLayerApply) {
        if let Some(existing) = self
            .deferred_best_effort_applies
            .iter_mut()
            .find(|existing| existing.lease == apply.lease)
        {
            *existing = apply;
            return;
        }
        self.deferred_best_effort_applies.push(apply);
    }

    fn take_oldest_deferred_best_effort_apply(&mut self) -> Option<PendingLayerApply> {
        if self.deferred_best_effort_applies.is_empty() {
            return None;
        }
        Some(self.deferred_best_effort_applies.remove(0))
    }

    fn remove_root_surface_order_jobs(&mut self, attached_root_parent: MainEntity) {
        self.jobs.retain(|job| match job {
            LayerPoolRenderJob::ReplaceRootSurfaceOrder {
                attached_root_parent: existing_root_parent,
                ..
            }
            | LayerPoolRenderJob::DetachRootSurfaceOrder {
                attached_root_parent: existing_root_parent,
            } => *existing_root_parent != attached_root_parent,
            LayerPoolRenderJob::Apply(_)
            | LayerPoolRenderJob::ExternalRgbaUpload(_)
            | LayerPoolRenderJob::Invalidate { .. } => true,
        });
    }

    #[cfg(test)]
    pub(crate) fn clear_jobs_for_test(&mut self) {
        self.jobs.clear();
        self.deferred_best_effort_applies.clear();
    }

    #[cfg(test)]
    pub(crate) fn queued_job_count_for_test(&self) -> usize {
        self.jobs.len()
    }

    #[cfg(test)]
    pub(crate) fn ordered_surface_entities_for_test(
        &self,
        attached_root_parent: MainEntity,
    ) -> Option<&[MainEntity]> {
        self.jobs
            .iter()
            .find_map(|job| match job {
                LayerPoolRenderJob::ReplaceRootSurfaceOrder {
                    attached_root_parent: existing_root_parent,
                    ordered_surface_entities,
                    ..
                } if *existing_root_parent == attached_root_parent => {
                    Some(ordered_surface_entities)
                }
                _ => None,
            })
            .map(Vec::as_slice)
    }
}
