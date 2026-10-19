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
use crate::types::{DomBoxModel, DomNode, DomNodeSlotKey};

mod dom_layout_resolver;
mod flex;
mod flex_item_source;
mod flex_wrap_mode;
mod flow;
mod identity_dom_layout_resolver;
mod layout_pass_result;
mod measurement_flags;
mod measurement_record;
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

pub use dom_layout_resolver::DomLayoutResolver;
pub use identity_dom_layout_resolver::IdentityDomLayoutResolver;
pub use measurement_flags::MeasurementFlags;
pub use measurement_record::MeasurementRecord;

pub use walker::{node_box, node_layout_dirty, node_paint_dirty, root_key};

pub(crate) use style::{layer_kind_for_node, node_box_from_ref, parse_translate};
#[allow(unused_imports)]
pub(crate) use walker::{
    layer_offset_from_dom_climb, relative_offset_for_node, relayout_tree,
    relayout_tree_with_inherited, tree_needs_layout,
};
