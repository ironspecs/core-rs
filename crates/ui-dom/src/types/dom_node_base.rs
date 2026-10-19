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
//! - `presentation` (spine 3.3a) is written only by the style pass's
//!   projection; nothing else mutates it, and text nodes keep the default

use std::collections::BTreeMap;

use crate::domains::layout::MeasurementRecord;
use crate::domains::style::computed::NodePresentation;
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
    /// Per-node intrinsic measurement record written once per node per dirty
    /// pass by the layout up fold (flex step 1). A pass OUTPUT, never a dirt
    /// source: writes are equality-gated and mark no domain dirty.
    pub measurement: MeasurementRecord,
    pub layout: DomLayoutState,
    pub paint: DomPaintState,
    pub style: DomStyleObject,
    /// The seven computed presentation groups + the transition-longhand slot
    /// (PLAN.md spine 3.3a dual-write). Written by the style pass's projection
    /// from the node's stored presentation attrs; read only by the NEW
    /// computed-style view accessors. Text nodes carry the default and are
    /// never projected.
    pub presentation: NodePresentation,
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
            measurement: MeasurementRecord::default(),
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
            presentation: NodePresentation::default(),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_owner_document_key() -> DomNodeSlotKey {
        let mut keys = slotmap::SlotMap::with_key();
        keys.insert(())
    }

    #[test]
    fn new_base_starts_dirty_with_owner_key_and_empty_links() {
        let owner_key = test_owner_document_key();
        let base = DomNodeBase::new(owner_key);

        assert_eq!(base.owner_document_key, owner_key);
        assert_eq!(base.box_model, DomBoxModel::zero());
        assert_eq!(base.intrinsic, DomIntrinsic::None);
        // A freshly created node must get its first layout and paint pass, so
        // the base is born dirty in both domains. This is the actual contract.
        assert!(base.layout.dirty);
        assert!(base.paint.is_dirty());
        assert!(base.parent_key.is_none());
        assert!(base.first_child_key.is_none());
        assert!(base.last_child_key.is_none());
        assert!(base.prev_sibling_key.is_none());
        assert!(base.next_sibling_key.is_none());
        assert!(base.event_listeners.is_empty());
    }
}
