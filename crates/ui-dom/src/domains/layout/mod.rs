//! Shared DOM-style layout module for `ui-dom`.
//!
//! This module is the single entry boundary for the `TreeWalker` layout engine.
//! `mod.rs` owns the public layout seam, while the private sibling files keep
//! property interpretation, flow solving, and traversal orchestration separate.
//!
//! Invariants and rules:
//!
//! - this module is the only module entrypoint for the layout folder
//! - the layout folder must not grow a second `TreeWalker` implementation
//! - public layout seams live here; private solver and property details stay in
//!   `style.rs`, `flow.rs`, `flex.rs`, and `walker.rs`
//! - this module exposes measurement and inspection seams, not app-local layout
//!   policy
//!
//! Advice and code philosophy:
//!
//! - keep this file small and boundary-focused
//! - push business decisions upward and push shared util downward
//! - if a helper is not part of the public layout seam, prefer keeping it in
//!   the narrower private file that owns the concept
//!
use crate::types::{DomBoxModel, DomError, DomNode, DomNodeSlotKey};

mod flex;
mod flow;
mod style;
mod walker;

/// Read-only measurement input for one DOM node during a shared layout walk.
#[derive(Clone, Copy, Debug)]
pub struct DomLayoutInput<'a> {
    pub node_key: DomNodeSlotKey,
    pub node: &'a DomNode,
    pub parent_box: Option<DomBoxModel>,
    pub image_size_px: (f32, f32),
}

/// Measurement seam used by the shared `TreeWalker` when intrinsic node size is needed.
pub trait DomLayoutResolver {
    fn measure_box(&mut self, input: DomLayoutInput<'_>) -> Result<DomBoxModel, DomError>;
}

/// Falls back to the current node-owned box, or the matched image size when geometry is absent.
#[derive(Clone, Copy, Debug, Default)]
pub struct IdentityDomLayoutResolver;

impl DomLayoutResolver for IdentityDomLayoutResolver {
    fn measure_box(&mut self, input: DomLayoutInput<'_>) -> Result<DomBoxModel, DomError> {
        style::intrinsic_box_for_node(input.node, input.image_size_px)
    }
}

pub use walker::{node_box, node_layout_dirty, node_paint_dirty, root_key};

pub(crate) use style::{layer_kind_for_node, node_box_from_ref};
#[allow(unused_imports)]
pub(crate) use walker::{
    layer_offset_from_dom_climb, relative_offset_for_node, relayout_tree,
    relayout_tree_with_inherited, tree_needs_layout,
};
