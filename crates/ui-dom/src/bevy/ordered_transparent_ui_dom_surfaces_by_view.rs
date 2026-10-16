//! `OrderedTransparentUiDomSurfacesByView` retains the queued transparent items for each render view.
//! This file owns the per-view ordered item buffers that the ordered transparent pass walks.
//!
//! Queueing policy and item preparation stay in `ordered_surface_render.rs`; this resource only
//! stores and clears per-view item lists.
//!
//! Strict invariants:
//! - items are cleared per live view each frame before re-queueing.
//! - item order is established by the queuer and never re-sorted here.

use std::collections::{HashMap, HashSet};

use bevy::{
    core_pipeline::core_2d::Transparent2d, prelude::Resource, render::view::RetainedViewEntity,
};

#[derive(Default, Resource)]
pub(crate) struct OrderedTransparentUiDomSurfacesByView {
    pub(crate) items_by_view: HashMap<RetainedViewEntity, Vec<Transparent2d>>,
}

impl OrderedTransparentUiDomSurfacesByView {
    pub(crate) fn clear_live_views(&mut self, live_views: &HashSet<RetainedViewEntity>) {
        self.items_by_view.retain(|view, items| {
            let should_keep = live_views.contains(view);
            if should_keep {
                items.clear();
            }
            should_keep
        });
    }

    pub(crate) fn items_for_view_mut(
        &mut self,
        view: RetainedViewEntity,
    ) -> &mut Vec<Transparent2d> {
        self.items_by_view.entry(view).or_default()
    }

    pub(crate) fn items_for_view(&self, view: &RetainedViewEntity) -> Option<&[Transparent2d]> {
        self.items_by_view.get(view).map(Vec::as_slice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use bevy::prelude::Entity;

    #[test]
    fn views_are_created_empty_on_first_access() {
        let mut store = OrderedTransparentUiDomSurfacesByView::default();
        let view_a = RetainedViewEntity::new(Entity::from_bits(1).into(), None, 0);

        assert!(store.items_for_view(&view_a).is_none());
        let _ = store.items_for_view_mut(view_a);
        assert!(
            store
                .items_for_view(&view_a)
                .is_some_and(<[Transparent2d]>::is_empty)
        );
    }

    #[test]
    fn clear_live_views_drops_dead_views_and_keeps_live_ones() {
        let mut store = OrderedTransparentUiDomSurfacesByView::default();
        let view_a = RetainedViewEntity::new(Entity::from_bits(1).into(), None, 0);
        let view_b = RetainedViewEntity::new(Entity::from_bits(2).into(), None, 0);
        let _ = store.items_for_view_mut(view_a);
        let _ = store.items_for_view_mut(view_b);

        store.clear_live_views(&std::collections::HashSet::from([view_a]));

        assert!(
            store
                .items_for_view(&view_a)
                .is_some_and(<[Transparent2d]>::is_empty)
        );
        assert!(store.items_for_view(&view_b).is_none());
    }
}
