//! Shared base state carried by every authored DOM node record.
//!
//! This file owns the node-local state bundle shared by every `DomNode` kind.
//! Its purpose is to keep owner identity, box model, intrinsic, layout, paint,
//! style cache, tree links, transitions, and event listeners in one honest
//! record without owning the document store or tree mutation policy.
//!
//! Invariants and rules:
//!
//! - this file owns only the `DomNodeBase` struct and its `impl` blocks
//! - every `DomNodeBase` is owned by exactly one document root key
//! - tree links and transitions are mutated through the owner root, not by
//!   callers holding a bare base reference
//! - event-listener buckets and exact-name presence bits stay synchronized
//!   through the `EventListenerNameBits` helpers

use std::collections::BTreeMap;

use crate::types::box_model::DomBoxModel;
use crate::types::dom_event_name::DomEventName;
use crate::types::dom_node::{
    DomEventListenerRegistration, DomIntrinsic, DomLayerKind, DomLayoutState, DomNodeSlotKey,
    DomPaintState, DomStyleObject, LayerOrder,
};
use crate::types::dom_transition::DomTransitionState;
use crate::types::event_listener_name_bits::EventListenerNameBits;

#[derive(Clone, Debug, PartialEq)]
pub struct DomNodeBase {
    /// Stable identity of the document that created this node.
    pub owner_document_key: DomNodeSlotKey,
    pub box_model: DomBoxModel,
    pub intrinsic: DomIntrinsic,
    pub layout: DomLayoutState,
    pub paint: DomPaintState,
    pub style: DomStyleObject,
    pub(crate) transitions: DomTransitionState,
    pub(crate) parent_key: Option<DomNodeSlotKey>,
    pub(crate) first_child_key: Option<DomNodeSlotKey>,
    pub(crate) last_child_key: Option<DomNodeSlotKey>,
    pub(crate) prev_sibling_key: Option<DomNodeSlotKey>,
    pub(crate) next_sibling_key: Option<DomNodeSlotKey>,
    pub(crate) event_listeners: BTreeMap<DomEventName, Vec<DomEventListenerRegistration>>,
    /// Exact event-name listener presence for non-bubbling dispatch gates.
    pub(crate) exact_listener_name_bits: EventListenerNameBits,
}

impl DomNodeBase {
    #[must_use]
    pub(crate) fn new(owner_document_key: DomNodeSlotKey) -> Self {
        Self {
            owner_document_key,
            box_model: DomBoxModel::zero(),
            intrinsic: DomIntrinsic::None,
            layout: DomLayoutState {
                dirty: true,
                layer_kind: DomLayerKind::None,
                layer_order: LayerOrder::Normal,
                available_width_px: 0.0,
                available_height_px: 0.0,
                width_px: 0.0,
                height_px: 0.0,
                content_height_px: 0.0,
            },
            paint: DomPaintState::FullLayerRepaint,
            style: DomStyleObject {
                dirty: true,
                ..DomStyleObject::default()
            },
            transitions: DomTransitionState::default(),
            parent_key: None,
            first_child_key: None,
            last_child_key: None,
            prev_sibling_key: None,
            next_sibling_key: None,
            event_listeners: BTreeMap::new(),
            exact_listener_name_bits: EventListenerNameBits::default(),
        }
    }
}
