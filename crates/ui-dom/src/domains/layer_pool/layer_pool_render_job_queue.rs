//! `LayerPoolRenderJobQueue` stores pending retained render commands before the render executor consumes them.
//! This file exists so invalidation and surface-order publication behavior remain one explicit policy boundary instead of being duplicated in callers.
//!
//! The queue owns only scheduling behavior for retained commands; it does not own render state or retained layer records.
//! Keeping the queue in its own file makes it easier to see which behavior is policy about timing and which behavior belongs to the executor or main-world owner.
//!
//! Staged paint ops and best-effort deferral moved to `domains/render_ops` (the
//! `RenderOpsHandler`); this queue now carries retained commands only.
//!
//! Strict invariants:
//! - the queue stores retained commands, not render resources.
//! - invalidation and surface-order publication are the only queued commands.

#[allow(clippy::wildcard_imports)]
use super::*;

#[derive(Default, Resource)]
pub(crate) struct LayerPoolRenderJobQueue {
    pub(super) jobs: Vec<LayerPoolRenderJob>,
}

impl LayerPoolRenderJobQueue {
    pub(super) fn push_invalidate(&mut self, lease: LayerLease) {
        self.jobs.push(LayerPoolRenderJob::Invalidate { lease });
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

    fn remove_root_surface_order_jobs(&mut self, attached_root_parent: MainEntity) {
        self.jobs.retain(|job| match job {
            LayerPoolRenderJob::ReplaceRootSurfaceOrder {
                attached_root_parent: existing_root_parent,
                ..
            }
            | LayerPoolRenderJob::DetachRootSurfaceOrder {
                attached_root_parent: existing_root_parent,
            } => *existing_root_parent != attached_root_parent,
            LayerPoolRenderJob::Invalidate { .. } => true,
        });
    }

    #[cfg(test)]
    pub(crate) fn clear_jobs_for_test(&mut self) {
        self.jobs.clear();
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

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::Entity;

    fn main_entity(bits: u64) -> MainEntity {
        Entity::from_bits(bits).into()
    }

    #[test]
    fn surface_order_jobs_replace_and_detach_per_root() {
        let mut queue = LayerPoolRenderJobQueue::default();
        let root_a = main_entity(1);
        let root_b = main_entity(2);
        let e1 = main_entity(10);
        let e2 = main_entity(11);
        let e3 = main_entity(12);
        let e4 = main_entity(20);

        queue.replace_root_surface_order(root_a, 1.0, vec![e1, e2]);
        assert_eq!(
            queue.ordered_surface_entities_for_test(root_a),
            Some(&[e1, e2][..])
        );
        assert_eq!(queue.queued_job_count_for_test(), 1);

        queue.replace_root_surface_order(root_a, 2.0, vec![e3]);
        assert_eq!(
            queue.ordered_surface_entities_for_test(root_a),
            Some(&[e3][..])
        );
        assert_eq!(queue.queued_job_count_for_test(), 1);

        queue.replace_root_surface_order(root_b, 0.5, vec![e4]);
        assert_eq!(queue.queued_job_count_for_test(), 2);
        assert_eq!(
            queue.ordered_surface_entities_for_test(root_a),
            Some(&[e3][..])
        );
        assert_eq!(
            queue.ordered_surface_entities_for_test(root_b),
            Some(&[e4][..])
        );

        queue.detach_root_surface_order(root_a);
        assert_eq!(queue.ordered_surface_entities_for_test(root_a), None);
        assert_eq!(
            queue.ordered_surface_entities_for_test(root_b),
            Some(&[e4][..])
        );

        queue.detach_root_surface_order(root_a);
        assert_eq!(queue.ordered_surface_entities_for_test(root_a), None);
        assert_eq!(
            queue.ordered_surface_entities_for_test(root_b),
            Some(&[e4][..])
        );
    }
}
