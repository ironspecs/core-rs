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
