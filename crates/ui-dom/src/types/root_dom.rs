#![allow(dead_code)]

//! Root-owned DOM document and mutation boundary for `ui-dom`.
//!
//! This file represents the primary facade outsiders use to create, mutate,
//! attach, update, and detach one DOM document tree. Its purpose is to keep
//! document ownership, indexing, invalidation, and presentation lifecycle in
//! one root-owned place instead of scattering those responsibilities across
//! nodes or app crates.
//!
//! Invariants and rules:
//!
//! - `RootDom` owns the document slot store and all tree mutation authority
//! - `RootDom` owns the default-behavior lifecycle methods (activation, focus,
//!   blur, pointer, keyboard, and presented-options default behavior)
//! - attachment state here describes Bevy presentation lifecycle only
//! - node-level data must not absorb root-owned indexing or lifecycle policy
//!
//! Advice and code philosophy:
//!
//! - keep root APIs DOM-shaped and unsurprising
//! - push one-off helpers downward; keep ownership decisions here explicit
use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
};

use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
    },
    prelude::{Entity, World},
};

use crate::domains::composite::apply_vertical_scroll_from_point;
use crate::domains::layer_hit::any_layer_hit_from_root_point;
use crate::domains::layout::{layer_kind_for_node, layer_offset_from_dom_climb, node_box_from_ref};
use crate::domains::paint::{
    mark_full_repaint_up_to_layout_root, mark_node_dirty_up_to_layout_root,
};
use crate::domains::style::{
    ComputedStyleMap, apply_pseudo_state_style_declarations, matched_style_rules_for_element,
};
use crate::domains::visitor::event_target::{
    DomPointEventTarget, dispatch_event_from_point_and_finish, dispatch_event_to_target_and_finish,
    topmost_event_target_from_point,
};
use crate::domains::visitor::pseudo_state::{
    active_pointer_target_from_attached_point, active_pointer_target_from_point,
    sync_hover_from_attached_pointer,
};
use crate::domains::{ExternalLayerTarget, LayerLease, LayerPool, LayerPoolPolicy};
use crate::element_controls::{
    NumericStepDirection, ResolvedElementControl, range_value_string_from_point,
    resolved_control_part_from_point, resolved_element_control, resolved_text_edit_kind,
};
use crate::types::dom_attr_node::DomAttributeNode;
use crate::types::dom_attr_parse::{normalize_attribute_value, style_attr_allows_property};
use crate::types::dom_event::{
    DomEvent, DomEventDetail, DomEventListener, DomEventListenerOptions, DomEventName,
    DomFocusEvent, DomModifierState, DomMouseButton, DomMouseEvent, DomPointerEvent,
    DomPointerType, DomTransitionEvent, DomValueChangeEvent, DomWheelDeltaMode, DomWheelEvent,
};
use crate::types::dom_event_listener::{
    add_event_listener_to_node, remove_event_listener_from_node,
};
use crate::types::dom_node::{
    AttributeInvalidation, DomDocumentNode, DomIntrinsic, DomNode, DomNodeSlotKey, LayerOrder,
    attribute_changes_layer_membership, attribute_invalidation,
};
use crate::types::dom_pseudo_state::{DomPointerState, DomPseudoState};
use crate::types::dom_query_selector::{
    DomQuerySelectorStore, query_selector as query_selector_in_store,
    query_selector_all as query_selector_all_in_store,
    query_selector_all_from as query_selector_all_from_in_store,
    query_selector_from as query_selector_from_in_store,
};
use crate::types::dom_tag::{DomElementEditKind, DomTag};
use crate::types::dom_transition::{DomTransition, DomTransitionStore};
use crate::types::dom_transition::{
    DomTransitionConfig, TransitionLonghandValues, transition_longhand_values_from_shorthand,
    transition_spec_for_attribute,
};
use crate::types::element_behaviors::{
    ActiveRangeDragSession, DomElementAfterClick, DomElementAfterFocus, DomElementOnActivation,
    DomElementOnBlur, DomElementOnKeyboard, DomElementOnPointer, DomElementPresentOptionsBehavior,
    DomEventDispatchOutcome, PRESENTED_OPTIONS_HORIZONTAL_PADDING_PX, PRESENTED_OPTIONS_ROLE_ROW,
    PRESENTED_OPTIONS_ROLE_SCROLL, PRESENTED_OPTIONS_ROLE_SHELL, PRESENTED_OPTIONS_STATE_CLOSING,
    PRESENTED_OPTIONS_STATE_OPEN, PRESENTED_OPTIONS_VERTICAL_PADDING_PX, PresentedOptionRecord,
    PresentedOptionsOwnerKind, PresentedOptionsPopupAnchor, PressedPointerClickTarget,
    choose_presented_options_highlighted_index, collect_direct_child_element_keys,
    collect_presented_option_records_from_owner_source, measure_presented_options_popup_height_px,
    presented_options_popup_anchor_for_owner, presented_options_popup_state,
    presented_options_role_value, presented_options_row_content_height_px,
    presented_options_row_is_highlighted, presented_options_row_source_index,
    set_select_source_option_selected_attrs, string_attr_value,
};
use crate::types::primitives::PointPx;
use crate::types::root_dom_context::RootDomContext;
use crate::types::root_dom_fragment::RootDomFragment;
use crate::types::root_dom_metrics::RootDomMetricTally;
use crate::types::root_dom_nodes::RootDomNodes;
use crate::types::root_dom_nodes::{
    RootDomNodesMetricHistoryIter, RootDomNodesMetricName, RootDomNodesMetrics,
};
use crate::types::{
    DomAttribute, DomAttributeValue, DomError, DomEventListenerOwner, parse_authored_value_input,
};
use crate::types::{RootDomMetricHistoryIter, RootDomMetricName, RootDomMetrics};
use slotmap::SlotMap;

#[cfg(test)]
use crate::domains::hit_test::hit_test_from_dom_subtree_cluster_local;
#[cfg(test)]
use crate::domains::layer_hit::topmost_attached_layer_hit_from_root_point;
#[cfg(test)]
use crate::domains::paint::build_layer_paint_plan;
#[cfg(test)]
use crate::domains::visitor::inherited_style::InheritedStyleContext;
#[cfg(test)]
use crate::types::primitives::DomRect;
#[cfg(test)]
use crate::{LayerConfig, UiDomRenderSupport, domains::LayerPoolRenderJobQueue};

#[derive(Clone, Copy, Debug, PartialEq)]
struct PointerEventDispatchResult {
    target: DomPointEventTarget,
    default_prevented: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct HoveredLayerEntry {
    pub(crate) layer_root_key: DomNodeSlotKey,
    pub(crate) element_chain_root_to_target: Vec<DomNodeSlotKey>,
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct RootDomPseudoState {
    hovered_layers: Vec<HoveredLayerEntry>,
    hovered_layers_work: Vec<HoveredLayerEntry>,
    active_element_key: Option<DomNodeSlotKey>,
    focused_element_key: Option<DomNodeSlotKey>,
}

/// A text selection owned by one focused input-like element.
///
/// A caret is represented by a collapsed selection where
/// `selection_start == selection_end` and `selection_direction == None`.
/// Offsets use Unicode scalar indices into the authoritative stored `value`
/// string.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TextSelectionDirection {
    #[default]
    None,
    Forward,
    Backward,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextSelection {
    /// The element whose value is being selected.
    pub target_element: DomNodeSlotKey,
    /// The ordered start offset into the element value.
    pub selection_start: usize,
    /// The ordered end offset into the element value.
    pub selection_end: usize,
    /// The browser-like focus direction for this text selection.
    ///
    /// `Forward` means the live focus edge is at `selection_end`.
    /// `Backward` means the live focus edge is at `selection_start`.
    /// Collapsed carets must keep this at `None`.
    pub selection_direction: TextSelectionDirection,
}

/// A document selection spanning arbitrary DOM nodes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DocumentSelection {
    /// The anchor endpoint node for this selection.
    pub anchor_node: DomNodeSlotKey,
    /// The anchor endpoint offset within `anchor_node`.
    pub anchor_offset: usize,
    /// The focus endpoint node for this selection.
    pub focus_node: DomNodeSlotKey,
    /// The focus endpoint offset within `focus_node`.
    pub focus_offset: usize,
}

/// The single root-owned selection state.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DomSelection {
    #[default]
    None,
    TextSelected(TextSelection),
    DocumentSelected(DocumentSelection),
}

/// One explicit pointer action for a timestamped `RootDom::update(...)` call.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DomPointerUpdate {
    /// Keep the previous pointer-derived pseudo-state unchanged for this tick.
    Unchanged,
    /// Clear pointer-derived pseudo-state for this tick.
    Clear,
    /// Replace pointer-derived pseudo-state from one DOM-local pointer snapshot.
    Set(DomPointerState),
}

#[derive(Debug)]
pub struct RootDom {
    context: Arc<RootDomContext>,
    pub(crate) root: DomNodeSlotKey,
    pub(crate) nodes: RootDomNodes,
    id_lookup: HashMap<String, DomNodeSlotKey>,
    style_pass_attrs_by_node: HashMap<DomNodeSlotKey, BTreeSet<DomAttribute>>,
    pseudo_state: RootDomPseudoState,
    selection: DomSelection,
    transitions: DomTransitionStore,
    // Root-owned queue of finished DOM event instances awaiting ownership
    // transfer out of `update(...)`.
    //
    // Invariants:
    //
    // - store only owned `DomEvent` values here
    // - once an event enters this queue, it stays owned by the queue until
    //   `update(...)` moves it out with `std::mem::take(...)`
    // - do not clear this queue at the start of `update(...)`; pre-update
    //   events are still owned and must survive until drained
    // - do not clone queued events; `DomEvent` is move-only and this queue is
    //   the handoff boundary
    emitted_events: Vec<DomEvent>,
    attached_bevy_parent: Option<Entity>,
    pending_layer_append_roots: Vec<DomNodeSlotKey>,
    pending_layer_remove_roots: Vec<DomNodeSlotKey>,
    raised_layer_roots: Vec<DomNodeSlotKey>,
    layer_entity_by_root: HashMap<DomNodeSlotKey, Entity>,
    layer_lease_by_root: HashMap<DomNodeSlotKey, LayerLease>,
    vertical_scroll_offset_by_node: HashMap<DomNodeSlotKey, f32>,
    previous_pointer_state: Option<DomPointerState>,
    pub(crate) pressed_pointer_click_target: Option<PressedPointerClickTarget>,
    pub(crate) active_range_drag_session: Option<ActiveRangeDragSession>,
    metrics: RootDomMetricTally,
    clock_s: f64,
    style_runtime_revision: u64,
}

impl RootDom {
    /// Creates one root-owned DOM document with one `Document` root node and
    /// an empty element-id lookup.
    pub fn new(context: Arc<RootDomContext>) -> Self {
        let mut nodes = RootDomNodes::with_key();
        let root = nodes.insert_with_key(DomNode::document);
        let style_runtime_revision = context.style_runtime_revision();
        Self {
            context,
            root,
            nodes,
            id_lookup: HashMap::new(),
            style_pass_attrs_by_node: HashMap::new(),
            pseudo_state: RootDomPseudoState::default(),
            selection: DomSelection::None,
            transitions: DomTransitionStore::default(),
            emitted_events: Vec::new(),
            attached_bevy_parent: None,
            pending_layer_append_roots: Vec::new(),
            pending_layer_remove_roots: Vec::new(),
            raised_layer_roots: Vec::new(),
            layer_entity_by_root: HashMap::new(),
            layer_lease_by_root: HashMap::new(),
            vertical_scroll_offset_by_node: HashMap::new(),
            previous_pointer_state: None,
            pressed_pointer_click_target: None,
            active_range_drag_session: None,
            metrics: RootDomMetricTally::default(),
            clock_s: 0.0,
            style_runtime_revision,
        }
    }

    /// Returns the shared context this root uses for backend selection,
    /// density, and shared caches.
    #[must_use]
    pub fn context(&self) -> &Arc<RootDomContext> {
        &self.context
    }

    #[must_use]
    pub(crate) fn attached_bevy_parent_entity(&self) -> Option<Entity> {
        self.attached_bevy_parent
    }

    /// Returns the single document root node owned by this document together
    /// with its stable slot key.
    #[must_use]
    pub fn document_node(&self) -> (DomNodeSlotKey, &DomDocumentNode) {
        let node = self
            .nodes
            .get(self.root)
            .expect("document root should exist");
        match node {
            DomNode::Document(node) => (self.root, node),
            _ => panic!("document root should be a document node"),
        }
    }

    /// Creates one element node in the root-owned slot store and returns its
    /// external slot key for later linking and mutation.
    pub fn create_element(&mut self, tag: DomTag) -> Result<DomNodeSlotKey, DomError> {
        self.insert_node(DomNode::element(self.root, tag))
    }

    /// Creates one text node in the root-owned slot store and returns its
    /// external slot key for later linking and mutation.
    pub fn create_text_node(
        &mut self,
        data: impl Into<String>,
    ) -> Result<DomNodeSlotKey, DomError> {
        self.insert_node(DomNode::text(self.root, data.into().into()))
    }

    /// Returns one element by its `id` attribute using the root-owned O(1)
    /// lookup rather than walking the DOM tree.
    #[must_use]
    pub fn get_element_by_id(&self, id: &str) -> Option<(DomNodeSlotKey, &DomNode)> {
        let key = *self.id_lookup.get(id)?;
        let node = self.nodes.get(key)?;
        let element = node.as_element()?;
        if element.attrs.id.as_deref() != Some(id) {
            return None;
        }
        Some((key, node))
    }

    /// Returns one node by its stable slot key.
    ///
    /// This is the root-owned direct lookup seam for callers that already hold
    /// a document-local key. The key is only meaningful inside this `RootDom`'s
    /// slot store and is not a global node identity.
    #[must_use]
    pub fn node(&self, key: DomNodeSlotKey) -> Option<&DomNode> {
        self.nodes.get(key)
    }

    /// Returns the aggregate count for one named root metric.
    ///
    /// This mirrors `LayerPool::count(name)` so the owner-facing metrics read
    /// pattern stays the same across both retained metrics owners.
    #[must_use]
    pub fn count(&self, name: RootDomMetricName) -> u64 {
        self.metrics.count(name)
    }

    /// Returns one snapshot copy of the root-owned metrics.
    ///
    /// This is the secondary profiling API. Prefer `count(name)` for direct
    /// reads when the caller only needs one metric.
    #[must_use]
    pub fn metrics(&self) -> RootDomMetrics {
        self.metrics.metrics()
    }

    /// Returns retained metric increment history in oldest-to-newest order.
    ///
    /// History returns named retained entries so callers can inspect recent
    /// root mutations without string conversion or hidden slot-order folklore.
    #[must_use]
    pub fn history(&self) -> RootDomMetricHistoryIter<'_> {
        self.metrics.history()
    }

    /// Increments one root-owned metric counter and records the increment in history.
    ///
    /// This mutates only the root-owned metrics tally. It does not mutate DOM
    /// structure, style, layout, paint, event, or transition state. Call this
    /// only for work performed by a mutating `RootDom` operation.
    pub fn increment_metric(&mut self, name: RootDomMetricName) {
        self.metrics.increment(name);
    }

    /// Clears root-owned metric counters and history, then restarts the profiling window.
    pub fn reset_metrics(&mut self) {
        self.metrics.reset();
    }

    /// Returns the aggregate count for one named root node-store metric.
    #[must_use]
    pub fn node_count(&self, name: RootDomNodesMetricName) -> u64 {
        self.nodes.count(name)
    }

    /// Returns one snapshot copy of the root node-store metrics.
    #[must_use]
    pub fn node_metrics(&self) -> RootDomNodesMetrics {
        self.nodes.metrics()
    }

    /// Returns retained root node-store metric history in oldest-to-newest order.
    #[must_use]
    pub fn node_history(&self) -> RootDomNodesMetricHistoryIter {
        self.nodes.history()
    }

    /// Clears root node-store metric counters and history.
    pub fn reset_node_metrics(&mut self) {
        self.nodes.reset_metrics();
    }

    /// Attaches one event listener to the targeted node through the root-owned
    /// mutation boundary.
    pub fn add_event_listener(
        &mut self,
        key: DomNodeSlotKey,
        event_name: crate::types::dom_event::DomEventName,
        listener: impl Into<crate::types::dom_event::DomEventListener>,
        options: Option<crate::types::dom_event::DomEventListenerOptions>,
    ) -> Result<(), DomError> {
        <Self as crate::types::DomEventListenerOwner>::add_event_listener(
            self, key, event_name, listener, options,
        )
    }

    /// Removes one event listener from the targeted node and reports whether a
    /// matching registration existed.
    pub fn remove_event_listener(
        &mut self,
        key: DomNodeSlotKey,
        event_name: crate::types::dom_event::DomEventName,
        listener: impl Into<crate::types::dom_event::DomEventListener>,
        options: Option<crate::types::dom_event::DomEventListenerOptions>,
    ) -> Result<bool, DomError> {
        <Self as crate::types::DomEventListenerOwner>::remove_event_listener(
            self, key, event_name, listener, options,
        )
    }
}

impl DomEventListenerOwner for RootDom {
    fn add_event_listener(
        &mut self,
        key: DomNodeSlotKey,
        event_name: DomEventName,
        listener: impl Into<DomEventListener>,
        options: Option<DomEventListenerOptions>,
    ) -> Result<(), DomError> {
        let node = self.nodes.get_mut(key).ok_or(DomError::InvalidNodeKey)?;
        if add_event_listener_to_node(node, event_name, listener, options) {
            self.increment_metric(
                crate::types::root_dom_metrics::RootDomMetricName::EventListenerRegistrationAllocation,
            );
        }
        Ok(())
    }

    fn remove_event_listener(
        &mut self,
        key: DomNodeSlotKey,
        event_name: DomEventName,
        listener: impl Into<DomEventListener>,
        options: Option<DomEventListenerOptions>,
    ) -> Result<bool, DomError> {
        let node = self.nodes.get_mut(key).ok_or(DomError::InvalidNodeKey)?;
        Ok(remove_event_listener_from_node(
            node, event_name, listener, options,
        ))
    }
}

impl RootDom {
    /// Returns the first node matching the provided selector in document order.
    ///
    /// V1 selectors support `#id`, `.class`, `.class_a.class_b`, `tag`,
    /// `tag.class`, `tag.class_a.class_b`, and descendant
    /// spaces between those simple selectors. Unsupported or malformed syntax
    /// returns `DomError::InvalidSelector`.
    pub fn query_selector(
        &self,
        selector: &str,
    ) -> Result<Option<(DomNodeSlotKey, &DomNode)>, DomError> {
        query_selector_in_store(self.query_selector_store(), selector)
    }

    /// Returns all nodes matching the provided selector in document order.
    ///
    /// V1 selectors support `#id`, `.class`, `.class_a.class_b`, `tag`,
    /// `tag.class`, `tag.class_a.class_b`, and descendant
    /// spaces between those simple selectors. Unsupported or malformed syntax
    /// returns `DomError::InvalidSelector`.
    pub fn query_selector_all(
        &self,
        selector: &str,
    ) -> Result<Vec<(DomNodeSlotKey, &DomNode)>, DomError> {
        query_selector_all_in_store(self.query_selector_store(), selector)
    }

    /// Returns the first descendant of `from` matching the selector.
    ///
    /// This search is descendants-only and excludes `from` itself. V1 selectors
    /// support `#id`, `.class`, `.class_a.class_b`, `tag`, `tag.class`,
    /// `tag.class_a.class_b`, and descendant spaces
    /// between those simple selectors. Unsupported or malformed syntax returns
    /// `DomError::InvalidSelector`.
    pub fn query_selector_from(
        &self,
        from: DomNodeSlotKey,
        selector: &str,
    ) -> Result<Option<(DomNodeSlotKey, &DomNode)>, DomError> {
        query_selector_from_in_store(self.query_selector_store(), from, selector)
    }

    /// Returns all descendants of `from` matching the selector in document order.
    ///
    /// This search is descendants-only and excludes `from` itself. V1 selectors
    /// support `#id`, `.class`, `.class_a.class_b`, `tag`, `tag.class`,
    /// `tag.class_a.class_b`, and descendant spaces
    /// between those simple selectors. Unsupported or malformed syntax returns
    /// `DomError::InvalidSelector`.
    pub fn query_selector_all_from(
        &self,
        from: DomNodeSlotKey,
        selector: &str,
    ) -> Result<Vec<(DomNodeSlotKey, &DomNode)>, DomError> {
        query_selector_all_from_in_store(self.query_selector_store(), from, selector)
    }

    /// Returns the current active element key for this root, if any.
    #[must_use]
    pub fn active_element_key(&self) -> Option<DomNodeSlotKey> {
        self.pseudo_state.active_element_key
    }

    /// Returns the current focused element key for this root, if any.
    #[must_use]
    pub fn focused_element_key(&self) -> Option<DomNodeSlotKey> {
        self.pseudo_state.focused_element_key
    }

    /// Returns the current root-owned selection state.
    #[must_use]
    pub fn selection(&self) -> DomSelection {
        self.selection
    }

    pub(crate) fn set_selection(&mut self, selection: DomSelection) {
        self.selection = selection;
    }

    #[cfg(test)]
    pub(crate) fn set_border_box_for_test(
        &mut self,
        key: DomNodeSlotKey,
        left_px: f32,
        top_px: f32,
        width_px: f32,
        height_px: f32,
    ) {
        let node = self.nodes.get_mut(key).expect("node should exist");
        match node {
            DomNode::Document(document) => {
                document.base.box_model.border_box.left_px = left_px;
                document.base.box_model.border_box.top_px = top_px;
                document.base.box_model.border_box.width_px = width_px;
                document.base.box_model.border_box.height_px = height_px;
            }
            DomNode::DocumentFragment(fragment) => {
                fragment.base.box_model.border_box.left_px = left_px;
                fragment.base.box_model.border_box.top_px = top_px;
                fragment.base.box_model.border_box.width_px = width_px;
                fragment.base.box_model.border_box.height_px = height_px;
            }
            DomNode::Element(element) => {
                element.base.box_model.border_box.left_px = left_px;
                element.base.box_model.border_box.top_px = top_px;
                element.base.box_model.border_box.width_px = width_px;
                element.base.box_model.border_box.height_px = height_px;
            }
            DomNode::Text(text) => {
                text.base.box_model.border_box.left_px = left_px;
                text.base.box_model.border_box.top_px = top_px;
                text.base.box_model.border_box.width_px = width_px;
                text.base.box_model.border_box.height_px = height_px;
            }
        }
    }

    /// Returns whether the provided document-local point intersects any
    /// presented Bevy-backed layer owned by this root, including the
    /// document root layer.
    pub fn hits_any_layer_at_point(&self, x_px: f32, y_px: f32) -> Result<bool, DomError> {
        any_layer_hit_from_root_point(self, PointPx { x_px, y_px })
    }

    /// Acquires the caller-renderable retained image target for one
    /// already-presented DOM layer.
    ///
    /// This switches the retained layer to external policy so DOM-derived paint
    /// no longer overwrites the target. The addressed node must already be a
    /// live layer root with an attached retained lease.
    pub fn acquire_external_layer_target(
        &self,
        world: &mut World,
        node_key: DomNodeSlotKey,
    ) -> Result<ExternalLayerTarget, DomError> {
        if self.attached_bevy_parent.is_none() {
            return Err(DomError::InvalidParentTarget);
        }
        let node = self.node(node_key).ok_or(DomError::InvalidNodeKey)?;
        if node_key != self.root && layer_kind_for_node(node) == crate::types::DomLayerKind::None {
            return Err(DomError::InvalidNodeKey);
        }
        if self.layer_entity(node_key).is_none() {
            return Err(DomError::InvalidNodeKey);
        }
        let lease = self.layer_lease(node_key).ok_or(DomError::InvalidNodeKey)?;
        let target = LayerPool::acquire_external_layer_target_in_world(world, lease)
            .map_err(|_| DomError::RetainedGpuApplyFailed)?;
        LayerPool::set_layer_policy_in_world(world, lease, LayerPoolPolicy::External)
            .map_err(|_| DomError::RetainedGpuApplyFailed)?;
        Ok(target)
    }

    /// Advances one root, presents the result in Bevy when attached, and
    /// returns every owned DOM event currently queued for drain.
    ///
    /// Returned events can have been queued before this call or during the
    /// work performed by this call. Ownership leaves `RootDom` only through
    /// this returned `Vec<DomEvent>`.
    ///
    /// This is the only public root lifecycle update boundary. It fails fast
    /// with [`DomError::InvalidParentTarget`] when the root is not attached.
    pub fn update(
        &mut self,
        world: &mut World,
        now_s: f64,
        pointer: DomPointerUpdate,
    ) -> Result<Vec<DomEvent>, DomError> {
        self.clock_s = now_s;
        self.sync_stylesheet_runtime();
        let Some(parent) = self.attached_bevy_parent else {
            if matches!(pointer, DomPointerUpdate::Unchanged)
                && self.transitions.active_nodes.is_empty()
            {
                return Ok(std::mem::take(&mut self.emitted_events));
            }
            return Err(DomError::InvalidParentTarget);
        };
        if !self.transitions.active_nodes.is_empty() {
            self.sync_transitions_at_current_time();
            crate::domains::composite::update_style_layout(self, world, parent)?;
        }
        match pointer {
            DomPointerUpdate::Unchanged => {}
            DomPointerUpdate::Clear => {
                self.clear_pointer_state_for_update();
            }
            DomPointerUpdate::Set(pointer) => {
                self.sync_style_layout_before_pointer_update(world, parent)?;
                self.apply_pointer_update(world, parent, pointer)?;
            }
        }
        crate::domains::composite::update(self, world, parent, now_s)?;
        Ok(std::mem::take(&mut self.emitted_events))
    }

    fn apply_pointer_update(
        &mut self,
        world: &World,
        parent: Entity,
        pointer: DomPointerState,
    ) -> Result<(), DomError> {
        let previous_pointer = self.previous_pointer_state;
        self.dispatch_pointer_events_for_update(world, parent, previous_pointer, pointer)?;
        self.previous_pointer_state = Some(DomPointerState {
            scroll_delta_y_px: 0.0,
            ..pointer
        });
        Ok(())
    }

    pub(crate) fn apply_vertical_scroll_to_layer_root(
        &mut self,
        scroll_root_key: DomNodeSlotKey,
        delta_y_px: f32,
    ) -> Result<bool, DomError> {
        if delta_y_px == 0.0 {
            return Ok(false);
        }
        let node = self.node(scroll_root_key).ok_or(DomError::InvalidNodeKey)?;
        if !node_allows_vertical_scroll_offset(node) {
            return Ok(false);
        }
        let viewport_height_px = scroll_viewport_height_px_for_node(self, scroll_root_key)?;
        let content_height_px = scroll_content_height_px_for_node(self, scroll_root_key)?;
        let max_scroll_offset_y_px = (content_height_px - viewport_height_px).max(0.0);
        let current_offset_y_px = self
            .vertical_scroll_offset_by_node
            .get(&scroll_root_key)
            .copied()
            .unwrap_or(0.0);
        let next_offset_y_px =
            (current_offset_y_px - delta_y_px).clamp(0.0, max_scroll_offset_y_px);
        if (next_offset_y_px - current_offset_y_px).abs() <= f32::EPSILON {
            return Ok(false);
        }
        self.vertical_scroll_offset_by_node
            .insert(scroll_root_key, next_offset_y_px);
        Ok(true)
    }

    fn dispatch_pointer_events_for_update(
        &mut self,
        world: &World,
        parent: Entity,
        previous_pointer: Option<DomPointerState>,
        pointer: DomPointerState,
    ) -> Result<(), DomError> {
        let point = PointPx {
            x_px: pointer.x_px,
            y_px: pointer.y_px,
        };
        if pointer_moved(previous_pointer, pointer) {
            self.dispatch_pointer_move_for_update(world, parent, pointer, point)?;
        }
        if pointer_pressed(previous_pointer, pointer) {
            self.dispatch_pointer_down_and_apply_focus_for_update(world, parent, pointer)?;
        }
        if pointer_released(previous_pointer, pointer) {
            self.dispatch_pointer_up_for_update(world, parent, pointer)?;
        }
        if pointer.scroll_delta_y_px != 0.0 {
            self.dispatch_wheel_for_update(world, parent, pointer, point)?;
        }
        Ok(())
    }

    fn dispatch_pointer_move_for_update(
        &mut self,
        world: &World,
        parent: Entity,
        pointer: DomPointerState,
        point: PointPx,
    ) -> Result<(), DomError> {
        sync_hover_from_attached_pointer(self, world, hover_pointer_event(pointer))?;
        if pointer.primary_button_down {
            self.sync_active_from_attached_point(world, point)?;
        }
        let event = DomEvent::new(
            DomEventName::PointerMove,
            DomEventDetail::Pointer(DomPointerEvent {
                pointer_id: 1,
                pointer_type: DomPointerType::Mouse,
                is_primary: true,
                button: None,
                buttons: u16::from(pointer.primary_button_down),
                client_x: pointer.x_px,
                client_y: pointer.y_px,
                modifiers: DomModifierState::default(),
            }),
        );
        let _ = self.dispatch_pointer_event_from_point_and_record(world, parent, event)?;
        self.apply_default_range_drag_for_point(point)?;
        Ok(())
    }

    fn dispatch_pointer_down_and_apply_focus_for_update(
        &mut self,
        world: &World,
        parent: Entity,
        pointer: DomPointerState,
    ) -> Result<(), DomError> {
        let point = PointPx {
            x_px: pointer.x_px,
            y_px: pointer.y_px,
        };
        let raw_target_key =
            self.point_event_target_key_from_attached_point(world, parent, point)?;
        sync_hover_from_attached_pointer(self, world, hover_pointer_event(pointer))?;
        self.sync_active_from_attached_point(world, point)?;
        let event = DomEvent::new(
            DomEventName::PointerDown,
            DomEventDetail::Pointer(DomPointerEvent {
                pointer_id: 1,
                pointer_type: DomPointerType::Mouse,
                is_primary: true,
                button: Some(DomMouseButton::Primary),
                buttons: 1,
                client_x: pointer.x_px,
                client_y: pointer.y_px,
                modifiers: DomModifierState::default(),
            }),
        );
        let dispatch_result =
            self.dispatch_pointer_event_from_point_and_record(world, parent, event)?;
        let accepted_target_key = dispatch_result
            .as_ref()
            .filter(|result| !result.default_prevented)
            .map(|result| result.target.target_key);
        let default_target = dispatch_result
            .filter(|result| !result.default_prevented)
            .map(|result| result.target);
        self.apply_primary_pointer_press_focus_change(accepted_target_key)?;
        self.apply_pointer_down_default_lifecycles(point, raw_target_key, default_target)
    }

    fn dispatch_pointer_up_for_update(
        &mut self,
        world: &World,
        parent: Entity,
        pointer: DomPointerState,
    ) -> Result<(), DomError> {
        let point = PointPx {
            x_px: pointer.x_px,
            y_px: pointer.y_px,
        };
        let raw_target_key =
            self.point_event_target_key_from_attached_point(world, parent, point)?;
        sync_hover_from_attached_pointer(self, world, hover_pointer_event(pointer))?;
        self.clear_active();
        let event = DomEvent::new(
            DomEventName::PointerUp,
            DomEventDetail::Pointer(DomPointerEvent {
                pointer_id: 1,
                pointer_type: DomPointerType::Mouse,
                is_primary: true,
                button: Some(DomMouseButton::Primary),
                buttons: 0,
                client_x: pointer.x_px,
                client_y: pointer.y_px,
                modifiers: DomModifierState::default(),
            }),
        );
        let _ = self.dispatch_pointer_event_from_point_and_record(world, parent, event)?;
        self.apply_pointer_up_default_lifecycles(point, raw_target_key)?;
        Ok(())
    }

    fn dispatch_wheel_for_update(
        &mut self,
        world: &World,
        parent: Entity,
        pointer: DomPointerState,
        point: PointPx,
    ) -> Result<(), DomError> {
        let _ = apply_vertical_scroll_from_point(
            self,
            world,
            parent,
            point,
            pointer.scroll_delta_y_px,
        )?;
        sync_hover_from_attached_pointer(self, world, hover_pointer_event(pointer))?;
        let event = DomEvent::new(
            DomEventName::Wheel,
            DomEventDetail::Wheel(DomWheelEvent {
                delta_x: 0.0,
                delta_y: pointer.scroll_delta_y_px,
                delta_z: 0.0,
                delta_mode: DomWheelDeltaMode::Pixel,
                client_x: pointer.x_px,
                client_y: pointer.y_px,
                modifiers: DomModifierState::default(),
            }),
        );
        let _ = self.dispatch_pointer_event_from_point_and_record(world, parent, event)?;
        Ok(())
    }

    fn dispatch_pointer_event_from_point_and_record(
        &mut self,
        world: &World,
        parent: Entity,
        event: DomEvent,
    ) -> Result<Option<PointerEventDispatchResult>, DomError> {
        let Some(point) = event_point(&event) else {
            return Ok(None);
        };
        let Some(dispatch) =
            dispatch_event_from_point_and_finish(self, world, parent, point, event)?
        else {
            return Ok(None);
        };
        let crate::domains::visitor::event_target::DomPointEventDispatch { target, event } =
            dispatch;
        let default_prevented = event.default_prevented;
        if !event.default_prevented && !event.propagation.stopped {
            self.push_emitted_event(event);
        }
        Ok(Some(PointerEventDispatchResult {
            target,
            default_prevented,
        }))
    }

    fn apply_primary_pointer_press_focus_change(
        &mut self,
        accepted_target_key: Option<DomNodeSlotKey>,
    ) -> Result<(), DomError> {
        let Some(mut key) = accepted_target_key else {
            if self.pseudo_state.hovered_layers.is_empty() {
                self.blur();
            }
            return Ok(());
        };
        loop {
            if self.validate_focus_target(key).is_ok() {
                let next_focus_visible =
                    self.node(key)
                        .and_then(DomNode::as_element)
                        .is_some_and(|element| {
                            !matches!(resolved_text_edit_kind(element), DomElementEditKind::None)
                        });
                self.focus_with_focus_visible(key, next_focus_visible)?;
                return Ok(());
            }
            let Some(parent_key) = self.node(key).ok_or(DomError::InvalidNodeKey)?.parent_key()
            else {
                self.blur();
                return Ok(());
            };
            key = parent_key;
        }
    }

    fn sync_active_from_attached_point(
        &mut self,
        world: &World,
        point: PointPx,
    ) -> Result<(), DomError> {
        let next_active_element_key =
            active_pointer_target_from_attached_point(self, world, point)?;
        self.replace_active_element_key(next_active_element_key);
        Ok(())
    }

    fn sync_active_from_point(&mut self, point: PointPx) -> Result<(), DomError> {
        let next_active_element_key = active_pointer_target_from_point(self, point)?;
        self.replace_active_element_key(next_active_element_key);
        Ok(())
    }

    pub(crate) fn vertical_scroll_offset_y_px(&self, key: DomNodeSlotKey) -> f32 {
        let Some(node) = self.node(key) else {
            return 0.0;
        };
        if !node_allows_vertical_scroll_offset(node) {
            return 0.0;
        }
        self.vertical_scroll_offset_by_node
            .get(&key)
            .copied()
            .unwrap_or(0.0)
    }

    fn sync_stylesheet_runtime(&mut self) {
        let next_revision = self.context.style_runtime_revision();
        if next_revision == self.style_runtime_revision {
            return;
        }
        self.style_runtime_revision = next_revision;
        self.mark_style_dirty_for_attribute_change(self.root);
    }

    /// Clears pointer-derived pseudo-state (`:hover` and `:active`) for this root.
    fn clear_pointer_state_internal(&mut self) {
        let hovered_layers = self.take_hovered_layers();
        for hovered_layer in hovered_layers {
            for key in hovered_layer.element_chain_root_to_target {
                let _ = self.clear_pseudo_state_for_element(key, DomPseudoState::Hover);
            }
        }
        self.clear_active();
    }

    fn clear_pointer_state_for_update(&mut self) {
        self.previous_pointer_state = None;
        self.clear_pointer_state_internal();
        self.pressed_pointer_click_target = None;
        self.active_range_drag_session = None;
    }

    fn sync_style_layout_before_pointer_update(
        &mut self,
        world: &mut World,
        parent: Entity,
    ) -> Result<(), DomError> {
        let style_dirty =
            crate::domains::visitor::style_recompute::style_tree_needs_recompute(self, self.root)?;
        let layout_dirty = crate::domains::layout::tree_needs_layout(self, self.root)?;
        if !style_dirty && !layout_dirty {
            return Ok(());
        }
        crate::domains::composite::update_style_layout(self, world, parent)
    }

    /// Focuses the targeted element for this root.
    pub fn focus(&mut self, key: DomNodeSlotKey) -> Result<(), DomError> {
        let next_focus_visible =
            self.node(key)
                .and_then(DomNode::as_element)
                .is_some_and(|element| {
                    !matches!(resolved_text_edit_kind(element), DomElementEditKind::None)
                })
                || self
                    .pseudo_state
                    .focused_element_key
                    .and_then(|focused_key| self.node(focused_key))
                    .and_then(DomNode::as_element)
                    .is_some_and(|element| element.has_pseudo_state(DomPseudoState::FocusVisible));
        self.focus_with_focus_visible(key, next_focus_visible)
    }

    fn focus_with_focus_visible(
        &mut self,
        key: DomNodeSlotKey,
        next_focus_visible: bool,
    ) -> Result<(), DomError> {
        self.validate_focus_target(key)?;
        if self.pseudo_state.focused_element_key == Some(key) {
            if next_focus_visible {
                let _ = self.set_pseudo_state_for_element(key, DomPseudoState::FocusVisible);
            } else {
                let _ = self.clear_pseudo_state_for_element(key, DomPseudoState::FocusVisible);
            }
            return Ok(());
        }
        let previous_focus = self.pseudo_state.focused_element_key.take();
        if let Some(previous_key) = previous_focus {
            assert!(
                self.clear_pseudo_state_for_element(previous_key, DomPseudoState::Focus),
                "focused element key should reference a live element node"
            );
            let _ = self.clear_pseudo_state_for_element(previous_key, DomPseudoState::FocusVisible);
            // Keep the previous text selection live until the old target's
            // focus-loss lifecycle finishes.
            self.dispatch_focus_transition_event(previous_key, DomEventName::FocusOut, Some(key))?;
            self.dispatch_focus_transition_event(previous_key, DomEventName::Blur, Some(key))?;
        }
        self.set_pseudo_state_for_element(key, DomPseudoState::Focus)?;
        if next_focus_visible {
            self.set_pseudo_state_for_element(key, DomPseudoState::FocusVisible)?;
        }
        self.pseudo_state.focused_element_key = Some(key);
        self.ensure_text_selection_for_target(key);
        self.apply_after_focus(key)?;
        self.dispatch_focus_transition_event(key, DomEventName::FocusIn, previous_focus)?;
        self.dispatch_focus_transition_event(key, DomEventName::Focus, previous_focus)?;
        Ok(())
    }

    /// Blurs the current focused element, if any.
    pub fn blur(&mut self) {
        let Some(key) = self.pseudo_state.focused_element_key.take() else {
            self.selection = DomSelection::None;
            return;
        };
        assert!(
            self.clear_pseudo_state_for_element(key, DomPseudoState::Focus),
            "focused element key should reference a live element node"
        );
        let _ = self.clear_pseudo_state_for_element(key, DomPseudoState::FocusVisible);
        // Keep the current text selection live until the focus-loss lifecycle
        // finishes for this target.
        self.dispatch_focus_transition_event(key, DomEventName::FocusOut, None)
            .expect("live focused element should dispatch focusout");
        self.dispatch_focus_transition_event(key, DomEventName::Blur, None)
            .expect("live focused element should dispatch blur");
        self.apply_on_blur(key)
            .expect("live blurred target should finish blur lifecycle");
        self.selection = DomSelection::None;
    }

    /// Attaches this DOM tree to one foreign Bevy parent and performs the
    /// initial composite setup immediately.
    pub fn attach_to_bevy_parent_entity(
        &mut self,
        world: &mut World,
        parent: Entity,
        _now_s: f64,
    ) -> Result<(), DomError> {
        if self.attached_bevy_parent.is_some() {
            return Err(DomError::InvalidParentTarget);
        }
        self.attached_bevy_parent = Some(parent);
        crate::domains::register_render_ops_handler(world, &self.context.render_ops_shared());
        if let Err(error) = crate::domains::composite::create(self, world, parent) {
            self.attached_bevy_parent = None;
            return Err(error);
        }
        Ok(())
    }

    /// Detaches this DOM tree from its Bevy parent and releases all ui-dom
    /// layer entities under that parent.
    pub fn detach_from_bevy_parent_entity(&mut self, world: &mut World) -> Result<(), DomError> {
        let parent = self
            .attached_bevy_parent
            .ok_or(DomError::InvalidParentTarget)?;
        crate::domains::composite::destroy(self, world, parent)?;
        self.attached_bevy_parent = None;
        Ok(())
    }

    /// Appends one child to one parent in this root-owned document and returns
    /// the appended child key.
    pub fn append_child(
        &mut self,
        parent_key: DomNodeSlotKey,
        child_key: DomNodeSlotKey,
    ) -> Result<DomNodeSlotKey, DomError> {
        if self
            .node(parent_key)
            .and_then(|node| node.last_child(|key| self.node(key)))
            .is_some_and(|(last_child_key, _)| last_child_key == child_key)
            && self
                .node(child_key)
                .and_then(DomNode::parent_key)
                .is_some_and(|current_parent_key| current_parent_key == parent_key)
        {
            return Ok(child_key);
        }
        let previous_parent_key = self.node(child_key).and_then(DomNode::parent_key);
        let appended_key = append_child_to_parent(&mut self.nodes, parent_key, child_key)?;
        if let Some(previous_parent_key) = previous_parent_key {
            mark_dirty_up_to_layer_root(self, previous_parent_key, DirtyFlag::Layout);
            mark_full_repaint_up_to_layout_root(self, previous_parent_key);
        } else {
            mark_newly_attached_subtree_dirty(self, appended_key);
            queue_newly_connected_layer_roots_in_subtree(self, appended_key);
        }
        mark_dirty_up_to_layer_root(self, parent_key, DirtyFlag::Layout);
        // Style matching depends on tree position (type selectors, class
        // rules, inheritance), so an insertion must arm the enclosing layer
        // root's style phase just like attribute changes and hover do;
        // otherwise the new subtree is laid out and painted with unstyled
        // attrs until some later trigger marks the layer root style-dirty.
        mark_dirty_up_to_layer_root(self, parent_key, DirtyFlag::Style);
        mark_full_repaint_up_to_layout_root(self, parent_key);
        if self
            .node(appended_key)
            .is_some_and(|node| layer_kind_for_node(node) != crate::types::DomLayerKind::None)
        {
            mark_dirty_up_to_layer_root(self, appended_key, DirtyFlag::Layout);
            mark_full_repaint_up_to_layout_root(self, appended_key);
        }
        Ok(appended_key)
    }

    /// Consumes one detached fragment and imports its root children under the
    /// provided live document parent in DOM order.
    pub fn append_fragment(
        &mut self,
        parent_key: DomNodeSlotKey,
        fragment: RootDomFragment,
    ) -> Result<(), DomError> {
        let parent = self.node(parent_key).ok_or(DomError::InvalidNodeKey)?;
        if !parent.is_parent_node() {
            return Err(DomError::InvalidParentTarget);
        }
        if fragment
            .node(fragment.document_fragment_key())
            .and_then(DomNode::first_child_key_for_store)
            .is_none()
        {
            return Ok(());
        }
        self.increment_metric(RootDomMetricName::FragmentTopLevelKeyListAllocation);
        self.increment_metric(RootDomMetricName::FragmentImportValidationStackAllocation);
        let imported_keys = self.import_fragment_children_into_parent(parent_key, &fragment)?;
        for imported_key in imported_keys {
            self.queue_layer_append_root(imported_key);
        }
        drop(fragment);
        Ok(())
    }

    /// Inserts one node into the root-owned slot store and updates the element
    /// id lookup if that node is an element carrying an `id` attribute.
    fn insert_node(&mut self, node: DomNode) -> Result<DomNodeSlotKey, DomError> {
        self.assert_id_is_available(
            node.as_element()
                .and_then(|element| element.attrs.id.as_deref()),
        )?;
        let mut node = node;
        node.after_create_intrinsic_update()?;
        self.record_intrinsic_metrics_for_node(&node);
        let is_text_node = matches!(node, DomNode::Text(_));
        let key = self.nodes.insert(node);
        self.increment_metric(RootDomMetricName::NodeSlotAllocation);
        if is_text_node {
            self.increment_metric(RootDomMetricName::TextStorageAllocation);
        }
        self.index_node_id_if_present(key);
        Ok(key)
    }

    fn record_intrinsic_metrics_for_node(&mut self, node: &DomNode) {
        match node.intrinsic() {
            DomIntrinsic::Text(_) => {
                self.increment_metric(RootDomMetricName::TextIntrinsicMeasurement);
            }
            DomIntrinsic::Img(_) => {
                self.increment_metric(RootDomMetricName::ImgIntrinsicRecomputation);
                self.increment_metric(RootDomMetricName::ImgIntrinsicStateUpdate);
            }
            DomIntrinsic::NotMeasured
                if matches!(
                    node,
                    DomNode::Element(element)
                        if element.behavior().intrinsic_kind == crate::types::DomElementIntrinsicKind::Img
                ) =>
            {
                self.increment_metric(RootDomMetricName::ImgIntrinsicStateUpdate);
            }
            _ => {}
        }
    }

    fn query_selector_store(&self) -> DomQuerySelectorStore<'_> {
        DomQuerySelectorStore {
            root: self.root,
            nodes: &self.nodes,
            id_lookup: &self.id_lookup,
        }
    }

    pub(crate) fn take_hovered_layers(&mut self) -> Vec<HoveredLayerEntry> {
        std::mem::take(&mut self.pseudo_state.hovered_layers)
    }

    pub(crate) fn store_hovered_layers(&mut self, hovered_layers: Vec<HoveredLayerEntry>) {
        self.pseudo_state.hovered_layers = hovered_layers;
    }

    pub(crate) fn take_hovered_layers_work(&mut self) -> Vec<HoveredLayerEntry> {
        std::mem::take(&mut self.pseudo_state.hovered_layers_work)
    }

    pub(crate) fn store_hovered_layers_work(
        &mut self,
        hovered_layers_work: Vec<HoveredLayerEntry>,
    ) {
        self.pseudo_state.hovered_layers_work = hovered_layers_work;
    }

    fn set_pseudo_state_for_element(
        &mut self,
        key: DomNodeSlotKey,
        pseudo_state: DomPseudoState,
    ) -> Result<(), DomError> {
        let changed = {
            let node = self.nodes.get_mut(key).ok_or(DomError::InvalidNodeKey)?;
            let element = node
                .as_element_mut()
                .ok_or(DomError::InvalidPseudoStateTarget)?;
            if element.has_pseudo_state(pseudo_state) {
                return Ok(());
            }
            let style_sensitive = self.context.uses_pseudo_state_styles(pseudo_state)
                || element.style().has_pseudo_state_declarations(pseudo_state);
            element.set_pseudo_state(pseudo_state);
            style_sensitive
        };
        if changed {
            self.mark_style_dirty_for_attribute_change(key);
        }
        Ok(())
    }

    fn clear_pseudo_state_for_element(
        &mut self,
        key: DomNodeSlotKey,
        pseudo_state: DomPseudoState,
    ) -> bool {
        let changed = {
            let Some(element) = self.nodes.get_mut(key).and_then(DomNode::as_element_mut) else {
                return false;
            };
            if !element.has_pseudo_state(pseudo_state) {
                return false;
            }
            let style_sensitive = self.context.uses_pseudo_state_styles(pseudo_state)
                || element.style().has_pseudo_state_declarations(pseudo_state);
            element.clear_pseudo_state(pseudo_state);
            style_sensitive
        };
        if changed {
            self.mark_style_dirty_for_attribute_change(key);
        }
        true
    }

    fn queue_layer_append_root(&mut self, key: DomNodeSlotKey) {
        let Some(node) = self.node(key) else {
            return;
        };
        if layer_kind_for_node(node) == crate::types::DomLayerKind::None {
            return;
        }
        if self.attached_bevy_parent.is_some()
            && node_is_connected_to_root(&self.nodes, self.root, key)
        {
            push_unique_key(&mut self.pending_layer_append_roots, key);
        }
    }

    fn queue_layer_remove_root(&mut self, key: DomNodeSlotKey) {
        if self.attached_bevy_parent.is_some() {
            push_unique_key(&mut self.pending_layer_remove_roots, key);
        }
    }

    pub(crate) fn drain_pending_layer_append_roots(&mut self) -> Vec<DomNodeSlotKey> {
        std::mem::take(&mut self.pending_layer_append_roots)
    }

    pub(crate) fn drain_pending_layer_remove_roots(&mut self) -> Vec<DomNodeSlotKey> {
        std::mem::take(&mut self.pending_layer_remove_roots)
    }

    pub(crate) fn register_layer_entity(&mut self, key: DomNodeSlotKey, entity: Entity) {
        self.layer_entity_by_root.insert(key, entity);
        self.register_raised_layer_root(key);
    }

    pub(crate) fn register_layer_lease(&mut self, key: DomNodeSlotKey, lease: LayerLease) {
        self.layer_lease_by_root.insert(key, lease);
    }

    pub(crate) fn unregister_layer_entity(&mut self, key: DomNodeSlotKey) {
        self.layer_entity_by_root.remove(&key);
        self.layer_lease_by_root.remove(&key);
        self.unregister_raised_layer_root(key);
    }

    pub(crate) fn raised_layer_roots(&self) -> &[DomNodeSlotKey] {
        &self.raised_layer_roots
    }

    pub(crate) fn set_layout_layer_order(
        &mut self,
        key: DomNodeSlotKey,
        layer_order: LayerOrder,
    ) -> Result<(), DomError> {
        let Some(node) = self.nodes.get_mut(key) else {
            return Err(DomError::InvalidNodeKey);
        };
        let current_layer_order = match node {
            DomNode::Document(document) => &mut document.base.layout.layer_order,
            DomNode::DocumentFragment(fragment) => &mut fragment.base.layout.layer_order,
            DomNode::Element(element) => &mut element.base.layout.layer_order,
            DomNode::Text(text) => &mut text.base.layout.layer_order,
        };
        if *current_layer_order == layer_order {
            return Ok(());
        }
        *current_layer_order = layer_order;
        if self.layer_entity(key).is_some() {
            self.unregister_raised_layer_root(key);
            self.register_raised_layer_root(key);
        }
        Ok(())
    }

    fn register_raised_layer_root(&mut self, key: DomNodeSlotKey) {
        let Some(node) = self.node(key) else {
            return;
        };
        if layer_kind_for_node(node) == crate::types::DomLayerKind::None {
            return;
        }
        if node.layer_order() != LayerOrder::Raised {
            return;
        }
        push_unique_key(&mut self.raised_layer_roots, key);
    }

    fn unregister_raised_layer_root(&mut self, key: DomNodeSlotKey) {
        self.raised_layer_roots
            .retain(|current_key| *current_key != key);
    }

    pub(crate) fn layer_entity(&self, key: DomNodeSlotKey) -> Option<Entity> {
        self.layer_entity_by_root.get(&key).copied()
    }

    pub(crate) fn layer_root_for_entity(&self, entity: Entity) -> Option<DomNodeSlotKey> {
        self.layer_entity_by_root
            .iter()
            .find_map(|(key, layer_entity)| {
                if *layer_entity == entity {
                    Some(*key)
                } else {
                    None
                }
            })
    }

    /// Returns the retained-layer lease for one live layer root, if any.
    ///
    /// The lease is an opaque `Copy` capability that identifies a retained
    /// surface; by itself it grants no mutation authority.
    pub fn layer_lease(&self, key: DomNodeSlotKey) -> Option<LayerLease> {
        self.layer_lease_by_root.get(&key).copied()
    }

    /// Returns the presented offset of one live layer root within this root's
    /// presented space: the accumulation of `layer_offset_from_dom_climb` over
    /// the layer-ancestor chain — the same accumulation the composite pass
    /// performs down the Bevy layer hierarchy when it places layer entities.
    /// The value includes ancestor-scroll adjustment: each climb subtracts the
    /// ancestor layer's vertical scroll offset, matching layer hit-test
    /// coordinates.
    ///
    /// Returns `None` when `key` does not name a live layer root.
    #[allow(clippy::similar_names)]
    #[must_use]
    pub fn layer_offset_px(&self, key: DomNodeSlotKey) -> Option<[f32; 2]> {
        let node = self.node(key)?;
        if key != self.root && layer_kind_for_node(node) == crate::types::DomLayerKind::None {
            return None;
        }
        let mut total_x_px = 0.0;
        let mut total_y_px = 0.0;
        let mut current_key = Some(key);
        while let Some(current) = current_key {
            if current == self.root {
                break;
            }
            let current_node = self.node(current)?;
            if layer_kind_for_node(current_node) != crate::types::DomLayerKind::None {
                let offset = layer_offset_from_dom_climb(self, current).ok()?;
                total_x_px += offset.x_px;
                total_y_px += offset.y_px;
            }
            current_key = current_node.parent_key();
        }
        Some([total_x_px, total_y_px])
    }

    fn import_fragment_children_into_parent(
        &mut self,
        parent_key: DomNodeSlotKey,
        fragment: &RootDomFragment,
    ) -> Result<Vec<DomNodeSlotKey>, DomError> {
        let Some(parent) = self.node(parent_key) else {
            return Err(DomError::InvalidNodeKey);
        };
        if !parent.is_parent_node() {
            return Err(DomError::InvalidParentTarget);
        }
        let Some(mut fragment_key) = first_fragment_import_key(fragment)? else {
            return Ok(Vec::new());
        };
        self.assert_fragment_ids_are_available_in_root(fragment)?;
        let fragment_root = fragment.document_fragment_key();
        let mut imported_top_level_keys = Vec::new();
        let mut imported_key_by_fragment_key = HashMap::new();
        loop {
            let fragment_node = fragment
                .node(fragment_key)
                .ok_or(DomError::InvalidNodeKey)?;
            let live_parent_key = parent_key_for_imported_fragment_node(
                fragment,
                fragment_root,
                fragment_key,
                parent_key,
                &imported_key_by_fragment_key,
            )?;
            let imported_key =
                self.import_fragment_node_into_parent(live_parent_key, fragment_node)?;
            imported_key_by_fragment_key.insert(fragment_key, imported_key);
            if fragment_node.parent_key() == Some(fragment_root) {
                imported_top_level_keys.push(imported_key);
            }
            let Some(next_fragment_key) =
                next_fragment_import_key(fragment, fragment_root, fragment_key)?
            else {
                break;
            };
            fragment_key = next_fragment_key;
        }
        mark_dirty_up_to_layer_root(self, parent_key, DirtyFlag::Layout);
        // Imported nodes are born dirty on their own flags, but the style
        // and paint gates read the enclosing layer root, so the parent chain
        // must propagate all three flags for the first update to restyle,
        // relayout, and repaint the imported subtree.
        mark_dirty_up_to_layer_root(self, parent_key, DirtyFlag::Style);
        mark_full_repaint_up_to_layout_root(self, parent_key);
        Ok(imported_top_level_keys)
    }

    fn import_fragment_node_into_parent(
        &mut self,
        parent_key: DomNodeSlotKey,
        fragment_node: &DomNode,
    ) -> Result<DomNodeSlotKey, DomError> {
        let mut imported_node = match fragment_node {
            DomNode::Document(_) | DomNode::DocumentFragment(_) => {
                return Err(DomError::InvalidParentTarget);
            }
            DomNode::Element(_) => fragment_node.clone_for_owner_document(self.root),
            DomNode::Text(text_node) => DomNode::text(self.root, text_node.data().into()),
        };
        imported_node.after_create_intrinsic_update()?;
        self.record_intrinsic_metrics_for_node(&imported_node);
        let is_text_node = matches!(imported_node, DomNode::Text(_));
        mark_node_born_dirty(&mut imported_node);
        let imported_key = self.nodes.insert(imported_node);
        self.increment_metric(RootDomMetricName::NodeSlotAllocation);
        if is_text_node {
            self.increment_metric(RootDomMetricName::TextStorageAllocation);
        }
        self.index_node_id_if_present(imported_key);
        append_child_to_parent(&mut self.nodes, parent_key, imported_key)?;
        Ok(imported_key)
    }

    fn replace_active_element_key(&mut self, next_active_element_key: Option<DomNodeSlotKey>) {
        if self.pseudo_state.active_element_key == next_active_element_key {
            return;
        }
        self.clear_active();
        let Some(key) = next_active_element_key else {
            return;
        };
        if self
            .set_pseudo_state_for_element(key, DomPseudoState::Active)
            .is_ok()
        {
            self.pseudo_state.active_element_key = Some(key);
        }
    }

    fn assert_fragment_ids_are_available_in_root(
        &self,
        fragment: &RootDomFragment,
    ) -> Result<(), DomError> {
        let Some(mut fragment_key) = first_fragment_import_key(fragment)? else {
            return Ok(());
        };
        let fragment_root = fragment.document_fragment_key();
        loop {
            let fragment_node = fragment
                .node(fragment_key)
                .ok_or(DomError::InvalidNodeKey)?;
            self.assert_id_is_available(
                fragment_node
                    .as_element()
                    .and_then(|element| element.attrs.id.as_deref()),
            )?;
            let Some(next_fragment_key) =
                next_fragment_import_key(fragment, fragment_root, fragment_key)?
            else {
                return Ok(());
            };
            fragment_key = next_fragment_key;
        }
    }

    fn clear_active(&mut self) {
        let Some(key) = self.pseudo_state.active_element_key.take() else {
            return;
        };
        let _ = self.clear_pseudo_state_for_element(key, DomPseudoState::Active);
    }

    pub(crate) fn validate_focus_target(&self, key: DomNodeSlotKey) -> Result<(), DomError> {
        let element = self
            .node(key)
            .ok_or(DomError::InvalidNodeKey)?
            .as_element()
            .ok_or(DomError::InvalidPseudoStateTarget)?;
        if matches!(
            element.attrs.attributes.get(&DomAttribute::Disabled),
            Some(DomAttributeValue::Bool(true))
        ) {
            return Err(DomError::InvalidPseudoStateTarget);
        }
        if matches!(
            element.attrs.attributes.get(&DomAttribute::TabIndex),
            Some(DomAttributeValue::Integer(tab_index)) if *tab_index >= 0
        ) || !element
            .attrs
            .attributes
            .contains_key(&DomAttribute::TabIndex)
            && element.behavior().focusable
        {
            return Ok(());
        }
        Err(DomError::InvalidPseudoStateTarget)
    }

    fn clear_pseudo_state_for_removed_keys(&mut self, removed_keys: &[DomNodeSlotKey]) {
        self.clear_hover_for_removed_keys(removed_keys);
        self.clear_active_for_removed_keys(removed_keys);
        self.clear_focus_selection_for_removed_keys(removed_keys);
    }

    fn clear_hover_for_removed_keys(&mut self, removed_keys: &[DomNodeSlotKey]) {
        let previous_hovered_layers = self.take_hovered_layers();
        let mut next_hovered_layers = Vec::with_capacity(previous_hovered_layers.len());
        for hovered_layer in previous_hovered_layers {
            if hover_layer_intersects_removed_keys(&hovered_layer, removed_keys) {
                for key in hovered_layer.element_chain_root_to_target {
                    let _ = self.clear_pseudo_state_for_element(key, DomPseudoState::Hover);
                }
                continue;
            }
            next_hovered_layers.push(hovered_layer);
        }
        self.store_hovered_layers(next_hovered_layers);
    }

    fn clear_active_for_removed_keys(&mut self, removed_keys: &[DomNodeSlotKey]) {
        if self
            .pseudo_state
            .active_element_key
            .is_some_and(|key| removed_keys.contains(&key))
        {
            self.pseudo_state.active_element_key = None;
        }
    }

    fn clear_focus_selection_for_removed_keys(&mut self, removed_keys: &[DomNodeSlotKey]) {
        if self
            .pseudo_state
            .focused_element_key
            .is_some_and(|key| removed_keys.contains(&key))
        {
            self.pseudo_state.focused_element_key = None;
            self.selection = DomSelection::None;
        }
        if matches!(
            self.selection,
            DomSelection::TextSelected(selection) if removed_keys.contains(&selection.target_element)
        ) {
            self.selection = DomSelection::None;
        }
        if self
            .pressed_pointer_click_target
            .as_ref()
            .is_some_and(|target| removed_keys.contains(&target.target_key))
        {
            self.pressed_pointer_click_target = None;
        }
        if self
            .active_range_drag_session
            .as_ref()
            .is_some_and(|session| removed_keys.contains(&session.target_key))
        {
            self.active_range_drag_session = None;
        }
    }

    /// Removes one non-root node together with its full subtree from the
    /// root-owned slot store.
    pub fn remove_node(&mut self, key: DomNodeSlotKey) -> Result<(), DomError> {
        if key == self.root {
            return Err(DomError::CannotRemoveRoot);
        }
        if !self.nodes.contains_key(key) {
            return Err(DomError::InvalidNodeKey);
        }
        let removed_keys = collect_subtree_keys(&self.nodes, key)?;
        self.increment_metric(RootDomMetricName::RemovedSubtreeKeyListAllocation);
        if self
            .pseudo_state
            .focused_element_key
            .is_some_and(|focused_key| removed_keys.contains(&focused_key))
        {
            self.blur();
        }
        self.clear_pseudo_state_for_removed_keys(&removed_keys);
        for removed_key in &removed_keys {
            self.vertical_scroll_offset_by_node.remove(removed_key);
        }
        self.pending_layer_append_roots
            .retain(|key| !removed_keys.contains(key));
        self.pending_layer_remove_roots
            .retain(|key| !removed_keys.contains(key));

        let previous_parent_key = detach_node_from_parent(&mut self.nodes, key)?;

        if let Some(parent_key) = previous_parent_key {
            mark_dirty_up_to_layer_root(self, parent_key, DirtyFlag::Layout);
            mark_full_repaint_up_to_layout_root(self, parent_key);
        }
        for removed_key in &removed_keys {
            let Some(node) = self.nodes.get(*removed_key) else {
                continue;
            };
            let should_remove_layer = layer_kind_for_node(node) != crate::types::DomLayerKind::None;
            if should_remove_layer {
                self.queue_layer_remove_root(*removed_key);
            }
        }

        remove_collected_subtree_keys(
            &mut self.nodes,
            &removed_keys,
            &mut self.transitions.active_nodes,
            &mut self.style_pass_attrs_by_node,
            &mut self.id_lookup,
        );
        Ok(())
    }

    /// Sets one node attribute through the root-owned mutation path so the id
    /// lookup stays synchronized with element attribute changes.
    pub fn set_attr(
        &mut self,
        key: DomNodeSlotKey,
        attribute: DomAttribute,
        value: Option<DomAttributeValue>,
    ) -> Result<(), DomError> {
        self.set_attr_internal(key, attribute, value)
    }

    #[allow(clippy::too_many_lines)]
    fn set_attr_internal(
        &mut self,
        key: DomNodeSlotKey,
        attribute: DomAttribute,
        value: Option<DomAttributeValue>,
    ) -> Result<(), DomError> {
        let value = normalize_attribute_value(&attribute, value)?;
        if attribute == DomAttribute::Transition {
            let longhand_values = transition_longhand_values_from_shorthand(value.as_ref())?;
            self.set_transition_longhand_attrs(key, longhand_values)?;
        }
        let is_id_attribute = attribute == DomAttribute::Id;
        let previous_id = self.current_element_id(key);
        let next_id = next_id_from_attr(&attribute, value.as_ref())?;
        self.assert_id_is_available_for(key, next_id.as_deref())?;
        let attribute_for_refresh = attribute.clone();
        let current_node = self.nodes.get(key).ok_or(DomError::InvalidNodeKey)?;
        let previous_layer_kind = layer_kind_for_node(current_node);
        if current_node.attr(&attribute_for_refresh) == value.as_ref() {
            return Ok(());
        }
        let pending_transition = transition_for_attr_change(
            current_node,
            key,
            &attribute_for_refresh,
            value.as_ref(),
            self.clock_s,
        );
        let canceled_elapsed_s = current_node
            .active_transition(&attribute_for_refresh)
            .map(|transition| transition_elapsed_s_at(transition, self.clock_s));
        let had_active_transition = canceled_elapsed_s.is_some();
        let mut invalidation = attribute_invalidation(&attribute_for_refresh);
        let prepared_intrinsic_after_attr_write = current_node
            .before_attr_write_intrinsic_update(&attribute_for_refresh, value.as_ref())?;
        if prepared_intrinsic_after_attr_write.is_some() {
            invalidation = invalidation.with_layout();
        }
        let previous_layer_root = if invalidation.layer_membership() {
            current_layer_root_key(&self.nodes, key)
        } else {
            None
        };
        let intrinsic_after_attr_write_ran = prepared_intrinsic_after_attr_write.is_some();
        let scheduled_transition = pending_transition.is_some();
        let live_text_selection = match self.selection {
            DomSelection::TextSelected(selection) if selection.target_element == key => {
                Some(selection)
            }
            _ => None,
        };
        let node_has_active_transitions = {
            let node = self.nodes.get_mut(key).ok_or(DomError::InvalidNodeKey)?;
            if had_active_transition {
                node.clear_transition(&attribute_for_refresh);
            }
            node.set_attr(attribute, value)?;
            let _intrinsic_changed =
                node.after_attr_write_intrinsic_update(prepared_intrinsic_after_attr_write);
            if let Some(transition) = pending_transition {
                node.set_transition(transition);
            }
            node.active_transition_attributes().next().is_some()
        };
        let next_layer_kind = {
            let node = self.nodes.get(key).ok_or(DomError::InvalidNodeKey)?;
            layer_kind_for_node(node)
        };
        if scheduled_transition {
            self.increment_metric(RootDomMetricName::TransitionScheduleAllocation);
        }
        if had_active_transition {
            self.dispatch_and_record_transition_event(
                key,
                DomEventName::TransitionCancel,
                attribute_for_refresh.clone(),
                canceled_elapsed_s.expect("existing transition should carry elapsed time"),
            );
        }
        if node_has_active_transitions {
            self.transitions.active_nodes.insert(key);
        } else {
            self.transitions.active_nodes.remove(&key);
        }
        if intrinsic_after_attr_write_ran
            && matches!(
                self.node(key).and_then(DomNode::as_element),
                Some(element)
                    if element.behavior().intrinsic_kind == crate::types::DomElementIntrinsicKind::Img
            )
        {
            self.increment_metric(RootDomMetricName::ImgIntrinsicStateUpdate);
            if matches!(
                self.node(key)
                    .expect("intrinsic node should exist")
                    .intrinsic(),
                DomIntrinsic::Img(_)
            ) {
                self.increment_metric(RootDomMetricName::ImgIntrinsicRecomputation);
            }
        }

        if is_id_attribute {
            self.replace_indexed_id(key, previous_id, next_id);
        }
        self.sync_vertical_scroll_entry_for_attr_change(key, &attribute_for_refresh);
        if attribute_for_refresh == DomAttribute::Value {
            let (edit_kind, current_value) = {
                let element = self
                    .node(key)
                    .and_then(DomNode::as_element)
                    .expect("value attribute target should remain an element");
                let current_value = match element.attrs.attributes.get(&DomAttribute::Value) {
                    Some(DomAttributeValue::String(value)) => value.clone(),
                    None => String::new(),
                    Some(_) => panic!("editable element value attribute should stay string"),
                };
                (resolved_text_edit_kind(element), current_value)
            };
            if let Some(selection) = live_text_selection {
                match edit_kind {
                    DomElementEditKind::None => {
                        self.selection = DomSelection::None;
                    }
                    DomElementEditKind::SingleLine | DomElementEditKind::MultiLine => {
                        let value_len = count_unicode_scalars(&current_value);
                        let start = selection.selection_start.min(value_len);
                        let end = selection.selection_end.min(value_len);
                        self.selection = DomSelection::TextSelected(TextSelection {
                            target_element: key,
                            selection_start: start.min(end),
                            selection_end: start.max(end),
                            selection_direction: if start == end {
                                TextSelectionDirection::None
                            } else {
                                selection.selection_direction
                            },
                        });
                    }
                }
            }
        }

        if attribute_for_refresh == DomAttribute::Disabled
            && self.focused_element_key() == Some(key)
            && matches!(
                self.node(key)
                    .and_then(DomNode::as_element)
                    .and_then(|element| element.attrs.attributes.get(&DomAttribute::Disabled)),
                Some(DomAttributeValue::Bool(true))
            )
        {
            self.blur();
        }

        self.mark_dirty_for_invalidation(key, invalidation, previous_layer_root);
        if previous_layer_kind != next_layer_kind {
            if previous_layer_kind != crate::types::DomLayerKind::None {
                self.queue_layer_remove_root(key);
            }
            if next_layer_kind != crate::types::DomLayerKind::None {
                self.queue_layer_append_root(key);
            }
        }
        Ok(())
    }

    fn set_transition_longhand_attrs(
        &mut self,
        key: DomNodeSlotKey,
        values: TransitionLonghandValues,
    ) -> Result<(), DomError> {
        for (attribute, value) in transition_longhand_attr_values(values) {
            self.set_attr_internal(key, attribute, value)?;
        }
        Ok(())
    }

    fn sync_vertical_scroll_entry_for_attr_change(
        &mut self,
        key: DomNodeSlotKey,
        attribute: &DomAttribute,
    ) {
        if *attribute != DomAttribute::Overflow {
            return;
        }
        let Some(node) = self.node(key) else {
            return;
        };
        if node_allows_vertical_scroll_offset(node) {
            return;
        }
        self.vertical_scroll_offset_by_node.remove(&key);
    }

    /// Sets one node attribute from authored string name/value text.
    ///
    /// Use `"unset"` as the authored removal value.
    pub fn set_attr_str(
        &mut self,
        key: DomNodeSlotKey,
        attribute: &str,
        value: &str,
    ) -> Result<(), DomError> {
        let attribute = DomAttribute::try_from(attribute)?;
        let value = parse_authored_value_input(&attribute, value)?;
        self.set_attr(key, attribute, value)
    }

    pub fn set_text_content(
        &mut self,
        key: DomNodeSlotKey,
        text: impl AsRef<str>,
    ) -> Result<(), DomError> {
        let text = text.as_ref();
        let current_node = self.nodes.get(key).ok_or(DomError::InvalidNodeKey)?;
        let DomNode::Text(text_node) = current_node else {
            return Err(DomError::InvalidAttributeTarget);
        };
        if text_node.data() == text {
            return Ok(());
        }
        let inherited_context =
            crate::domains::visitor::inherited_style::inherited_style_context_from_ancestors(
                self, key,
            )?;
        let next_intrinsic =
            crate::domains::paint::measure_text_intrinsic_with_font_size_line_height_white_space_and_wrap_policy(
                text,
                inherited_context.effective_font_size_px(),
                inherited_context.effective_line_height_px(),
                inherited_context.effective_white_space(),
                inherited_context.effective_overflow_wrap(),
                inherited_context.effective_word_break(),
            );
        let next_text = Arc::<str>::from(text);
        {
            let node = self.nodes.get_mut(key).ok_or(DomError::InvalidNodeKey)?;
            let DomNode::Text(text_node) = node else {
                return Err(DomError::InvalidAttributeTarget);
            };
            text_node.set_data(next_text);
            text_node.set_text_run_id_hint(None);
            text_node.base.intrinsic = DomIntrinsic::Text(next_intrinsic);
        }
        self.increment_metric(RootDomMetricName::TextIntrinsicMeasurement);
        self.increment_metric(RootDomMetricName::TextStorageAllocation);
        self.mark_layout_dirty_for_attribute_change(key);
        self.mark_paint_dirty_for_attribute_change(key);
        Ok(())
    }

    pub fn compute_style_map(
        &self,
        node_key: DomNodeSlotKey,
        parent_attr_map: Option<&ComputedStyleMap>,
    ) -> Result<ComputedStyleMap, DomError> {
        let element = self
            .node(node_key)
            .and_then(DomNode::as_element)
            .ok_or(DomError::InvalidAttributeTarget)?;
        let matched_rules = matched_style_rules_for_element(self, node_key)?;
        let mut resolved = parent_attr_map.cloned().unwrap_or_default();
        apply_pseudo_state_style_declarations(element, &mut resolved);
        for rule in matched_rules {
            for (attribute, value) in &rule.attr_map {
                resolved.insert(attribute.clone(), value.clone());
            }
        }
        Ok(resolved)
    }

    pub fn apply_styles(
        &mut self,
        node_key: DomNodeSlotKey,
        map: &ComputedStyleMap,
    ) -> Result<(), DomError> {
        let removable_attributes =
            style_removed_attributes_for_node(self.style_pass_attrs_by_node.get(&node_key), map);
        if removable_attributes.capacity() > 0 {
            self.increment_metric(RootDomMetricName::StylesheetRemovedAttributeListAllocation);
        }
        for attribute in removable_attributes
            .iter()
            .filter(|attribute| **attribute != DomAttribute::Transition)
        {
            self.set_attr_internal(node_key, attribute.clone(), None)?;
        }
        if removable_attributes.contains(&DomAttribute::Transition) {
            self.set_attr_internal(node_key, DomAttribute::Transition, None)?;
        }
        for (attribute, value) in map {
            self.set_attr_internal(node_key, attribute.clone(), Some(value.clone()))?;
        }
        self.record_style_pass_attrs(node_key, map.keys().cloned());
        if !map.is_empty() {
            self.increment_metric(RootDomMetricName::StylesheetTrackedAttributeSetAllocation);
        }
        Ok(())
    }

    fn record_style_pass_attrs(
        &mut self,
        key: DomNodeSlotKey,
        attributes: impl IntoIterator<Item = DomAttribute>,
    ) {
        let attrs = self.style_pass_attrs_by_node.entry(key).or_default();
        attrs.clear();
        attrs.extend(attributes);
        if attrs.is_empty() {
            self.style_pass_attrs_by_node.remove(&key);
        }
    }

    /// Schedules one node-owned transition and keeps the node in the root
    /// scheduler until all of its active transitions have finished.
    pub fn set_transition(
        &mut self,
        key: DomNodeSlotKey,
        transition: DomTransition,
    ) -> Result<(), DomError> {
        let transition = validate_and_normalize_transition_payload(transition)?;
        let attribute = transition.attribute.clone();
        let canceled_elapsed_s = self
            .nodes
            .get(key)
            .ok_or(DomError::InvalidNodeKey)?
            .active_transition(&attribute)
            .map(|existing| transition_elapsed_s_at(existing, self.clock_s));
        let had_active_transition = canceled_elapsed_s.is_some();
        let node = self.nodes.get_mut(key).ok_or(DomError::InvalidNodeKey)?;
        node.set_transition(transition);
        self.increment_metric(RootDomMetricName::TransitionScheduleAllocation);
        self.transitions.active_nodes.insert(key);
        if had_active_transition {
            self.dispatch_and_record_transition_event(
                key,
                DomEventName::TransitionCancel,
                attribute,
                canceled_elapsed_s.expect("existing transition should carry elapsed time"),
            );
        }
        Ok(())
    }

    /// Advances all scheduled node transitions to the provided timestamp and
    /// marks their nodes dirty through the normal upward invalidation path.
    pub fn sync_transitions(&mut self, now_s: f64) {
        self.clock_s = now_s;
        self.sync_transitions_at_current_time();
    }

    fn sync_transitions_at_current_time(&mut self) {
        let active_nodes = std::mem::take(&mut self.transitions.active_nodes);
        for key in active_nodes {
            let Some(node) = self.nodes.get(key) else {
                continue;
            };
            let previous_layer_kind = layer_kind_for_node(node);
            let previous_layer_root = if node
                .active_transition_attributes()
                .any(attribute_changes_layer_membership)
            {
                current_layer_root_key(&self.nodes, key)
            } else {
                None
            };
            let Some(node) = self.nodes.get_mut(key) else {
                continue;
            };
            let result = node.sync_transitions(self.clock_s);
            let next_layer_kind = layer_kind_for_node(node);
            if result.invalidation.layout() {
                self.mark_layout_dirty_for_attribute_change(key);
            }
            if result.invalidation.paint() {
                self.mark_paint_dirty_for_attribute_change(key);
            }
            if result.invalidation.style() {
                self.mark_style_dirty_for_attribute_change(key);
            }
            self.mark_previous_layer_dirty_when_membership_changes(
                key,
                result.invalidation,
                previous_layer_root,
            );
            if previous_layer_kind != next_layer_kind {
                if previous_layer_kind != crate::types::DomLayerKind::None {
                    self.queue_layer_remove_root(key);
                }
                if next_layer_kind != crate::types::DomLayerKind::None {
                    self.queue_layer_append_root(key);
                }
            }
            for finished in result.finished_attributes {
                self.dispatch_and_record_transition_event(
                    key,
                    DomEventName::TransitionEnd,
                    finished.attribute,
                    finished.elapsed_s,
                );
            }
            if result.still_active {
                self.transitions.active_nodes.insert(key);
            }
        }
    }

    /// Accepts one already-constructed event from shared context rebroadcast,
    /// injects it into the root's local dispatch path, and queues ownership of
    /// the finished event for the next `update(...)` drain when it remains
    /// sink-eligible.
    pub fn dispatch_broadcast_event(&mut self, event: DomEvent) -> bool {
        let bubbles = event.bubbles();
        self.dispatch_event_to_target_key_and_record_if_bubbled(self.root, event)
            .expect("document root should always dispatch")
            && bubbles
    }

    pub(crate) fn dispatch_event_to_target_key(
        &mut self,
        target_key: DomNodeSlotKey,
        event: DomEvent,
    ) -> Result<Option<DomEvent>, DomError> {
        self.dispatch_event_to_target_key_and_finish(target_key, event)
            .map(crate::types::element_behaviors::DomEventDispatchOutcome::into_sink_eligible_event)
    }

    pub(crate) fn push_emitted_event(&mut self, event: DomEvent) {
        self.emitted_events.push(event);
        self.increment_metric(RootDomMetricName::EmittedEventAllocation);
    }

    pub(crate) fn dispatch_input_event_to_target_key(
        &mut self,
        target_key: DomNodeSlotKey,
        value: String,
    ) -> Result<bool, DomError> {
        self.dispatch_value_change_event_to_target_key(target_key, DomEventName::Input, value)
    }

    pub(crate) fn dispatch_change_event_to_target_key(
        &mut self,
        target_key: DomNodeSlotKey,
        value: String,
    ) -> Result<bool, DomError> {
        self.dispatch_value_change_event_to_target_key(target_key, DomEventName::Change, value)
    }

    pub(crate) fn apply_default_text_behavior_and_dispatch_input_from_keyboard_input(
        &mut self,
        input: &KeyboardInput,
        shift_key: bool,
    ) -> Result<bool, DomError> {
        if input.state != ButtonState::Pressed {
            return Ok(false);
        }
        let Some(target_key) = self.focused_element_key() else {
            return Ok(false);
        };
        let element = self
            .node(target_key)
            .and_then(DomNode::as_element)
            .ok_or(DomError::InvalidAttributeTarget)?;
        let edit_kind = resolved_text_edit_kind(element);
        if matches!(edit_kind, DomElementEditKind::None)
            || matches!(
                element.attrs.attributes.get(&DomAttribute::Disabled),
                Some(DomAttributeValue::Bool(true))
            )
        {
            return Ok(false);
        }
        let readonly = matches!(
            element.attrs.attributes.get(&DomAttribute::ReadOnly),
            Some(DomAttributeValue::Bool(true))
        );
        let selection = match self.selection {
            DomSelection::TextSelected(selection) if selection.target_element == target_key => {
                selection
            }
            _ => panic!("focused editable element should own live text selection"),
        };
        let value = match element.attrs.attributes.get(&DomAttribute::Value) {
            Some(DomAttributeValue::String(value)) => value.clone(),
            None => panic!("editable element value attribute should exist"),
            Some(_) => panic!("editable element value attribute should stay string"),
        };
        let Some(behavior) = default_text_behavior_result(
            &value,
            edit_kind,
            selection.selection_start,
            selection.selection_end,
            selection.selection_direction,
            input,
            shift_key,
        ) else {
            return Ok(false);
        };
        match behavior {
            DefaultTextBehaviorResult::SetSelection {
                selection_start,
                selection_end,
                selection_direction,
            } => {
                self.selection = DomSelection::TextSelected(TextSelection {
                    target_element: target_key,
                    selection_start,
                    selection_end,
                    selection_direction,
                });
                Ok(true)
            }
            DefaultTextBehaviorResult::ReplaceValue {
                next_value,
                next_caret_offset,
            } => {
                if readonly {
                    return Ok(false);
                }
                assert!(
                    next_caret_offset <= count_unicode_scalars(&next_value),
                    "default text behavior should keep the caret within the next value",
                );
                self.set_attr(
                    target_key,
                    DomAttribute::Value,
                    Some(DomAttributeValue::String(next_value.clone())),
                )?;
                self.selection = DomSelection::TextSelected(TextSelection {
                    target_element: target_key,
                    selection_start: next_caret_offset,
                    selection_end: next_caret_offset,
                    selection_direction: TextSelectionDirection::None,
                });
                let _ = self.dispatch_input_event_to_target_key(target_key, next_value)?;
                self.reopen_presented_options_for_text_input_value_change(target_key)?;
                Ok(true)
            }
        }
    }

    fn dispatch_value_change_event_to_target_key(
        &mut self,
        target_key: DomNodeSlotKey,
        name: DomEventName,
        value: String,
    ) -> Result<bool, DomError> {
        assert!(
            matches!(name, DomEventName::Input | DomEventName::Change),
            "value-change dispatch should use input or change",
        );
        let element = self
            .node(target_key)
            .and_then(DomNode::as_element)
            .ok_or(DomError::InvalidAttributeTarget)?;
        if element.tag == DomTag::Select {
            return self.dispatch_event_to_target_key_and_record_if_bubbled(
                target_key,
                DomEvent::new(
                    name,
                    DomEventDetail::ValueChange(DomValueChangeEvent { value }),
                ),
            );
        }
        let Some(control) = resolved_element_control(element) else {
            return Err(DomError::InvalidAttributeTarget);
        };
        if matches!(
            element.attrs.attributes.get(&DomAttribute::Disabled),
            Some(DomAttributeValue::Bool(true))
        ) {
            return Err(DomError::InvalidAttributeTarget);
        }
        let readonly = matches!(
            control,
            ResolvedElementControl::TextInput | ResolvedElementControl::Textarea
        ) && matches!(
            element.attrs.attributes.get(&DomAttribute::ReadOnly),
            Some(DomAttributeValue::Bool(true))
        ) || matches!(control, ResolvedElementControl::Number(control) if control.readonly);
        if readonly {
            return Err(DomError::InvalidAttributeTarget);
        }
        self.dispatch_event_to_target_key_and_record_if_bubbled(
            target_key,
            DomEvent::new(
                name,
                DomEventDetail::ValueChange(DomValueChangeEvent { value }),
            ),
        )
    }

    pub(crate) fn dispatch_event_to_target_key_and_record_if_bubbled(
        &mut self,
        target_key: DomNodeSlotKey,
        event: DomEvent,
    ) -> Result<bool, DomError> {
        let outcome = self.dispatch_event_to_target_key_and_finish(target_key, event)?;
        let Some(dispatched_event) = outcome.into_sink_eligible_bubbled_event() else {
            return Ok(false);
        };
        self.push_emitted_event(dispatched_event);
        Ok(true)
    }

    pub(crate) fn target_has_exact_listener(
        &self,
        target_key: DomNodeSlotKey,
        event_name: DomEventName,
    ) -> Result<bool, DomError> {
        let node = self.node(target_key).ok_or(DomError::InvalidNodeKey)?;
        Ok(node.has_listener_for_event_name(event_name))
    }

    fn dispatch_and_record_transition_event(
        &mut self,
        key: DomNodeSlotKey,
        name: DomEventName,
        attribute: DomAttribute,
        elapsed_s: f64,
    ) {
        self.increment_metric(RootDomMetricName::TransitionEventPathAllocation);
        let mut event = DomEvent::new(
            name,
            DomEventDetail::Transition(DomTransitionEvent {
                attribute,
                elapsed_s,
            }),
        );
        event.target_key = Some(key);
        let _ = self.dispatch_event_to_target_key_and_record_if_bubbled(key, event);
        if name == DomEventName::TransitionEnd {
            self.queue_presented_options_popup_shell_for_layer_removal_after_transition_end(key);
        }
    }

    fn queue_presented_options_popup_shell_for_layer_removal_after_transition_end(
        &mut self,
        shell_key: DomNodeSlotKey,
    ) {
        if self
            .node(shell_key)
            .and_then(DomNode::as_element)
            .and_then(|element| {
                element
                    .attrs
                    .attributes
                    .get(&DomAttribute::PresentedOptionsState)
            })
            != Some(&DomAttributeValue::String("closing".to_string()))
        {
            return;
        }
        push_unique_key(&mut self.pending_layer_remove_roots, shell_key);
    }

    fn dispatch_focus_transition_event(
        &mut self,
        key: DomNodeSlotKey,
        name: DomEventName,
        related_target_key: Option<DomNodeSlotKey>,
    ) -> Result<(), DomError> {
        assert!(
            matches!(
                name,
                DomEventName::Focus
                    | DomEventName::Blur
                    | DomEventName::FocusIn
                    | DomEventName::FocusOut
            ),
            "focus transition dispatch should use one focus-family event name"
        );
        let event = DomEvent::new(
            name,
            DomEventDetail::Focus(DomFocusEvent { related_target_key }),
        );
        let _ = self.dispatch_event_to_target_key_and_record_if_bubbled(key, event)?;
        Ok(())
    }

    fn assert_id_is_available(&self, id: Option<&str>) -> Result<(), DomError> {
        let Some(id) = id else {
            return Ok(());
        };
        if self.id_lookup.contains_key(id) {
            return Err(DomError::DuplicateElementId);
        }
        Ok(())
    }

    fn index_node_id_if_present(&mut self, key: DomNodeSlotKey) {
        let Some(id) = self
            .nodes
            .get(key)
            .and_then(DomNode::as_element)
            .and_then(|element| element.attrs.id.clone())
        else {
            return;
        };
        self.id_lookup.insert(id, key);
    }

    fn assert_id_is_available_for(
        &self,
        key: DomNodeSlotKey,
        id: Option<&str>,
    ) -> Result<(), DomError> {
        let Some(id) = id else {
            return Ok(());
        };
        match self.id_lookup.get(id) {
            Some(existing_key) if *existing_key != key => Err(DomError::DuplicateElementId),
            _ => Ok(()),
        }
    }

    fn current_element_id(&self, key: DomNodeSlotKey) -> Option<String> {
        self.nodes
            .get(key)
            .and_then(DomNode::as_element)
            .and_then(|element| element.attrs.id.clone())
    }

    fn replace_indexed_id(
        &mut self,
        key: DomNodeSlotKey,
        previous_id: Option<String>,
        next_id: Option<String>,
    ) {
        if let Some(previous_id) = previous_id {
            self.id_lookup.remove(&previous_id);
        }
        if let Some(next_id) = next_id {
            self.id_lookup.insert(next_id, key);
        }
    }

    fn mark_dirty_for_invalidation(
        &mut self,
        key: DomNodeSlotKey,
        invalidation: AttributeInvalidation,
        previous_layer_root: Option<DomNodeSlotKey>,
    ) {
        if invalidation.layout() {
            self.mark_layout_dirty_for_attribute_change(key);
        }
        if invalidation.paint() {
            self.mark_paint_dirty_for_attribute_change(key);
        }
        if invalidation.style() {
            self.mark_style_dirty_for_attribute_change(key);
        }
        self.mark_previous_layer_dirty_when_membership_changes(
            key,
            invalidation,
            previous_layer_root,
        );
    }

    pub(crate) fn mark_layout_dirty_for_attribute_change(&mut self, key: DomNodeSlotKey) {
        mark_dirty_up_to_layer_root(self, key, DirtyFlag::Layout);
    }

    pub(crate) fn mark_paint_dirty_for_attribute_change(&mut self, key: DomNodeSlotKey) {
        mark_node_dirty_up_to_layout_root(self, key);
    }

    pub(crate) fn mark_style_dirty_for_attribute_change(&mut self, key: DomNodeSlotKey) {
        mark_dirty_up_to_layer_root(self, key, DirtyFlag::Style);
    }

    fn mark_previous_layer_dirty_when_membership_changes(
        &mut self,
        key: DomNodeSlotKey,
        invalidation: AttributeInvalidation,
        previous_layer_root: Option<DomNodeSlotKey>,
    ) {
        let Some(previous_layer_root) = previous_layer_root else {
            return;
        };
        let Some(current_layer_root) = current_layer_root_key(&self.nodes, key) else {
            return;
        };
        if current_layer_root == previous_layer_root {
            return;
        }
        mark_previous_layer_remainder_dirty(
            self,
            key,
            current_layer_root,
            previous_layer_root,
            invalidation,
        );
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ActivationPreDispatchState {
    Checkbox { checked_before: bool },
}

impl RootDom {
    pub(crate) fn point_event_target_key_from_attached_point(
        &self,
        world: &World,
        parent: Entity,
        point: PointPx,
    ) -> Result<Option<DomNodeSlotKey>, DomError> {
        topmost_event_target_from_point(self, world, parent, point)
    }

    pub(crate) fn dispatch_event_to_target_key_and_finish(
        &mut self,
        target_key: DomNodeSlotKey,
        event: DomEvent,
    ) -> Result<DomEventDispatchOutcome, DomError> {
        if !event.bubbles() && !self.target_has_exact_listener(target_key, event.name)? {
            return Ok(DomEventDispatchOutcome::NotDispatched);
        }
        Ok(DomEventDispatchOutcome::Dispatched(
            dispatch_event_to_target_and_finish(self, target_key, event)?,
        ))
    }

    pub(crate) fn apply_default_behavior_from_keyboard_input(
        &mut self,
        input: &KeyboardInput,
        shift_key: bool,
        event_stayed_unblocked: bool,
    ) -> Result<bool, DomError> {
        let Some(target_key) = self.focused_element_key() else {
            return Ok(false);
        };
        let Some(element) = self.node(target_key).and_then(DomNode::as_element) else {
            return Ok(false);
        };
        match element.behavior().on_keyboard {
            DomElementOnKeyboard::None => Ok(false),
            DomElementOnKeyboard::Button => {
                self.apply_button_on_keyboard(target_key, input, event_stayed_unblocked)
            }
            DomElementOnKeyboard::InputControl => self.apply_input_control_on_keyboard(
                target_key,
                input,
                shift_key,
                event_stayed_unblocked,
            ),
            DomElementOnKeyboard::Select => self.apply_select_keyboard_default_for_owner(
                target_key,
                input,
                event_stayed_unblocked,
            ),
            DomElementOnKeyboard::TextEdit => self.apply_text_edit_on_keyboard(
                target_key,
                input,
                shift_key,
                event_stayed_unblocked,
            ),
            DomElementOnKeyboard::PresentedOptions => self
                .apply_presented_options_text_input_keyboard_navigation_for_owner(
                    target_key,
                    input,
                    event_stayed_unblocked,
                ),
        }
    }

    pub(crate) fn apply_after_focus(&mut self, target_key: DomNodeSlotKey) -> Result<(), DomError> {
        let Some(element) = self.node(target_key).and_then(DomNode::as_element) else {
            return Ok(());
        };
        match element.behavior().after_focus {
            DomElementAfterFocus::None => {}
            DomElementAfterFocus::PresentedOptions => {
                let _ = self.open_presented_options_for_focused_text_input_owner(target_key)?;
            }
        }
        Ok(())
    }

    pub(crate) fn apply_on_blur(&mut self, target_key: DomNodeSlotKey) -> Result<(), DomError> {
        let Some(element) = self.node(target_key).and_then(DomNode::as_element) else {
            return Ok(());
        };
        match element.behavior().on_blur {
            DomElementOnBlur::None => {}
            DomElementOnBlur::Select => {
                let _ = self.close_select_popup_for_blurred_owner(target_key)?;
            }
            DomElementOnBlur::PresentedOptions => {
                let _ = self.close_presented_options_for_blurred_owner(target_key)?;
            }
        }
        Ok(())
    }

    pub(crate) fn apply_pointer_down_default_lifecycles(
        &mut self,
        point: PointPx,
        raw_target_key: Option<DomNodeSlotKey>,
        default_target: Option<DomPointEventTarget>,
    ) -> Result<(), DomError> {
        self.pressed_pointer_click_target =
            self.pressed_pointer_click_target_from_raw_target(raw_target_key);
        let Some(target) = default_target else {
            return Ok(());
        };
        self.apply_on_pointer_down(target, point)
    }

    pub(crate) fn apply_pointer_up_default_lifecycles(
        &mut self,
        point: PointPx,
        raw_target_key: Option<DomNodeSlotKey>,
    ) -> Result<(), DomError> {
        self.finish_range_drag_change_if_dirty()?;
        let pressed_click_target = self.pressed_pointer_click_target.take();
        let Some(pressed_click_target) = pressed_click_target else {
            return Ok(());
        };
        if raw_target_key != Some(pressed_click_target.target_key) {
            return Ok(());
        }
        let _ =
            self.dispatch_click_and_apply_after_click(pressed_click_target.target_key, point)?;
        Ok(())
    }

    pub(crate) fn apply_default_range_drag_for_point(
        &mut self,
        point: PointPx,
    ) -> Result<(), DomError> {
        let Some(session) = self.active_range_drag_session.clone() else {
            return Ok(());
        };
        let Some(ResolvedElementControl::Range(control)) =
            self.resolved_element_control_for_key(session.target_key)
        else {
            self.active_range_drag_session = None;
            return Ok(());
        };
        if control.disabled {
            self.active_range_drag_session = None;
            return Ok(());
        }
        let subtree_point = range_drag_subtree_point(&session, point);
        let next_value =
            self.range_value_string_for_point(session.target_key, subtree_point, control)?;
        let changed = self.set_control_value_and_dispatch_input(session.target_key, next_value)?;
        self.mark_active_range_drag_dirty(session.target_key, changed);
        Ok(())
    }

    fn apply_button_on_keyboard(
        &mut self,
        target_key: DomNodeSlotKey,
        input: &KeyboardInput,
        event_stayed_unblocked: bool,
    ) -> Result<bool, DomError> {
        if input.state == ButtonState::Released {
            if !event_stayed_unblocked {
                return Ok(false);
            }
            return self.apply_button_keyboard_click_on_keyup(target_key, input);
        }
        if input.state != ButtonState::Pressed || !event_stayed_unblocked {
            return Ok(false);
        }
        self.apply_button_keyboard_click_on_keydown(target_key, input)
    }

    fn apply_input_control_on_keyboard(
        &mut self,
        target_key: DomNodeSlotKey,
        input: &KeyboardInput,
        shift_key: bool,
        event_stayed_unblocked: bool,
    ) -> Result<bool, DomError> {
        if input.state == ButtonState::Released {
            if !event_stayed_unblocked {
                return Ok(false);
            }
            return self.apply_input_keyboard_click_on_keyup(target_key, input);
        }
        if input.state != ButtonState::Pressed || !event_stayed_unblocked {
            return Ok(false);
        }
        if let Some(applied) = self.apply_input_keyboard_step_on_keydown(target_key, input)? {
            return Ok(applied);
        }
        if self.apply_presented_options_text_input_keyboard_navigation_for_owner(
            target_key,
            input,
            event_stayed_unblocked,
        )? {
            return Ok(true);
        }
        self.ensure_text_selection_for_target(target_key);
        self.apply_default_text_behavior_and_dispatch_input_from_keyboard_input(input, shift_key)
    }

    fn apply_text_edit_on_keyboard(
        &mut self,
        target_key: DomNodeSlotKey,
        input: &KeyboardInput,
        shift_key: bool,
        event_stayed_unblocked: bool,
    ) -> Result<bool, DomError> {
        if input.state != ButtonState::Pressed || !event_stayed_unblocked {
            return Ok(false);
        }
        if self.apply_presented_options_text_input_keyboard_navigation_for_owner(
            target_key,
            input,
            event_stayed_unblocked,
        )? {
            return Ok(true);
        }
        self.ensure_text_selection_for_target(target_key);
        self.apply_default_text_behavior_and_dispatch_input_from_keyboard_input(input, shift_key)
    }

    fn apply_on_pointer_down(
        &mut self,
        target: DomPointEventTarget,
        root_point: PointPx,
    ) -> Result<(), DomError> {
        let target_key = target.target_key;
        let Some(element) = self.node(target_key).and_then(DomNode::as_element) else {
            return Ok(());
        };
        match element.behavior().on_pointer {
            DomElementOnPointer::None => Ok(()),
            DomElementOnPointer::TextEdit => {
                self.ensure_text_selection_for_target(target_key);
                Ok(())
            }
            DomElementOnPointer::InputControl => {
                let Some(control) = self.resolved_element_control_for_key(target_key) else {
                    return Ok(());
                };
                if matches!(control, ResolvedElementControl::TextInput) {
                    self.ensure_text_selection_for_target(target_key);
                    return Ok(());
                }
                let Some(control_part) =
                    self.resolved_control_part_for_key(target_key, target.subtree_point)?
                else {
                    return Ok(());
                };
                match (control, control_part) {
                    (
                        ResolvedElementControl::Number(control),
                        crate::element_controls::ResolvedElementControlPart::NumberDecrementButton,
                    ) if !control.disabled && !control.readonly => self
                        .set_control_value_and_dispatch_input_change(
                            target_key,
                            control
                                .numeric
                                .stepped_value_string(NumericStepDirection::Decrement),
                        )
                        .map(|_| ()),
                    (
                        ResolvedElementControl::Number(control),
                        crate::element_controls::ResolvedElementControlPart::NumberIncrementButton,
                    ) if !control.disabled && !control.readonly => self
                        .set_control_value_and_dispatch_input_change(
                            target_key,
                            control
                                .numeric
                                .stepped_value_string(NumericStepDirection::Increment),
                        )
                        .map(|_| ()),
                    (ResolvedElementControl::Range(control), _) if !control.disabled => {
                        self.start_range_drag_session(
                            target_key,
                            root_point,
                            target.subtree_point,
                        )?;
                        let next_value = self.range_value_string_for_point(
                            target_key,
                            target.subtree_point,
                            control,
                        )?;
                        let changed =
                            self.set_control_value_and_dispatch_input(target_key, next_value)?;
                        self.mark_active_range_drag_dirty(target_key, changed);
                        Ok(())
                    }
                    _ => Ok(()),
                }
            }
        }
    }

    pub(crate) fn ensure_text_selection_for_target(&mut self, target_key: DomNodeSlotKey) {
        let Some(element) = self.node(target_key).and_then(DomNode::as_element) else {
            self.set_selection(DomSelection::None);
            return;
        };
        let current_value = match element.attrs.attributes.get(&DomAttribute::Value) {
            Some(DomAttributeValue::String(value)) => value.clone(),
            None => String::new(),
            Some(_) => panic!("editable element value attribute should stay string"),
        };
        match resolved_text_edit_kind(element) {
            DomElementEditKind::None => {
                self.set_selection(DomSelection::None);
            }
            DomElementEditKind::SingleLine | DomElementEditKind::MultiLine => {
                if matches!(
                    self.selection(),
                    DomSelection::TextSelected(selection) if selection.target_element == target_key
                ) {
                    return;
                }
                let value_len = count_unicode_scalars(&current_value);
                self.set_selection(DomSelection::TextSelected(TextSelection {
                    target_element: target_key,
                    selection_start: value_len,
                    selection_end: value_len,
                    selection_direction: TextSelectionDirection::None,
                }));
            }
        }
    }

    fn apply_input_keyboard_step_on_keydown(
        &mut self,
        target_key: DomNodeSlotKey,
        input: &KeyboardInput,
    ) -> Result<Option<bool>, DomError> {
        let Some(control) = self.resolved_element_control_for_key(target_key) else {
            return Ok(Some(false));
        };
        match control {
            ResolvedElementControl::Checkbox(_) | ResolvedElementControl::Textarea => {
                Ok(Some(false))
            }
            ResolvedElementControl::Number(control) => {
                let Some(next_value) =
                    numeric_keyboard_value_string(control.numeric, &input.logical_key)
                else {
                    return Ok(None);
                };
                if control.disabled || control.readonly {
                    return Ok(Some(false));
                }
                self.set_control_value_and_dispatch_input_change(target_key, next_value)
                    .map(Some)
            }
            ResolvedElementControl::Range(control) => {
                let Some(next_value) =
                    numeric_keyboard_value_string(control.numeric, &input.logical_key)
                else {
                    return Ok(Some(false));
                };
                if control.disabled {
                    return Ok(Some(false));
                }
                self.set_control_value_and_dispatch_input_change(target_key, next_value)
                    .map(Some)
            }
            ResolvedElementControl::TextInput => Ok(None),
        }
    }

    fn apply_button_keyboard_click_on_keydown(
        &mut self,
        target_key: DomNodeSlotKey,
        input: &KeyboardInput,
    ) -> Result<bool, DomError> {
        if input.repeat
            || !matches!(input.logical_key, Key::Enter)
            || self.target_has_disabled_attr(target_key)?
        {
            return Ok(false);
        }
        let point = self.keyboard_activation_click_point_for_target(target_key)?;
        self.dispatch_click_and_apply_after_click(target_key, point)
    }

    fn apply_button_keyboard_click_on_keyup(
        &mut self,
        target_key: DomNodeSlotKey,
        input: &KeyboardInput,
    ) -> Result<bool, DomError> {
        if !matches!(input.logical_key, Key::Space) || self.target_has_disabled_attr(target_key)? {
            return Ok(false);
        }
        let point = self.keyboard_activation_click_point_for_target(target_key)?;
        self.dispatch_click_and_apply_after_click(target_key, point)
    }

    fn apply_input_keyboard_click_on_keyup(
        &mut self,
        target_key: DomNodeSlotKey,
        input: &KeyboardInput,
    ) -> Result<bool, DomError> {
        if !matches!(input.logical_key, Key::Space) {
            return Ok(false);
        }
        let Some(ResolvedElementControl::Checkbox(control)) =
            self.resolved_element_control_for_key(target_key)
        else {
            return Ok(false);
        };
        if control.disabled {
            return Ok(false);
        }
        let point = self.keyboard_activation_click_point_for_target(target_key)?;
        self.dispatch_click_and_apply_after_click(target_key, point)
    }

    fn pressed_pointer_click_target_from_raw_target(
        &self,
        target_key: Option<DomNodeSlotKey>,
    ) -> Option<PressedPointerClickTarget> {
        let target_key = target_key?;
        let _element = self.node(target_key).and_then(DomNode::as_element)?;
        if self.target_has_disabled_attr(target_key).ok()? {
            return None;
        }
        Some(PressedPointerClickTarget { target_key })
    }

    fn dispatch_click_and_apply_after_click(
        &mut self,
        target_key: DomNodeSlotKey,
        point: PointPx,
    ) -> Result<bool, DomError> {
        let pre_activation_state = self.apply_after_click_before_dispatch(target_key)?;
        let outcome = self.dispatch_click_event_to_target_key(target_key, point)?;
        let default_prevented = outcome.default_prevented();
        if let Some(event) = outcome.into_sink_eligible_bubbled_event() {
            self.push_emitted_event(event);
        }
        if default_prevented {
            self.revert_after_click_after_canceled_dispatch(target_key, pre_activation_state)?;
            return Ok(false);
        }
        self.commit_after_click_after_dispatch(target_key)
    }

    fn dispatch_click_event_to_target_key(
        &mut self,
        target_key: DomNodeSlotKey,
        point: PointPx,
    ) -> Result<DomEventDispatchOutcome, DomError> {
        let outcome = self.dispatch_event_to_target_key_and_finish(
            target_key,
            DomEvent::new(
                DomEventName::Click,
                DomEventDetail::Mouse(DomMouseEvent {
                    button: Some(crate::types::dom_event::DomMouseButton::Primary),
                    buttons: 0,
                    client_x: point.x_px,
                    client_y: point.y_px,
                    modifiers: crate::types::dom_event::DomModifierState::default(),
                }),
            ),
        )?;
        if matches!(outcome, DomEventDispatchOutcome::Dispatched(_)) {
            self.increment_metric(RootDomMetricName::ClickDispatched);
        }
        Ok(outcome)
    }

    fn apply_after_click_before_dispatch(
        &mut self,
        target_key: DomNodeSlotKey,
    ) -> Result<Option<ActivationPreDispatchState>, DomError> {
        let Some(element) = self.node(target_key).and_then(DomNode::as_element) else {
            return Ok(None);
        };
        match Self::resolved_on_activation_for_element(element) {
            DomElementOnActivation::None
            | DomElementOnActivation::Select
            | DomElementOnActivation::PresentedOptions => Ok(None),
            DomElementOnActivation::Toggle => {
                let Some(ResolvedElementControl::Checkbox(control)) =
                    resolved_element_control(element)
                else {
                    return Ok(None);
                };
                if control.disabled {
                    return Ok(None);
                }
                self.set_attr(
                    target_key,
                    DomAttribute::Checked,
                    Some(DomAttributeValue::Bool(!control.checked)),
                )?;
                self.increment_metric(RootDomMetricName::ActivationPreApplied);
                Ok(Some(ActivationPreDispatchState::Checkbox {
                    checked_before: control.checked,
                }))
            }
        }
    }

    fn revert_after_click_after_canceled_dispatch(
        &mut self,
        target_key: DomNodeSlotKey,
        pre_activation_state: Option<ActivationPreDispatchState>,
    ) -> Result<(), DomError> {
        let Some(pre_activation_state) = pre_activation_state else {
            return Ok(());
        };
        let Some(element) = self.node(target_key).and_then(DomNode::as_element) else {
            return Ok(());
        };
        if Self::resolved_on_activation_for_element(element) != DomElementOnActivation::Toggle {
            return Ok(());
        }
        match pre_activation_state {
            ActivationPreDispatchState::Checkbox { checked_before } => {
                self.set_attr(
                    target_key,
                    DomAttribute::Checked,
                    Some(DomAttributeValue::Bool(checked_before)),
                )?;
                self.increment_metric(RootDomMetricName::ActivationCanceled);
            }
        }
        Ok(())
    }

    fn commit_after_click_after_dispatch(
        &mut self,
        target_key: DomNodeSlotKey,
    ) -> Result<bool, DomError> {
        if self.apply_select_popup_row_click_default_for_target(target_key)? {
            return Ok(true);
        }
        let Some(element) = self.node(target_key).and_then(DomNode::as_element) else {
            return Ok(false);
        };
        match Self::resolved_on_activation_for_element(element) {
            DomElementOnActivation::None => Ok(true),
            DomElementOnActivation::Select => {
                self.apply_select_click_default_for_target(target_key)
            }
            DomElementOnActivation::Toggle => {
                let Some(ResolvedElementControl::Checkbox(control)) =
                    resolved_element_control(element)
                else {
                    return Ok(true);
                };
                if control.disabled {
                    return Ok(false);
                }
                let Some(current_value) = self.control_value_event_string(target_key)? else {
                    return Ok(false);
                };
                let _ =
                    self.dispatch_input_event_to_target_key(target_key, current_value.clone())?;
                let _ = self.dispatch_change_event_to_target_key(target_key, current_value)?;
                self.increment_metric(RootDomMetricName::ActivationCommitted);
                Ok(true)
            }
            DomElementOnActivation::PresentedOptions => {
                self.apply_presented_options_click_default_for_target(target_key)
            }
        }
    }

    fn resolved_on_activation_for_element(
        element: &crate::types::DomElementNode,
    ) -> DomElementOnActivation {
        if element.behavior().after_click != DomElementAfterClick::Activate {
            return DomElementOnActivation::None;
        }
        match element.behavior().on_activation {
            DomElementOnActivation::None => DomElementOnActivation::None,
            DomElementOnActivation::Select => DomElementOnActivation::Select,
            DomElementOnActivation::PresentedOptions => DomElementOnActivation::PresentedOptions,
            DomElementOnActivation::Toggle => {
                if matches!(
                    resolved_element_control(element),
                    Some(ResolvedElementControl::Checkbox(_))
                ) {
                    DomElementOnActivation::Toggle
                } else {
                    DomElementOnActivation::None
                }
            }
        }
    }

    fn target_has_disabled_attr(&self, target_key: DomNodeSlotKey) -> Result<bool, DomError> {
        let Some(element) = self.node(target_key).and_then(DomNode::as_element) else {
            return Err(DomError::InvalidAttributeTarget);
        };
        Ok(matches!(
            element.attrs.attributes.get(&DomAttribute::Disabled),
            Some(DomAttributeValue::Bool(true))
        ))
    }

    fn keyboard_activation_click_point_for_target(
        &self,
        target_key: DomNodeSlotKey,
    ) -> Result<PointPx, DomError> {
        let node = self.node(target_key).ok_or(DomError::InvalidNodeKey)?;
        let border_box = node_box_from_ref(node).border_box;
        Ok(PointPx {
            x_px: border_box.left_px + border_box.width_px * 0.5,
            y_px: border_box.top_px + border_box.height_px * 0.5,
        })
    }

    fn resolved_element_control_for_key(
        &self,
        key: DomNodeSlotKey,
    ) -> Option<ResolvedElementControl> {
        let element = self.node(key)?.as_element()?;
        resolved_element_control(element)
    }

    fn resolved_control_part_for_key(
        &self,
        key: DomNodeSlotKey,
        point: PointPx,
    ) -> Result<Option<crate::element_controls::ResolvedElementControlPart>, DomError> {
        let Some(element) = self.node(key).and_then(DomNode::as_element) else {
            return Ok(None);
        };
        Ok(resolved_control_part_from_point(
            element,
            node_box_from_ref(self.node(key).ok_or(DomError::InvalidNodeKey)?),
            point,
        ))
    }

    fn range_value_string_for_point(
        &self,
        key: DomNodeSlotKey,
        point: PointPx,
        control: crate::element_controls::ResolvedRangeControl,
    ) -> Result<String, DomError> {
        Ok(range_value_string_from_point(
            node_box_from_ref(self.node(key).ok_or(DomError::InvalidNodeKey)?),
            control,
            point,
        ))
    }

    fn start_range_drag_session(
        &mut self,
        target_key: DomNodeSlotKey,
        root_point: PointPx,
        subtree_point: PointPx,
    ) -> Result<(), DomError> {
        let focus_entry_value = self
            .control_value_event_string(target_key)?
            .unwrap_or_else(|| String::from("0"));
        self.active_range_drag_session = Some(ActiveRangeDragSession {
            target_key,
            root_to_subtree_point_offset: PointPx {
                x_px: subtree_point.x_px - root_point.x_px,
                y_px: subtree_point.y_px - root_point.y_px,
            },
            focus_entry_value,
            dirty: false,
        });
        Ok(())
    }

    fn mark_active_range_drag_dirty(&mut self, target_key: DomNodeSlotKey, changed: bool) {
        if !changed {
            return;
        }
        if let Some(session) = self.active_range_drag_session.as_mut()
            && session.target_key == target_key
        {
            session.dirty = true;
        }
    }

    fn finish_range_drag_change_if_dirty(&mut self) -> Result<(), DomError> {
        let Some(session) = self.active_range_drag_session.take() else {
            return Ok(());
        };
        if !session.dirty {
            return Ok(());
        }
        let Some(current_value) = self.control_value_event_string(session.target_key)? else {
            return Ok(());
        };
        if current_value != session.focus_entry_value {
            let _ = self.dispatch_change_event_to_target_key(session.target_key, current_value)?;
        }
        Ok(())
    }

    fn set_control_value_and_dispatch_input_change(
        &mut self,
        target_key: DomNodeSlotKey,
        next_value: String,
    ) -> Result<bool, DomError> {
        let changed = self.set_control_value_and_dispatch_input(target_key, next_value.clone())?;
        if !changed {
            return Ok(false);
        }
        let _ = self.dispatch_change_event_to_target_key(target_key, next_value)?;
        Ok(true)
    }

    fn set_control_value_and_dispatch_input(
        &mut self,
        target_key: DomNodeSlotKey,
        next_value: String,
    ) -> Result<bool, DomError> {
        let current_value = self
            .control_value_event_string(target_key)?
            .unwrap_or_default();
        if current_value == next_value {
            return Ok(false);
        }
        self.set_attr(
            target_key,
            DomAttribute::Value,
            Some(DomAttributeValue::String(next_value.clone())),
        )?;
        if self.focused_element_key() == Some(target_key)
            && matches!(
                self.node(target_key)
                    .and_then(DomNode::as_element)
                    .map(resolved_text_edit_kind),
                Some(DomElementEditKind::SingleLine | DomElementEditKind::MultiLine)
            )
        {
            let value_len = count_unicode_scalars(&next_value);
            self.set_selection(DomSelection::TextSelected(TextSelection {
                target_element: target_key,
                selection_start: value_len,
                selection_end: value_len,
                selection_direction: TextSelectionDirection::None,
            }));
        }
        let _ = self.dispatch_input_event_to_target_key(target_key, next_value)?;
        Ok(true)
    }

    fn control_value_event_string(
        &self,
        target_key: DomNodeSlotKey,
    ) -> Result<Option<String>, DomError> {
        let Some(element) = self.node(target_key).and_then(DomNode::as_element) else {
            return Err(DomError::InvalidAttributeTarget);
        };
        let value = match resolved_element_control(element) {
            Some(ResolvedElementControl::Checkbox(control)) => Some(control.checked.to_string()),
            Some(ResolvedElementControl::Number(control)) => {
                Some(format!("{}", control.numeric.display_value_px()))
            }
            Some(ResolvedElementControl::Range(control)) => {
                Some(format!("{}", control.numeric.display_value_px()))
            }
            Some(ResolvedElementControl::TextInput | ResolvedElementControl::Textarea) => {
                match element.attrs.attributes.get(&DomAttribute::Value) {
                    Some(DomAttributeValue::String(value)) => Some(value.clone()),
                    None => Some(String::new()),
                    Some(_) => return Err(DomError::InvalidAttributeValue),
                }
            }
            None => None,
        };
        Ok(value)
    }
}

impl RootDom {
    pub(crate) fn apply_select_keyboard_default_for_owner(
        &mut self,
        owner_key: DomNodeSlotKey,
        input: &KeyboardInput,
        event_stayed_unblocked: bool,
    ) -> Result<bool, DomError> {
        if input.state != ButtonState::Pressed || !event_stayed_unblocked {
            return Ok(false);
        }
        if self.resolve_presented_options_owner_kind(owner_key)
            != Some(PresentedOptionsOwnerKind::Select)
        {
            return Ok(false);
        }
        match input.logical_key {
            Key::ArrowDown => self.move_presented_options_highlight_or_open_popup(owner_key, true),
            Key::ArrowUp => self.move_presented_options_highlight_or_open_popup(owner_key, false),
            Key::Enter | Key::Space => {
                self.commit_highlighted_presented_option_or_open_popup(owner_key)
            }
            Key::Escape => self.close_presented_options_popup_for_owner(owner_key, false),
            _ => Ok(false),
        }
    }

    pub(crate) fn apply_presented_options_text_input_keyboard_navigation_for_owner(
        &mut self,
        owner_key: DomNodeSlotKey,
        input: &KeyboardInput,
        event_stayed_unblocked: bool,
    ) -> Result<bool, DomError> {
        if input.state != ButtonState::Pressed || !event_stayed_unblocked {
            return Ok(false);
        }
        if self.resolve_presented_options_owner_kind(owner_key)
            != Some(PresentedOptionsOwnerKind::TextInput)
        {
            return Ok(false);
        }
        match input.logical_key {
            Key::ArrowDown => self.move_presented_options_highlight_or_open_popup(owner_key, true),
            Key::ArrowUp => self.move_presented_options_highlight_or_open_popup(owner_key, false),
            Key::Enter => self.commit_highlighted_presented_option_for_owner(owner_key),
            Key::Escape => self.close_presented_options_popup_for_owner(owner_key, false),
            _ => Ok(false),
        }
    }

    pub(crate) fn apply_presented_options_click_default_for_target(
        &mut self,
        target_key: DomNodeSlotKey,
    ) -> Result<bool, DomError> {
        if self.commit_presented_option_hit_row(target_key)? {
            return Ok(true);
        }
        let Some(owner_key) = self.resolve_presented_options_owner_key_from_target(target_key)
        else {
            return Ok(false);
        };
        if self.presented_options_popup_shell_key(owner_key).is_some() {
            return self.close_presented_options_popup_for_owner(owner_key, false);
        }
        self.open_presented_options_popup_for_owner(owner_key)
    }

    pub(crate) fn apply_select_popup_row_click_default_for_target(
        &mut self,
        target_key: DomNodeSlotKey,
    ) -> Result<bool, DomError> {
        if self
            .resolve_presented_options_row_key_from_target(target_key)
            .is_none()
        {
            return Ok(false);
        }
        self.commit_select_presented_option_hit_row(target_key)
    }

    pub(crate) fn apply_select_click_default_for_target(
        &mut self,
        target_key: DomNodeSlotKey,
    ) -> Result<bool, DomError> {
        if self.commit_select_presented_option_hit_row(target_key)? {
            return Ok(true);
        }
        let Some(owner_key) = self.resolve_select_owner_key_from_target(target_key) else {
            return Ok(false);
        };
        if self.presented_options_popup_shell_key(owner_key).is_some() {
            return self.close_presented_options_popup_for_owner(owner_key, false);
        }
        self.open_select_popup_for_owner(owner_key)
    }

    pub(crate) fn close_select_popup_for_blurred_owner(
        &mut self,
        owner_key: DomNodeSlotKey,
    ) -> Result<bool, DomError> {
        if self.resolve_presented_options_owner_kind(owner_key)
            != Some(PresentedOptionsOwnerKind::Select)
        {
            return Ok(false);
        }
        self.close_presented_options_popup_for_owner(owner_key, false)
    }

    pub(crate) fn open_presented_options_for_focused_text_input_owner(
        &mut self,
        owner_key: DomNodeSlotKey,
    ) -> Result<bool, DomError> {
        if self.resolve_presented_options_owner_kind(owner_key)
            != Some(PresentedOptionsOwnerKind::TextInput)
        {
            return Ok(false);
        }
        self.open_presented_options_popup_for_owner(owner_key)
    }

    pub(crate) fn close_presented_options_for_blurred_owner(
        &mut self,
        owner_key: DomNodeSlotKey,
    ) -> Result<bool, DomError> {
        self.close_presented_options_popup_for_owner(owner_key, false)
    }

    pub(crate) fn reopen_presented_options_for_text_input_value_change(
        &mut self,
        owner_key: DomNodeSlotKey,
    ) -> Result<(), DomError> {
        if self.resolve_presented_options_owner_kind(owner_key)
            != Some(PresentedOptionsOwnerKind::TextInput)
        {
            return Ok(());
        }
        let _ = self.open_presented_options_popup_for_owner(owner_key)?;
        Ok(())
    }

    pub(crate) fn resolve_presented_options_owner_kind(
        &self,
        owner_key: DomNodeSlotKey,
    ) -> Option<PresentedOptionsOwnerKind> {
        let element = self.node(owner_key)?.as_element()?;
        if element.tag == DomTag::Select {
            return Some(PresentedOptionsOwnerKind::Select);
        }
        match element.behavior().present_options_behavior {
            DomElementPresentOptionsBehavior::None => None,
            DomElementPresentOptionsBehavior::TextInput => self
                .resolve_presented_options_datalist_key_for_owner(owner_key)
                .map(|_| PresentedOptionsOwnerKind::TextInput),
        }
    }

    pub(crate) fn open_presented_options_popup_for_owner(
        &mut self,
        owner_key: DomNodeSlotKey,
    ) -> Result<bool, DomError> {
        let Some(owner_kind) = self.resolve_presented_options_owner_kind(owner_key) else {
            return Ok(false);
        };
        self.remove_other_presented_options_popups_immediately(owner_key)?;
        self.replace_presented_options_popup_for_owner_kind(owner_kind, owner_key)
    }

    pub(crate) fn open_select_popup_for_owner(
        &mut self,
        owner_key: DomNodeSlotKey,
    ) -> Result<bool, DomError> {
        if self.resolve_select_owner_key_from_target(owner_key) != Some(owner_key) {
            return Ok(false);
        }
        self.remove_other_presented_options_popups_immediately(owner_key)?;
        self.replace_presented_options_popup_for_owner_kind(
            PresentedOptionsOwnerKind::Select,
            owner_key,
        )
    }

    pub(crate) fn close_presented_options_popup_for_owner(
        &mut self,
        owner_key: DomNodeSlotKey,
        immediate: bool,
    ) -> Result<bool, DomError> {
        let Some(shell_key) = self.presented_options_popup_shell_key(owner_key) else {
            return Ok(false);
        };
        if immediate {
            self.remove_node(shell_key)?;
            return Ok(true);
        }
        if presented_options_popup_state(self, shell_key) == Some(PRESENTED_OPTIONS_STATE_CLOSING) {
            return Ok(true);
        }
        self.set_presented_options_popup_state(shell_key, PRESENTED_OPTIONS_STATE_CLOSING)?;
        self.set_presented_options_popup_opacity(shell_key, 0.0)?;
        Ok(true)
    }

    pub(crate) fn presented_options_popup_shell_key(
        &self,
        owner_key: DomNodeSlotKey,
    ) -> Option<DomNodeSlotKey> {
        collect_direct_child_element_keys(self, owner_key)
            .into_iter()
            .find(|key| {
                presented_options_role_value(self, *key) == Some(PRESENTED_OPTIONS_ROLE_SHELL)
            })
    }

    pub(crate) fn presented_options_highlighted_row_key(
        &self,
        owner_key: DomNodeSlotKey,
    ) -> Option<DomNodeSlotKey> {
        self.presented_options_popup_row_keys(owner_key)
            .into_iter()
            .find(|row_key| presented_options_row_is_highlighted(self, *row_key))
    }

    pub(crate) fn presented_options_popup_row_keys(
        &self,
        owner_key: DomNodeSlotKey,
    ) -> Vec<DomNodeSlotKey> {
        let Some(scroll_key) = self.presented_options_popup_scroll_key(owner_key) else {
            return Vec::new();
        };
        collect_direct_child_element_keys(self, scroll_key)
            .into_iter()
            .filter(|key| {
                presented_options_role_value(self, *key) == Some(PRESENTED_OPTIONS_ROLE_ROW)
            })
            .collect()
    }

    fn move_presented_options_highlight_or_open_popup(
        &mut self,
        owner_key: DomNodeSlotKey,
        move_forward: bool,
    ) -> Result<bool, DomError> {
        if self.presented_options_popup_shell_key(owner_key).is_some()
            && !self.presented_options_popup_is_closing_for_owner(owner_key)
        {
            return self.move_presented_options_popup_highlight(owner_key, move_forward);
        }
        self.open_presented_options_popup_for_owner(owner_key)
    }

    fn commit_highlighted_presented_option_or_open_popup(
        &mut self,
        owner_key: DomNodeSlotKey,
    ) -> Result<bool, DomError> {
        if self.presented_options_popup_shell_key(owner_key).is_some()
            && !self.presented_options_popup_is_closing_for_owner(owner_key)
        {
            return self.commit_highlighted_presented_option_for_owner(owner_key);
        }
        self.open_presented_options_popup_for_owner(owner_key)
    }

    fn replace_presented_options_popup_for_owner_kind(
        &mut self,
        owner_kind: PresentedOptionsOwnerKind,
        owner_key: DomNodeSlotKey,
    ) -> Result<bool, DomError> {
        let popup_anchor = presented_options_popup_anchor_for_owner(self, owner_key)?;
        let option_records =
            collect_presented_option_records_from_owner_source(self, owner_kind, owner_key)?;
        if option_records.is_empty() {
            let _ = self.close_presented_options_popup_for_owner(owner_key, true)?;
            return Ok(false);
        }
        let previous_highlighted_index =
            self.highlighted_presented_options_source_index_for_owner(owner_key);
        let _ = self.close_presented_options_popup_for_owner(owner_key, true)?;
        let highlighted_index = choose_presented_options_highlighted_index(
            self,
            owner_kind,
            owner_key,
            &option_records,
            previous_highlighted_index,
        );
        let popup_shell_key = self.append_presented_options_popup_shell_and_rows(
            owner_key,
            popup_anchor,
            &option_records,
            highlighted_index,
        )?;
        self.update_presented_options_popup_box_attrs_from_owner(owner_key)?;
        self.set_presented_options_popup_state(popup_shell_key, PRESENTED_OPTIONS_STATE_OPEN)?;
        self.set_presented_options_popup_opacity(popup_shell_key, 1.0)?;
        Ok(true)
    }

    fn append_presented_options_popup_shell_and_rows(
        &mut self,
        owner_key: DomNodeSlotKey,
        popup_anchor: PresentedOptionsPopupAnchor,
        option_records: &[PresentedOptionRecord],
        highlighted_index: usize,
    ) -> Result<DomNodeSlotKey, DomError> {
        let popup_height_px =
            measure_presented_options_popup_height_px(self, owner_key, option_records.len())?;
        let popup_shell_key = self.create_element(DomTag::Div)?;
        let popup_scroll_key = self.create_element(DomTag::Div)?;
        self.set_layout_layer_order(popup_shell_key, LayerOrder::Raised)?;
        self.set_presented_options_popup_shell_style_attrs(
            popup_shell_key,
            popup_anchor,
            popup_height_px,
        )?;
        self.set_presented_options_popup_scroll_style_attrs(
            popup_scroll_key,
            popup_anchor.owner_box.width_px,
            popup_height_px,
        )?;
        self.append_child(popup_shell_key, popup_scroll_key)?;
        for (source_index, option_record) in option_records.iter().enumerate() {
            self.append_presented_options_row_element(
                popup_scroll_key,
                popup_anchor.owner_box.width_px,
                source_index,
                option_record,
                source_index == highlighted_index,
            )?;
        }
        self.append_child(owner_key, popup_shell_key)?;
        Ok(popup_shell_key)
    }

    fn append_presented_options_row_element(
        &mut self,
        popup_scroll_key: DomNodeSlotKey,
        owner_width_px: f32,
        source_index: usize,
        option_record: &PresentedOptionRecord,
        highlighted: bool,
    ) -> Result<(), DomError> {
        let row_key = self.create_element(DomTag::Option)?;
        let text_key = self.create_text_node(option_record.label.clone())?;
        let row_content_height_px = presented_options_row_content_height_px();
        self.append_child(popup_scroll_key, row_key)?;
        self.append_child(row_key, text_key)?;
        self.set_attr_str(row_key, "display", "block")?;
        self.set_attr(
            row_key,
            DomAttribute::PresentedOptionsRole,
            Some(DomAttributeValue::String(
                PRESENTED_OPTIONS_ROLE_ROW.to_string(),
            )),
        )?;
        self.set_attr(
            row_key,
            DomAttribute::PresentedOptionsSourceIndex,
            Some(DomAttributeValue::Integer(
                i64::try_from(source_index).expect("source index should fit into i64"),
            )),
        )?;
        self.set_attr(
            row_key,
            DomAttribute::Value,
            Some(DomAttributeValue::String(option_record.value.clone())),
        )?;
        self.set_attr(
            row_key,
            DomAttribute::Width,
            Some(DomAttributeValue::String(format!(
                "{}px",
                owner_width_px.max(1.0)
            ))),
        )?;
        self.set_attr(
            row_key,
            DomAttribute::Height,
            Some(DomAttributeValue::String(format!(
                "{row_content_height_px}px"
            ))),
        )?;
        self.set_attr(
            row_key,
            DomAttribute::LineHeight,
            Some(DomAttributeValue::String(format!(
                "{row_content_height_px}px"
            ))),
        )?;
        self.set_attr(
            row_key,
            DomAttribute::PaddingLeft,
            Some(DomAttributeValue::String(format!(
                "{PRESENTED_OPTIONS_HORIZONTAL_PADDING_PX}px"
            ))),
        )?;
        self.set_attr(
            row_key,
            DomAttribute::PaddingRight,
            Some(DomAttributeValue::String(format!(
                "{PRESENTED_OPTIONS_HORIZONTAL_PADDING_PX}px"
            ))),
        )?;
        self.set_attr(
            row_key,
            DomAttribute::PaddingTop,
            Some(DomAttributeValue::String(format!(
                "{PRESENTED_OPTIONS_VERTICAL_PADDING_PX}px"
            ))),
        )?;
        self.set_attr(
            row_key,
            DomAttribute::PaddingBottom,
            Some(DomAttributeValue::String(format!(
                "{PRESENTED_OPTIONS_VERTICAL_PADDING_PX}px"
            ))),
        )?;
        self.set_presented_options_row_highlight_attrs(row_key, highlighted)
    }

    fn set_presented_options_popup_shell_style_attrs(
        &mut self,
        popup_shell_key: DomNodeSlotKey,
        popup_anchor: PresentedOptionsPopupAnchor,
        popup_height_px: f32,
    ) -> Result<(), DomError> {
        self.set_attr_str(popup_shell_key, "display", "block")?;
        self.set_attr_str(popup_shell_key, "position", "absolute")?;
        self.set_presented_options_popup_anchor_attrs(popup_shell_key, popup_anchor)?;
        self.set_attr(
            popup_shell_key,
            DomAttribute::Width,
            Some(DomAttributeValue::String(format!(
                "{}px",
                popup_anchor.owner_box.width_px.max(1.0)
            ))),
        )?;
        self.set_attr(
            popup_shell_key,
            DomAttribute::Height,
            Some(DomAttributeValue::String(format!("{popup_height_px}px"))),
        )?;
        self.set_attr_str(popup_shell_key, "transition", "opacity 120ms ease-out")?;
        self.set_attr(
            popup_shell_key,
            DomAttribute::Custom("background-color".to_string()),
            Some(DomAttributeValue::String("#141922".to_string())),
        )?;
        self.set_attr(
            popup_shell_key,
            DomAttribute::Custom("border-color".to_string()),
            Some(DomAttributeValue::String("#394254".to_string())),
        )?;
        self.set_attr_str(popup_shell_key, "border-top", "1px")?;
        self.set_attr_str(popup_shell_key, "border-right", "1px")?;
        self.set_attr_str(popup_shell_key, "border-bottom", "1px")?;
        self.set_attr_str(popup_shell_key, "border-left", "1px")?;
        self.set_attr(
            popup_shell_key,
            DomAttribute::PresentedOptionsRole,
            Some(DomAttributeValue::String(
                PRESENTED_OPTIONS_ROLE_SHELL.to_string(),
            )),
        )?;
        self.set_attr(
            popup_shell_key,
            DomAttribute::PresentedOptionsState,
            Some(DomAttributeValue::String(
                PRESENTED_OPTIONS_STATE_OPEN.to_string(),
            )),
        )
    }

    fn set_presented_options_popup_anchor_attrs(
        &mut self,
        popup_shell_key: DomNodeSlotKey,
        popup_anchor: PresentedOptionsPopupAnchor,
    ) -> Result<(), DomError> {
        self.set_attr(
            popup_shell_key,
            DomAttribute::Left,
            Some(DomAttributeValue::String(format!(
                "{}px",
                popup_anchor.owner_box.left_px
            ))),
        )?;
        self.set_attr(
            popup_shell_key,
            DomAttribute::Top,
            Some(DomAttributeValue::String(format!(
                "{}px",
                popup_anchor.owner_box.top_px + popup_anchor.owner_box.height_px
            ))),
        )
    }

    fn set_presented_options_popup_scroll_style_attrs(
        &mut self,
        popup_scroll_key: DomNodeSlotKey,
        owner_width_px: f32,
        popup_height_px: f32,
    ) -> Result<(), DomError> {
        self.set_attr_str(popup_scroll_key, "display", "block")?;
        self.set_attr_str(popup_scroll_key, "overflow", "scroll")?;
        self.set_attr(
            popup_scroll_key,
            DomAttribute::Width,
            Some(DomAttributeValue::String(format!(
                "{}px",
                owner_width_px.max(1.0)
            ))),
        )?;
        self.set_attr(
            popup_scroll_key,
            DomAttribute::Height,
            Some(DomAttributeValue::String(format!("{popup_height_px}px"))),
        )?;
        self.set_attr(
            popup_scroll_key,
            DomAttribute::PresentedOptionsRole,
            Some(DomAttributeValue::String(
                PRESENTED_OPTIONS_ROLE_SCROLL.to_string(),
            )),
        )
    }

    fn update_presented_options_popup_box_attrs_from_owner(
        &mut self,
        owner_key: DomNodeSlotKey,
    ) -> Result<(), DomError> {
        let Some(shell_key) = self.presented_options_popup_shell_key(owner_key) else {
            return Ok(());
        };
        let Some(scroll_key) = self.presented_options_popup_scroll_key(owner_key) else {
            return Ok(());
        };
        let popup_anchor = presented_options_popup_anchor_for_owner(self, owner_key)?;
        let popup_height_px = measure_presented_options_popup_height_px(
            self,
            owner_key,
            self.presented_options_popup_row_keys(owner_key).len(),
        )?;
        self.set_presented_options_popup_anchor_attrs(shell_key, popup_anchor)?;
        self.set_attr(
            shell_key,
            DomAttribute::Width,
            Some(DomAttributeValue::String(format!(
                "{}px",
                popup_anchor.owner_box.width_px.max(1.0)
            ))),
        )?;
        self.set_attr(
            shell_key,
            DomAttribute::Height,
            Some(DomAttributeValue::String(format!("{popup_height_px}px"))),
        )?;
        self.set_attr(
            scroll_key,
            DomAttribute::Width,
            Some(DomAttributeValue::String(format!(
                "{}px",
                popup_anchor.owner_box.width_px.max(1.0)
            ))),
        )?;
        self.set_attr(
            scroll_key,
            DomAttribute::Height,
            Some(DomAttributeValue::String(format!("{popup_height_px}px"))),
        )?;
        Ok(())
    }

    fn remove_other_presented_options_popups_immediately(
        &mut self,
        owner_key: DomNodeSlotKey,
    ) -> Result<(), DomError> {
        for other_owner_key in self.owner_keys_with_presented_options_popup() {
            if other_owner_key != owner_key {
                let _ = self.close_presented_options_popup_for_owner(other_owner_key, true)?;
            }
        }
        Ok(())
    }

    fn commit_presented_option_hit_row(
        &mut self,
        target_key: DomNodeSlotKey,
    ) -> Result<bool, DomError> {
        let Some(row_key) = self.resolve_presented_options_row_key_from_target(target_key) else {
            return Ok(false);
        };
        let Some(owner_key) = self.resolve_presented_options_owner_key_from_target(target_key)
        else {
            return Ok(false);
        };
        let Some(source_index) = presented_options_row_source_index(self, row_key) else {
            return Ok(false);
        };
        let Some(owner_kind) = self.resolve_presented_options_owner_kind(owner_key) else {
            return Ok(false);
        };
        let option_records =
            collect_presented_option_records_from_owner_source(self, owner_kind, owner_key)?;
        let Some(option_record) = option_records.get(source_index) else {
            return Ok(false);
        };
        self.apply_presented_option_to_owner_and_dispatch_events(
            owner_key,
            option_record.source_key,
            option_record.value.clone(),
        )
    }

    fn commit_select_presented_option_hit_row(
        &mut self,
        target_key: DomNodeSlotKey,
    ) -> Result<bool, DomError> {
        let Some(row_key) = self.resolve_presented_options_row_key_from_target(target_key) else {
            return Ok(false);
        };
        let Some(owner_key) = self.resolve_select_owner_key_from_target(target_key) else {
            return Ok(false);
        };
        let Some(source_index) = presented_options_row_source_index(self, row_key) else {
            return Ok(false);
        };
        let option_records = collect_presented_option_records_from_owner_source(
            self,
            PresentedOptionsOwnerKind::Select,
            owner_key,
        )?;
        let Some(option_record) = option_records.get(source_index) else {
            return Ok(false);
        };
        self.apply_select_option_to_owner_and_dispatch_events(
            owner_key,
            option_record.source_key,
            option_record.value.clone(),
        )
    }

    fn commit_highlighted_presented_option_for_owner(
        &mut self,
        owner_key: DomNodeSlotKey,
    ) -> Result<bool, DomError> {
        let Some(source_index) =
            self.highlighted_presented_options_source_index_for_owner(owner_key)
        else {
            return Ok(false);
        };
        let Some(owner_kind) = self.resolve_presented_options_owner_kind(owner_key) else {
            return Ok(false);
        };
        let option_records =
            collect_presented_option_records_from_owner_source(self, owner_kind, owner_key)?;
        let Some(option_record) = option_records.get(source_index) else {
            return Ok(false);
        };
        self.apply_presented_option_to_owner_and_dispatch_events(
            owner_key,
            option_record.source_key,
            option_record.value.clone(),
        )
    }

    fn apply_presented_option_to_owner_and_dispatch_events(
        &mut self,
        owner_key: DomNodeSlotKey,
        source_key: DomNodeSlotKey,
        value: String,
    ) -> Result<bool, DomError> {
        let Some(owner_kind) = self.resolve_presented_options_owner_kind(owner_key) else {
            return Ok(false);
        };
        match owner_kind {
            PresentedOptionsOwnerKind::Select => {
                return self.apply_select_option_to_owner_and_dispatch_events(
                    owner_key, source_key, value,
                );
            }
            PresentedOptionsOwnerKind::TextInput => {
                self.set_attr(
                    owner_key,
                    DomAttribute::Value,
                    Some(DomAttributeValue::String(value.clone())),
                )?;
                let _ = self.dispatch_input_event_to_target_key(owner_key, value.clone())?;
                let _ = self.dispatch_change_event_to_target_key(owner_key, value)?;
            }
        }
        let _ = self.close_presented_options_popup_for_owner(owner_key, false)?;
        Ok(true)
    }

    fn apply_select_option_to_owner_and_dispatch_events(
        &mut self,
        owner_key: DomNodeSlotKey,
        source_key: DomNodeSlotKey,
        value: String,
    ) -> Result<bool, DomError> {
        set_select_source_option_selected_attrs(self, owner_key, source_key)?;
        self.set_attr(
            owner_key,
            DomAttribute::Value,
            Some(DomAttributeValue::String(value.clone())),
        )?;
        let _ = self.dispatch_input_event_to_target_key(owner_key, value.clone())?;
        let _ = self.dispatch_change_event_to_target_key(owner_key, value)?;
        let _ = self.close_presented_options_popup_for_owner(owner_key, false)?;
        Ok(true)
    }

    fn move_presented_options_popup_highlight(
        &mut self,
        owner_key: DomNodeSlotKey,
        move_forward: bool,
    ) -> Result<bool, DomError> {
        let row_keys = self.presented_options_popup_row_keys(owner_key);
        if row_keys.is_empty() {
            return Ok(false);
        }
        let current_index = row_keys
            .iter()
            .position(|row_key| presented_options_row_is_highlighted(self, *row_key))
            .unwrap_or(0);
        let next_index = if move_forward {
            (current_index + 1).min(row_keys.len() - 1)
        } else {
            current_index.saturating_sub(1)
        };
        self.highlight_presented_options_popup_row_at_index(owner_key, next_index)?;
        Ok(true)
    }

    fn highlight_presented_options_popup_row_at_index(
        &mut self,
        owner_key: DomNodeSlotKey,
        highlighted_index: usize,
    ) -> Result<(), DomError> {
        let row_keys = self.presented_options_popup_row_keys(owner_key);
        for (row_index, row_key) in row_keys.into_iter().enumerate() {
            self.set_presented_options_row_highlight_attrs(
                row_key,
                row_index == highlighted_index,
            )?;
        }
        Ok(())
    }

    fn set_presented_options_row_highlight_attrs(
        &mut self,
        row_key: DomNodeSlotKey,
        highlighted: bool,
    ) -> Result<(), DomError> {
        self.set_attr(
            row_key,
            DomAttribute::PresentedOptionsHighlighted,
            highlighted.then_some(DomAttributeValue::Bool(true)),
        )?;
        self.set_attr(
            row_key,
            DomAttribute::Custom("background-color".to_string()),
            highlighted.then(|| DomAttributeValue::String("#273754".to_string())),
        )
    }

    fn resolve_presented_options_owner_key_from_target(
        &self,
        target_key: DomNodeSlotKey,
    ) -> Option<DomNodeSlotKey> {
        let mut current_key = Some(target_key);
        while let Some(key) = current_key {
            let node = self.node(key)?;
            if let Some(element) = node.as_element()
                && (element.tag == DomTag::Select
                    || element.behavior().present_options_behavior
                        != DomElementPresentOptionsBehavior::None)
            {
                return Some(key);
            }
            current_key = node.parent_key();
        }
        None
    }

    fn resolve_select_owner_key_from_target(
        &self,
        target_key: DomNodeSlotKey,
    ) -> Option<DomNodeSlotKey> {
        let mut current_key = Some(target_key);
        while let Some(key) = current_key {
            let node = self.node(key)?;
            if node
                .as_element()
                .is_some_and(|element| element.tag == DomTag::Select)
            {
                return Some(key);
            }
            current_key = node.parent_key();
        }
        None
    }

    fn resolve_presented_options_row_key_from_target(
        &self,
        target_key: DomNodeSlotKey,
    ) -> Option<DomNodeSlotKey> {
        let mut current_key = Some(target_key);
        while let Some(key) = current_key {
            if presented_options_role_value(self, key) == Some(PRESENTED_OPTIONS_ROLE_ROW) {
                return Some(key);
            }
            current_key = self.node(key).and_then(DomNode::parent_key);
        }
        None
    }

    pub(crate) fn resolve_presented_options_datalist_key_for_owner(
        &self,
        owner_key: DomNodeSlotKey,
    ) -> Option<DomNodeSlotKey> {
        let owner = self.node(owner_key)?.as_element()?;
        let list_id = owner
            .attr(&DomAttribute::List)
            .and_then(string_attr_value)?;
        let (list_key, list_node) = self.get_element_by_id(list_id)?;
        let list_element = list_node.as_element()?;
        if list_element.tag != DomTag::Datalist {
            return None;
        }
        Some(list_key)
    }

    fn owner_keys_with_presented_options_popup(&self) -> Vec<DomNodeSlotKey> {
        self.nodes
            .iter()
            .filter_map(|(key, node)| {
                let element = node.as_element()?;
                if element.tag != DomTag::Select
                    && element.behavior().present_options_behavior
                        == DomElementPresentOptionsBehavior::None
                {
                    return None;
                }
                self.presented_options_popup_shell_key(key).map(|_| key)
            })
            .collect()
    }

    fn presented_options_popup_scroll_key(
        &self,
        owner_key: DomNodeSlotKey,
    ) -> Option<DomNodeSlotKey> {
        let shell_key = self.presented_options_popup_shell_key(owner_key)?;
        collect_direct_child_element_keys(self, shell_key)
            .into_iter()
            .find(|key| {
                presented_options_role_value(self, *key) == Some(PRESENTED_OPTIONS_ROLE_SCROLL)
            })
    }

    fn presented_options_popup_is_closing_for_owner(&self, owner_key: DomNodeSlotKey) -> bool {
        self.presented_options_popup_shell_key(owner_key)
            .and_then(|shell_key| presented_options_popup_state(self, shell_key))
            .is_some_and(|state| state == PRESENTED_OPTIONS_STATE_CLOSING)
    }

    fn highlighted_presented_options_source_index_for_owner(
        &self,
        owner_key: DomNodeSlotKey,
    ) -> Option<usize> {
        let row_key = self.presented_options_highlighted_row_key(owner_key)?;
        presented_options_row_source_index(self, row_key)
    }

    fn set_presented_options_popup_state(
        &mut self,
        shell_key: DomNodeSlotKey,
        state: &str,
    ) -> Result<(), DomError> {
        self.set_attr(
            shell_key,
            DomAttribute::PresentedOptionsState,
            Some(DomAttributeValue::String(state.to_string())),
        )
    }

    fn set_presented_options_popup_opacity(
        &mut self,
        shell_key: DomNodeSlotKey,
        opacity: f32,
    ) -> Result<(), DomError> {
        self.set_attr(
            shell_key,
            DomAttribute::Custom("opacity".to_string()),
            Some(DomAttributeValue::Float(opacity)),
        )
    }
}

fn range_drag_subtree_point(session: &ActiveRangeDragSession, root_point: PointPx) -> PointPx {
    PointPx {
        x_px: root_point.x_px + session.root_to_subtree_point_offset.x_px,
        y_px: root_point.y_px + session.root_to_subtree_point_offset.y_px,
    }
}

pub(crate) fn next_id_from_attr(
    attribute: &DomAttribute,
    value: Option<&DomAttributeValue>,
) -> Result<Option<String>, DomError> {
    if *attribute != DomAttribute::Id {
        return Ok(None);
    }

    match value {
        None => Ok(None),
        Some(DomAttributeValue::String(value)) => Ok(Some(value.clone())),
        _ => Err(DomError::InvalidAttributeValue),
    }
}

fn style_removed_attributes_for_node(
    previous_stylesheet_attrs: Option<&BTreeSet<DomAttribute>>,
    next_map: &ComputedStyleMap,
) -> Vec<DomAttribute> {
    let Some(stored_attributes) = previous_stylesheet_attrs else {
        return Vec::new();
    };
    stored_attributes
        .iter()
        .filter(|attribute| {
            style_attr_allows_property(attribute) && !next_map.contains_key(*attribute)
        })
        .cloned()
        .collect()
}

fn transition_longhand_attr_values(
    values: TransitionLonghandValues,
) -> [(DomAttribute, Option<DomAttributeValue>); 4] {
    [
        (DomAttribute::TransitionProperty, values.property),
        (DomAttribute::TransitionDuration, values.duration),
        (
            DomAttribute::TransitionTimingFunction,
            values.timing_function,
        ),
        (DomAttribute::TransitionDelay, values.delay),
    ]
}

fn transition_for_attr_change(
    node: &DomNode,
    key: DomNodeSlotKey,
    attribute: &DomAttribute,
    next_value: Option<&DomAttributeValue>,
    now_s: f64,
) -> Option<DomTransition> {
    let spec = transition_spec_for_attribute(
        node.attr(&DomAttribute::TransitionProperty),
        node.attr(&DomAttribute::TransitionDuration),
        node.attr(&DomAttribute::TransitionTimingFunction),
        node.attr(&DomAttribute::TransitionDelay),
        attribute,
    );
    let spec = match spec {
        Ok(Some(spec)) => spec,
        Ok(None) => return None,
        Err(error) => {
            tracing::warn!(
                attribute = ?attribute,
                error = ?error,
                "transition_ignored"
            );
            return None;
        }
    };
    let start_value = node
        .presented_attr(attribute)
        .map(|value| normalize_transition_value(attribute, value))?;
    let next_value = transition_target_value(attribute, next_value)?;
    if start_value == next_value {
        return None;
    }
    Some(DomTransition::new(DomTransitionConfig {
        node_key: key,
        attribute: attribute.clone(),
        from: start_value,
        to: next_value,
        started_at_s: now_s,
        duration_s: spec.duration_s,
        delay_s: spec.delay_s,
        timing: spec.timing,
    }))
}

fn transition_target_value(
    attribute: &DomAttribute,
    next_value: Option<&DomAttributeValue>,
) -> Option<DomAttributeValue> {
    next_value
        .map(|value| normalize_transition_value(attribute, value))
        .or_else(|| removed_attr_transition_target(attribute))
}

fn removed_attr_transition_target(attribute: &DomAttribute) -> Option<DomAttributeValue> {
    if attribute_uses_zero_when_missing(attribute) {
        return Some(DomAttributeValue::Float(0.0));
    }
    None
}

fn attribute_uses_zero_when_missing(attribute: &DomAttribute) -> bool {
    matches!(
        attribute,
        DomAttribute::Top
            | DomAttribute::Left
            | DomAttribute::Right
            | DomAttribute::Bottom
            | DomAttribute::MarginTop
            | DomAttribute::MarginLeft
            | DomAttribute::MarginRight
            | DomAttribute::MarginBottom
            | DomAttribute::PaddingTop
            | DomAttribute::PaddingLeft
            | DomAttribute::PaddingRight
            | DomAttribute::PaddingBottom
            | DomAttribute::BorderTop
            | DomAttribute::BorderLeft
            | DomAttribute::BorderRight
            | DomAttribute::BorderBottom
    )
}

fn normalize_transition_value(
    attribute: &DomAttribute,
    value: &DomAttributeValue,
) -> DomAttributeValue {
    if let Some(numeric_value) = parse_transition_numeric_value(attribute, value) {
        return DomAttributeValue::Float(numeric_value);
    }
    value.clone()
}

fn validate_and_normalize_transition_payload(
    transition: DomTransition,
) -> Result<DomTransition, DomError> {
    let attribute = transition.attribute.clone();
    validate_transition_value(&attribute, &transition.from)?;
    validate_transition_value(&attribute, &transition.to)?;
    validate_transition_value(&attribute, &transition.presented)?;
    Ok(DomTransition {
        from: normalize_transition_value(&attribute, &transition.from),
        to: normalize_transition_value(&attribute, &transition.to),
        presented: normalize_transition_value(&attribute, &transition.presented),
        ..transition
    })
}

fn validate_transition_value(
    attribute: &DomAttribute,
    value: &DomAttributeValue,
) -> Result<(), DomError> {
    if transition_attribute_uses_numeric_interpolation(attribute)
        && parse_transition_numeric_value(attribute, value).is_none()
    {
        return Err(DomError::InvalidAttributeValue);
    }
    Ok(())
}

fn transition_attribute_uses_numeric_interpolation(attribute: &DomAttribute) -> bool {
    matches!(attribute, DomAttribute::Custom(name) if name == "opacity")
        || matches!(
            attribute,
            DomAttribute::Width
                | DomAttribute::Height
                | DomAttribute::MinWidth
                | DomAttribute::MinHeight
                | DomAttribute::MaxWidth
                | DomAttribute::MaxHeight
                | DomAttribute::Top
                | DomAttribute::Left
                | DomAttribute::Right
                | DomAttribute::Bottom
                | DomAttribute::MarginTop
                | DomAttribute::MarginLeft
                | DomAttribute::MarginRight
                | DomAttribute::MarginBottom
                | DomAttribute::PaddingTop
                | DomAttribute::PaddingLeft
                | DomAttribute::PaddingRight
                | DomAttribute::PaddingBottom
                | DomAttribute::BorderTop
                | DomAttribute::BorderLeft
                | DomAttribute::BorderRight
                | DomAttribute::BorderBottom
        )
}

fn parse_transition_numeric_value(
    attribute: &DomAttribute,
    value: &DomAttributeValue,
) -> Option<f32> {
    if matches!(attribute, DomAttribute::Custom(name) if name == "opacity") {
        return match value {
            DomAttributeValue::String(value) => value.parse::<f32>().ok(),
            DomAttributeValue::Float(value) => Some(*value),
            DomAttributeValue::Integer(value) => Some(crate::numeric::i64_to_f32(*value)),
            DomAttributeValue::Bool(_) | DomAttributeValue::Color(_) => None,
        };
    }
    if !matches!(
        attribute,
        DomAttribute::Width
            | DomAttribute::Height
            | DomAttribute::MinWidth
            | DomAttribute::MinHeight
            | DomAttribute::MaxWidth
            | DomAttribute::MaxHeight
            | DomAttribute::Top
            | DomAttribute::Left
            | DomAttribute::Right
            | DomAttribute::Bottom
            | DomAttribute::MarginTop
            | DomAttribute::MarginLeft
            | DomAttribute::MarginRight
            | DomAttribute::MarginBottom
            | DomAttribute::PaddingTop
            | DomAttribute::PaddingLeft
            | DomAttribute::PaddingRight
            | DomAttribute::PaddingBottom
            | DomAttribute::BorderTop
            | DomAttribute::BorderLeft
            | DomAttribute::BorderRight
            | DomAttribute::BorderBottom
    ) {
        return None;
    }
    match value {
        DomAttributeValue::Float(value) => Some(*value),
        DomAttributeValue::Integer(value) => Some(crate::numeric::i64_to_f32(*value)),
        DomAttributeValue::String(value) if value == "0" => Some(0.0),
        DomAttributeValue::String(value) => value.strip_suffix("px")?.parse::<f32>().ok(),
        DomAttributeValue::Bool(_) | DomAttributeValue::Color(_) => None,
    }
}

fn mark_dirty_up_to_layer_root(root_dom: &mut RootDom, key: DomNodeSlotKey, dirty_flag: DirtyFlag) {
    // Mutation dirtiness never crosses a layer boundary. A later layout pass
    // may still propagate solved size changes to parent layers, but that is a
    // separate pipeline and must not be inferred from retained dirty bits.
    let Some(layer_root_key) = current_layer_root_key(&root_dom.nodes, key) else {
        return;
    };
    let mut current_key = Some(key);
    while let Some(current) = current_key {
        let (was_dirty, parent_key) = {
            let Some(node) = root_dom.nodes.get_mut(current) else {
                return;
            };
            let was_dirty = match dirty_flag {
                DirtyFlag::Layout => node.mark_layout_dirty(),
                DirtyFlag::Style => node.mark_style_dirty(),
            };
            (was_dirty, node.parent_key())
        };
        if current == layer_root_key {
            return;
        }
        if was_dirty && matches!(dirty_flag, DirtyFlag::Layout) {
            return;
        }
        current_key = parent_key;
    }
}

fn mark_newly_attached_subtree_dirty(root_dom: &mut RootDom, subtree_root_key: DomNodeSlotKey) {
    let mut stack = vec![subtree_root_key];
    while let Some(current_key) = stack.pop() {
        let child_keys = {
            let Some(node) = root_dom.nodes.get_mut(current_key) else {
                continue;
            };
            node.mark_style_dirty();
            node.mark_layout_dirty();
            node.mark_full_layer_repaint();

            let mut child_keys = Vec::new();
            let mut next_child_key = node.first_child_key_for_store();
            while let Some(child_key) = next_child_key {
                child_keys.push(child_key);
                next_child_key = root_dom
                    .nodes
                    .get(child_key)
                    .and_then(DomNode::next_sibling_key_for_store);
            }
            child_keys
        };
        stack.extend(child_keys.into_iter().rev());
    }
}

fn queue_newly_connected_layer_roots_in_subtree(
    root_dom: &mut RootDom,
    subtree_root_key: DomNodeSlotKey,
) {
    let mut stack = vec![subtree_root_key];
    while let Some(current_key) = stack.pop() {
        let Some(node) = root_dom.node(current_key) else {
            continue;
        };
        if layer_kind_for_node(node) != crate::types::DomLayerKind::None {
            root_dom.queue_layer_append_root(current_key);
            continue;
        }
        let mut child_key = node.first_child_key_for_store();
        while let Some(current_child_key) = child_key {
            stack.push(current_child_key);
            child_key = root_dom
                .node(current_child_key)
                .and_then(DomNode::next_sibling_key_for_store);
        }
    }
}

fn event_point(event: &DomEvent) -> Option<PointPx> {
    match event.detail {
        DomEventDetail::Pointer(detail) => Some(PointPx {
            x_px: detail.client_x,
            y_px: detail.client_y,
        }),
        DomEventDetail::Wheel(detail) => Some(PointPx {
            x_px: detail.client_x,
            y_px: detail.client_y,
        }),
        _ => None,
    }
}

fn pointer_moved(previous_pointer: Option<DomPointerState>, pointer: DomPointerState) -> bool {
    previous_pointer.is_none_or(|previous| {
        (previous.x_px - pointer.x_px).abs() > f32::EPSILON
            || (previous.y_px - pointer.y_px).abs() > f32::EPSILON
    })
}

fn hover_pointer_event(pointer: DomPointerState) -> DomPointerEvent {
    DomPointerEvent {
        pointer_id: 1,
        pointer_type: DomPointerType::Mouse,
        is_primary: true,
        button: None,
        buttons: u16::from(pointer.primary_button_down),
        client_x: pointer.x_px,
        client_y: pointer.y_px,
        modifiers: DomModifierState::default(),
    }
}

fn pointer_pressed(previous_pointer: Option<DomPointerState>, pointer: DomPointerState) -> bool {
    previous_pointer.map_or(pointer.primary_button_down, |previous| {
        !previous.primary_button_down && pointer.primary_button_down
    })
}

fn pointer_released(previous_pointer: Option<DomPointerState>, pointer: DomPointerState) -> bool {
    previous_pointer
        .is_some_and(|previous| previous.primary_button_down && !pointer.primary_button_down)
}

fn hover_layer_intersects_removed_keys(
    hovered_layer: &HoveredLayerEntry,
    removed_keys: &[DomNodeSlotKey],
) -> bool {
    removed_keys.contains(&hovered_layer.layer_root_key)
        || hovered_layer
            .element_chain_root_to_target
            .iter()
            .any(|key| removed_keys.contains(key))
}

fn node_allows_vertical_scroll_offset(node: &DomNode) -> bool {
    matches!(
        node.presented_attr(&DomAttribute::Overflow),
        Some(DomAttributeValue::String(value))
            if matches!(value.as_str(), "scroll" | "auto" | "hidden")
    )
}

fn scroll_viewport_height_px_for_node(
    root_dom: &RootDom,
    key: DomNodeSlotKey,
) -> Result<f32, DomError> {
    let node = root_dom.node(key).ok_or(DomError::InvalidNodeKey)?;
    let box_model = node_box_from_ref(node);
    Ok((box_model.border_box.height_px
        - box_model.border.top_px
        - box_model.border.bottom_px
        - box_model.padding.top_px
        - box_model.padding.bottom_px)
        .max(0.0))
}

fn scroll_content_height_px_for_node(
    root_dom: &RootDom,
    key: DomNodeSlotKey,
) -> Result<f32, DomError> {
    let node = root_dom.node(key).ok_or(DomError::InvalidNodeKey)?;
    Ok(node.layout().content_height_px.max(0.0))
}

fn current_layer_root_key(
    nodes: &SlotMap<DomNodeSlotKey, DomNode>,
    key: DomNodeSlotKey,
) -> Option<DomNodeSlotKey> {
    let mut current_key = Some(key);
    while let Some(current) = current_key {
        let node = nodes.get(current)?;
        if layer_kind_for_node(node) != crate::types::DomLayerKind::None {
            return Some(current);
        }
        current_key = node.parent_key();
    }
    None
}

fn first_fragment_import_key(
    fragment: &RootDomFragment,
) -> Result<Option<DomNodeSlotKey>, DomError> {
    let fragment_root = fragment.document_fragment_key();
    Ok(fragment
        .node(fragment_root)
        .ok_or(DomError::InvalidNodeKey)?
        .first_child_key_for_store())
}

fn next_fragment_import_key(
    fragment: &RootDomFragment,
    fragment_root: DomNodeSlotKey,
    current_key: DomNodeSlotKey,
) -> Result<Option<DomNodeSlotKey>, DomError> {
    let node = fragment.node(current_key).ok_or(DomError::InvalidNodeKey)?;
    if let Some(child_key) = node.first_child_key_for_store() {
        return Ok(Some(child_key));
    }
    if let Some(sibling_key) = node.next_sibling_key_for_store() {
        return Ok(Some(sibling_key));
    }
    let mut ancestor_key = node.parent_key();
    while let Some(current_ancestor_key) = ancestor_key {
        if current_ancestor_key == fragment_root {
            return Ok(None);
        }
        let ancestor = fragment
            .node(current_ancestor_key)
            .ok_or(DomError::InvalidNodeKey)?;
        if let Some(sibling_key) = ancestor.next_sibling_key_for_store() {
            return Ok(Some(sibling_key));
        }
        ancestor_key = ancestor.parent_key();
    }
    Ok(None)
}

fn parent_key_for_imported_fragment_node(
    fragment: &RootDomFragment,
    fragment_root: DomNodeSlotKey,
    fragment_key: DomNodeSlotKey,
    default_parent_key: DomNodeSlotKey,
    imported_key_by_fragment_key: &HashMap<DomNodeSlotKey, DomNodeSlotKey>,
) -> Result<DomNodeSlotKey, DomError> {
    let fragment_parent_key = fragment
        .node(fragment_key)
        .ok_or(DomError::InvalidNodeKey)?
        .parent_key()
        .ok_or(DomError::InvalidParentTarget)?;
    if fragment_parent_key == fragment_root {
        return Ok(default_parent_key);
    }
    imported_key_by_fragment_key
        .get(&fragment_parent_key)
        .copied()
        .ok_or(DomError::InvalidNodeKey)
}

fn mark_layout_dirty_up_to_layer_root_for_store(
    nodes: &mut SlotMap<DomNodeSlotKey, DomNode>,
    key: DomNodeSlotKey,
) {
    let Some(layer_root_key) = current_layer_root_key(nodes, key) else {
        return;
    };
    let mut current_key = Some(key);
    while let Some(current) = current_key {
        let (was_dirty, parent_key) = {
            let Some(node) = nodes.get_mut(current) else {
                return;
            };
            (node.mark_layout_dirty(), node.parent_key())
        };
        if was_dirty || current == layer_root_key {
            return;
        }
        current_key = parent_key;
    }
}

fn mark_previous_layer_remainder_dirty(
    root_dom: &mut RootDom,
    key: DomNodeSlotKey,
    current_layer_root: DomNodeSlotKey,
    previous_layer_root: DomNodeSlotKey,
    invalidation: AttributeInvalidation,
) {
    let mut current_key = root_dom.nodes.get(key).and_then(DomNode::parent_key);
    let mut left_current_layer = current_layer_root == key;
    while let Some(current) = current_key {
        let Some(node) = root_dom.nodes.get(current) else {
            return;
        };
        let parent_key = node.parent_key();
        if left_current_layer {
            if mark_previous_layer_node_dirty(root_dom, current, invalidation) {
                return;
            }
        } else if current == current_layer_root {
            left_current_layer = true;
        }
        if current == previous_layer_root {
            return;
        }
        current_key = parent_key;
    }
}

fn mark_previous_layer_node_dirty(
    root_dom: &mut RootDom,
    key: DomNodeSlotKey,
    invalidation: AttributeInvalidation,
) -> bool {
    let Some(node) = root_dom.nodes.get_mut(key) else {
        return true;
    };
    let layout_already_dirty = !invalidation.layout() || node.mark_layout_dirty();
    let paint_already_dirty = !invalidation.paint() || node.mark_full_layer_repaint();
    let style_already_dirty = !invalidation.style() || node.mark_style_dirty();
    layout_already_dirty && paint_already_dirty && style_already_dirty
}

fn push_unique_key(keys: &mut Vec<DomNodeSlotKey>, key: DomNodeSlotKey) {
    if !keys.contains(&key) {
        keys.push(key);
    }
}

#[derive(Clone, Copy)]
enum DirtyFlag {
    Layout,
    Style,
}

fn transition_elapsed_s_at(transition: &DomTransition, now_s: f64) -> f64 {
    now_s.min(transition.effective_end_s()) - transition.effective_start_s()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AppendChildPlan {
    current_parent_key: Option<DomNodeSlotKey>,
    current_next_sibling_key: Option<DomNodeSlotKey>,
    previous_last_child: Option<DomNodeSlotKey>,
}

pub(crate) fn append_child_to_parent(
    nodes: &mut SlotMap<DomNodeSlotKey, DomNode>,
    parent_key: DomNodeSlotKey,
    child_key: DomNodeSlotKey,
) -> Result<DomNodeSlotKey, DomError> {
    let append = validate_append_child(nodes, parent_key, child_key)?;
    if append_is_noop(parent_key, append) {
        return Ok(child_key);
    }

    detach_child_for_append(nodes, child_key, append.current_parent_key)?;

    if let Some(last_child_key) = append.previous_last_child {
        let last_child = nodes
            .get_mut(last_child_key)
            .ok_or(DomError::InvalidNodeKey)?;
        set_next_sibling_key(last_child, Some(child_key));
    }

    {
        let parent = nodes.get_mut(parent_key).ok_or(DomError::InvalidNodeKey)?;
        if append.previous_last_child.is_none() {
            set_first_child_key(parent, Some(child_key));
        }
        set_last_child_key(parent, Some(child_key));
    }

    {
        let child = nodes.get_mut(child_key).ok_or(DomError::InvalidNodeKey)?;
        set_parent_key(child, Some(parent_key));
        set_previous_sibling_key(child, append.previous_last_child);
        set_next_sibling_key(child, None);
    }

    if let Some(previous_parent_key) = append.current_parent_key {
        mark_layout_dirty_up_to_layer_root_for_store(nodes, previous_parent_key);
    }
    mark_layout_dirty_up_to_layer_root_for_store(nodes, child_key);
    Ok(child_key)
}

fn validate_append_child(
    nodes: &SlotMap<DomNodeSlotKey, DomNode>,
    parent_key: DomNodeSlotKey,
    child_key: DomNodeSlotKey,
) -> Result<AppendChildPlan, DomError> {
    if parent_key == child_key {
        return Err(DomError::CycleCreationAttempt);
    }

    let parent = nodes.get(parent_key).ok_or(DomError::InvalidNodeKey)?;
    let child = nodes.get(child_key).ok_or(DomError::InvalidNodeKey)?;
    if !parent.is_parent_node() {
        return Err(DomError::InvalidParentTarget);
    }
    if parent.owner_document_key() != child.owner_document_key() {
        return Err(DomError::OwnerDocumentMismatch);
    }
    if append_creates_cycle(nodes, parent_key, child_key) {
        return Err(DomError::CycleCreationAttempt);
    }
    if child_is_partially_linked(child) {
        return Err(DomError::ParentMismatch);
    }

    Ok(AppendChildPlan {
        current_parent_key: child.parent_key(),
        current_next_sibling_key: next_sibling_key(child),
        previous_last_child: append_target_last_child(parent, child, child_key),
    })
}

fn append_is_noop(parent_key: DomNodeSlotKey, append: AppendChildPlan) -> bool {
    append.current_parent_key == Some(parent_key) && append.current_next_sibling_key.is_none()
}

fn append_target_last_child(
    parent: &DomNode,
    child: &DomNode,
    child_key: DomNodeSlotKey,
) -> Option<DomNodeSlotKey> {
    match last_child_key(parent) {
        Some(last_child_key) if last_child_key == child_key => previous_sibling_key(child),
        last_child_key => last_child_key,
    }
}

fn child_is_partially_linked(node: &DomNode) -> bool {
    let linked_edges = usize::from(node.parent_key().is_some())
        + usize::from(previous_sibling_key(node).is_some())
        + usize::from(next_sibling_key(node).is_some());
    linked_edges > 0 && node.parent_key().is_none()
}

fn append_creates_cycle(
    nodes: &SlotMap<DomNodeSlotKey, DomNode>,
    parent_key: DomNodeSlotKey,
    child_key: DomNodeSlotKey,
) -> bool {
    let mut current_key = Some(parent_key);
    while let Some(current) = current_key {
        if current == child_key {
            return true;
        }
        current_key = nodes.get(current).and_then(DomNode::parent_key);
    }
    false
}

fn node_is_connected_to_root(
    nodes: &SlotMap<DomNodeSlotKey, DomNode>,
    root_key: DomNodeSlotKey,
    key: DomNodeSlotKey,
) -> bool {
    let mut current_key = Some(key);
    while let Some(current) = current_key {
        if current == root_key {
            return true;
        }
        current_key = nodes.get(current).and_then(DomNode::parent_key);
    }
    false
}

fn detach_child_for_append(
    nodes: &mut SlotMap<DomNodeSlotKey, DomNode>,
    child_key: DomNodeSlotKey,
    current_parent_key: Option<DomNodeSlotKey>,
) -> Result<(), DomError> {
    let Some(parent_key) = current_parent_key else {
        return Ok(());
    };

    let (previous_key, next_key) = {
        let child = nodes.get(child_key).ok_or(DomError::InvalidNodeKey)?;
        (previous_sibling_key(child), next_sibling_key(child))
    };

    if let Some(previous_key) = previous_key {
        let previous = nodes
            .get_mut(previous_key)
            .ok_or(DomError::InvalidNodeKey)?;
        set_next_sibling_key(previous, next_key);
    }
    if let Some(next_key) = next_key {
        let next = nodes.get_mut(next_key).ok_or(DomError::InvalidNodeKey)?;
        set_previous_sibling_key(next, previous_key);
    }

    {
        let parent = nodes.get_mut(parent_key).ok_or(DomError::InvalidNodeKey)?;
        if first_child_key(parent) == Some(child_key) {
            set_first_child_key(parent, next_key);
        }
        if last_child_key(parent) == Some(child_key) {
            set_last_child_key(parent, previous_key);
        }
    }

    {
        let child = nodes.get_mut(child_key).ok_or(DomError::InvalidNodeKey)?;
        set_parent_key(child, None);
        set_previous_sibling_key(child, None);
        set_next_sibling_key(child, None);
    }

    Ok(())
}

pub(crate) fn detach_node_from_parent(
    nodes: &mut SlotMap<DomNodeSlotKey, DomNode>,
    key: DomNodeSlotKey,
) -> Result<Option<DomNodeSlotKey>, DomError> {
    let (parent_key, previous_key, next_key) = {
        let node = nodes.get(key).ok_or(DomError::InvalidNodeKey)?;
        (
            node.parent_key(),
            previous_sibling_key(node),
            next_sibling_key(node),
        )
    };

    let Some(parent_key) = parent_key else {
        return Ok(None);
    };

    if let Some(previous_key) = previous_key {
        let previous = nodes
            .get_mut(previous_key)
            .ok_or(DomError::InvalidNodeKey)?;
        set_next_sibling_key(previous, next_key);
    }
    if let Some(next_key) = next_key {
        let next = nodes.get_mut(next_key).ok_or(DomError::InvalidNodeKey)?;
        set_previous_sibling_key(next, previous_key);
    }

    {
        let parent = nodes.get_mut(parent_key).ok_or(DomError::InvalidNodeKey)?;
        if first_child_key(parent) == Some(key) {
            set_first_child_key(parent, next_key);
        }
        if last_child_key(parent) == Some(key) {
            set_last_child_key(parent, previous_key);
        }
    }

    {
        let node = nodes.get_mut(key).ok_or(DomError::InvalidNodeKey)?;
        set_parent_key(node, None);
        set_previous_sibling_key(node, None);
        set_next_sibling_key(node, None);
    }

    Ok(Some(parent_key))
}

fn remove_collected_subtree_keys(
    nodes: &mut SlotMap<DomNodeSlotKey, DomNode>,
    removed_keys: &[DomNodeSlotKey],
    active_transitions: &mut std::collections::BTreeSet<DomNodeSlotKey>,
    style_pass_attrs_by_node: &mut HashMap<DomNodeSlotKey, BTreeSet<DomAttribute>>,
    id_lookup: &mut HashMap<String, DomNodeSlotKey>,
) {
    for &removed_key in removed_keys {
        active_transitions.remove(&removed_key);
        style_pass_attrs_by_node.remove(&removed_key);
        if let Some(id) = nodes
            .get(removed_key)
            .and_then(DomNode::as_element)
            .and_then(|element| element.attrs.id.clone())
        {
            id_lookup.remove(id.as_str());
        }
    }
    for &removed_key in removed_keys {
        let removed = nodes.remove(removed_key);
        assert!(removed.is_some(), "validated subtree key should remove");
    }
}

fn first_child_key(node: &DomNode) -> Option<DomNodeSlotKey> {
    node.first_child_key_for_store()
}

fn last_child_key(node: &DomNode) -> Option<DomNodeSlotKey> {
    node.last_child_key_for_store()
}

fn previous_sibling_key(node: &DomNode) -> Option<DomNodeSlotKey> {
    node.previous_sibling_key_for_store()
}

fn next_sibling_key(node: &DomNode) -> Option<DomNodeSlotKey> {
    node.next_sibling_key_for_store()
}

fn set_parent_key(node: &mut DomNode, key: Option<DomNodeSlotKey>) {
    node.set_parent_key_for_store(key);
}

fn set_first_child_key(node: &mut DomNode, key: Option<DomNodeSlotKey>) {
    node.set_first_child_key_for_store(key);
}

fn set_last_child_key(node: &mut DomNode, key: Option<DomNodeSlotKey>) {
    node.set_last_child_key_for_store(key);
}

fn set_previous_sibling_key(node: &mut DomNode, key: Option<DomNodeSlotKey>) {
    node.set_previous_sibling_key_for_store(key);
}

fn set_next_sibling_key(node: &mut DomNode, key: Option<DomNodeSlotKey>) {
    node.set_next_sibling_key_for_store(key);
}

fn mark_node_born_dirty(node: &mut DomNode) {
    node.mark_style_dirty();
    node.mark_layout_dirty();
    node.mark_full_layer_repaint();
}

pub(crate) fn collect_subtree_keys(
    nodes: &SlotMap<DomNodeSlotKey, DomNode>,
    root_key: DomNodeSlotKey,
) -> Result<Vec<DomNodeSlotKey>, DomError> {
    let mut stack = vec![root_key];
    let mut removed_keys = Vec::new();

    while let Some(key) = stack.pop() {
        let node = nodes.get(key).ok_or(DomError::InvalidNodeKey)?;
        let mut child_key = node
            .first_child(|child_key| nodes.get(child_key))
            .map(|(key, _)| key);
        while let Some(current_child_key) = child_key {
            let child = nodes
                .get(current_child_key)
                .ok_or(DomError::InvalidNodeKey)?;
            child_key = child
                .next_sibling(|sibling_key| nodes.get(sibling_key))
                .map(|(key, _)| key);
            stack.push(current_child_key);
        }
        removed_keys.push(key);
    }

    Ok(removed_keys)
}

fn count_unicode_scalars(value: &str) -> usize {
    value.chars().count()
}

fn numeric_keyboard_value_string(
    control: crate::element_controls::ResolvedNumericControl,
    logical_key: &Key,
) -> Option<String> {
    match logical_key {
        Key::ArrowDown => control.keyboard_step_value_string("ArrowDown"),
        Key::ArrowLeft => control.keyboard_step_value_string("ArrowLeft"),
        Key::ArrowUp => control.keyboard_step_value_string("ArrowUp"),
        Key::ArrowRight => control.keyboard_step_value_string("ArrowRight"),
        Key::Home => control.keyboard_step_value_string("Home"),
        Key::End => control.keyboard_step_value_string("End"),
        Key::PageDown => control.keyboard_step_value_string("PageDown"),
        Key::PageUp => control.keyboard_step_value_string("PageUp"),
        _ => None,
    }
}

fn byte_index_for_unicode_scalar_offset(value: &str, scalar_offset: usize) -> usize {
    if scalar_offset == count_unicode_scalars(value) {
        return value.len();
    }
    value
        .char_indices()
        .nth(scalar_offset)
        .map(|(byte_index, _)| byte_index)
        .expect("unicode scalar offset should stay within the current value")
}

fn replaced_unicode_scalar_selection(
    value: &str,
    selection_start: usize,
    selection_end: usize,
    replacement: &str,
) -> (String, usize) {
    let replace_start = byte_index_for_unicode_scalar_offset(value, selection_start);
    let replace_end = byte_index_for_unicode_scalar_offset(value, selection_end);
    let mut next_value =
        String::with_capacity(value.len() - (replace_end - replace_start) + replacement.len());
    next_value.push_str(&value[..replace_start]);
    next_value.push_str(replacement);
    next_value.push_str(&value[replace_end..]);
    (
        next_value,
        selection_start + count_unicode_scalars(replacement),
    )
}

enum DefaultTextBehaviorResult {
    SetSelection {
        selection_start: usize,
        selection_end: usize,
        selection_direction: TextSelectionDirection,
    },
    ReplaceValue {
        next_value: String,
        next_caret_offset: usize,
    },
}

#[allow(clippy::too_many_lines)]
fn default_text_behavior_result(
    value: &str,
    edit_kind: DomElementEditKind,
    selection_start: usize,
    selection_end: usize,
    selection_direction: TextSelectionDirection,
    input: &KeyboardInput,
    shift_key: bool,
) -> Option<DefaultTextBehaviorResult> {
    let scalar_count = count_unicode_scalars(value);
    assert!(
        selection_start <= selection_end,
        "selection start should not exceed selection end",
    );
    assert!(
        selection_start <= scalar_count,
        "selection start should stay within the current value",
    );
    assert!(
        selection_end <= scalar_count,
        "selection end should stay within the current value",
    );
    let selection_is_collapsed = selection_start == selection_end;
    assert!(
        !selection_is_collapsed || selection_direction == TextSelectionDirection::None,
        "collapsed text selection should keep direction none",
    );
    assert!(
        selection_is_collapsed || selection_direction != TextSelectionDirection::None,
        "extended text selection should keep a forward or backward direction",
    );
    let replacement_text = if matches!(edit_kind, DomElementEditKind::MultiLine)
        && matches!(input.logical_key, Key::Enter)
    {
        Some("\n")
    } else if matches!(input.logical_key, Key::Character(_) | Key::Space)
        && let Some(text) = input.text.as_ref()
        && !text.is_empty()
    {
        Some(text.as_str())
    } else {
        None
    };
    let (anchor_offset, focus_offset) = match selection_direction {
        TextSelectionDirection::None => (selection_end, selection_end),
        TextSelectionDirection::Forward => (selection_start, selection_end),
        TextSelectionDirection::Backward => (selection_end, selection_start),
    };
    if let Some(replacement_text) = replacement_text {
        let (next_value, next_caret_offset) = replaced_unicode_scalar_selection(
            value,
            selection_start,
            selection_end,
            replacement_text,
        );
        return Some(DefaultTextBehaviorResult::ReplaceValue {
            next_value,
            next_caret_offset,
        });
    }
    if !selection_is_collapsed && matches!(input.logical_key, Key::Backspace | Key::Delete) {
        let (next_value, next_caret_offset) =
            replaced_unicode_scalar_selection(value, selection_start, selection_end, "");
        return Some(DefaultTextBehaviorResult::ReplaceValue {
            next_value,
            next_caret_offset,
        });
    }
    match input.logical_key {
        Key::Backspace => {
            if focus_offset == 0 {
                return None;
            }
            let delete_start = byte_index_for_unicode_scalar_offset(value, focus_offset - 1);
            let delete_end = byte_index_for_unicode_scalar_offset(value, focus_offset);
            let mut next_value = String::with_capacity(value.len() - (delete_end - delete_start));
            next_value.push_str(&value[..delete_start]);
            next_value.push_str(&value[delete_end..]);
            Some(DefaultTextBehaviorResult::ReplaceValue {
                next_value,
                next_caret_offset: focus_offset - 1,
            })
        }
        Key::Delete => {
            if focus_offset == scalar_count {
                return None;
            }
            let delete_start = byte_index_for_unicode_scalar_offset(value, focus_offset);
            let delete_end = byte_index_for_unicode_scalar_offset(value, focus_offset + 1);
            let mut next_value = String::with_capacity(value.len() - (delete_end - delete_start));
            next_value.push_str(&value[..delete_start]);
            next_value.push_str(&value[delete_end..]);
            Some(DefaultTextBehaviorResult::ReplaceValue {
                next_value,
                next_caret_offset: focus_offset,
            })
        }
        Key::ArrowLeft if shift_key && focus_offset > 0 => {
            let next_focus_offset = focus_offset - 1;
            let next_selection_direction = match next_focus_offset.cmp(&anchor_offset) {
                std::cmp::Ordering::Equal => TextSelectionDirection::None,
                std::cmp::Ordering::Greater => TextSelectionDirection::Forward,
                std::cmp::Ordering::Less => TextSelectionDirection::Backward,
            };
            Some(DefaultTextBehaviorResult::SetSelection {
                selection_start: next_focus_offset.min(anchor_offset),
                selection_end: next_focus_offset.max(anchor_offset),
                selection_direction: next_selection_direction,
            })
        }
        Key::ArrowLeft if !selection_is_collapsed => {
            Some(DefaultTextBehaviorResult::SetSelection {
                selection_start,
                selection_end: selection_start,
                selection_direction: TextSelectionDirection::None,
            })
        }
        Key::ArrowLeft if focus_offset > 0 => Some(DefaultTextBehaviorResult::SetSelection {
            selection_start: focus_offset - 1,
            selection_end: focus_offset - 1,
            selection_direction: TextSelectionDirection::None,
        }),
        Key::ArrowRight if shift_key && focus_offset < scalar_count => {
            let next_focus_offset = focus_offset + 1;
            let next_selection_direction = match next_focus_offset.cmp(&anchor_offset) {
                std::cmp::Ordering::Equal => TextSelectionDirection::None,
                std::cmp::Ordering::Greater => TextSelectionDirection::Forward,
                std::cmp::Ordering::Less => TextSelectionDirection::Backward,
            };
            Some(DefaultTextBehaviorResult::SetSelection {
                selection_start: next_focus_offset.min(anchor_offset),
                selection_end: next_focus_offset.max(anchor_offset),
                selection_direction: next_selection_direction,
            })
        }
        Key::ArrowRight if !selection_is_collapsed => {
            Some(DefaultTextBehaviorResult::SetSelection {
                selection_start: selection_end,
                selection_end,
                selection_direction: TextSelectionDirection::None,
            })
        }
        Key::ArrowRight if focus_offset < scalar_count => {
            Some(DefaultTextBehaviorResult::SetSelection {
                selection_start: focus_offset + 1,
                selection_end: focus_offset + 1,
                selection_direction: TextSelectionDirection::None,
            })
        }
        Key::Home if shift_key && focus_offset > 0 => {
            let next_focus_offset = 0;
            let next_selection_direction = if anchor_offset == 0 {
                TextSelectionDirection::None
            } else {
                TextSelectionDirection::Backward
            };
            Some(DefaultTextBehaviorResult::SetSelection {
                selection_start: 0,
                selection_end: anchor_offset.max(next_focus_offset),
                selection_direction: next_selection_direction,
            })
        }
        Key::Home if !selection_is_collapsed || focus_offset > 0 => {
            Some(DefaultTextBehaviorResult::SetSelection {
                selection_start: 0,
                selection_end: 0,
                selection_direction: TextSelectionDirection::None,
            })
        }
        Key::End if shift_key && focus_offset < scalar_count => {
            let next_focus_offset = scalar_count;
            let next_selection_direction = if anchor_offset == scalar_count {
                TextSelectionDirection::None
            } else {
                TextSelectionDirection::Forward
            };
            Some(DefaultTextBehaviorResult::SetSelection {
                selection_start: anchor_offset.min(next_focus_offset),
                selection_end: next_focus_offset,
                selection_direction: next_selection_direction,
            })
        }
        Key::End if !selection_is_collapsed || focus_offset < scalar_count => {
            Some(DefaultTextBehaviorResult::SetSelection {
                selection_start: scalar_count,
                selection_end: scalar_count,
                selection_direction: TextSelectionDirection::None,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use super::*;
    use bevy::{
        image::Image,
        input::{
            ButtonState,
            keyboard::{Key, KeyCode, KeyboardInput},
        },
        prelude::{Assets, Entity, Transform, Vec2, World},
        sprite::Sprite,
    };

    #[test]
    fn non_bubbling_dispatch_returns_none_when_target_has_no_exact_listener() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let target = root.create_element(DomTag::Button).expect("target create");
        root.append_child(root.document_node().0, target)
            .expect("target attach");

        let dispatched = root
            .dispatch_event_to_target_key(target, focus_event())
            .expect("non-bubbling dispatch should not error");

        assert!(dispatched.is_none());
    }

    #[test]
    fn non_bubbling_dispatch_runs_once_listener_then_short_circuits_after_cleanup() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let target = root.create_element(DomTag::Button).expect("target create");
        root.append_child(root.document_node().0, target)
            .expect("target attach");
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_listener = calls.clone();
        root.add_event_listener(
            target,
            DomEventName::Focus,
            DomEventListener::new(move |_| {
                calls_for_listener.fetch_add(1, Ordering::SeqCst);
            }),
            Some(DomEventListenerOptions {
                capture: false,
                once: true,
                passive: false,
            }),
        )
        .expect("listener add should succeed");

        let first_dispatch = root
            .dispatch_event_to_target_key(target, focus_event())
            .expect("first non-bubbling dispatch should not error");
        let second_dispatch = root
            .dispatch_event_to_target_key(target, focus_event())
            .expect("second non-bubbling dispatch should not error");

        assert!(first_dispatch.is_some());
        assert!(second_dispatch.is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn opening_select_presented_options_keeps_focus_on_the_owner() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let fixture = append_select_with_options(&mut root);
        root.focus(fixture.owner_key).expect("select should focus");

        let handled = root
            .apply_select_keyboard_default_for_owner(
                fixture.owner_key,
                &keyboard_input(ButtonState::Pressed, KeyCode::ArrowDown, Key::ArrowDown),
                true,
            )
            .expect("select presented-options open should succeed");

        assert!(handled);
        assert_eq!(root.focused_element_key(), Some(fixture.owner_key));
        assert!(
            root.presented_options_popup_shell_key(fixture.owner_key)
                .is_some()
        );
    }

    #[test]
    fn opening_input_datalist_presented_options_keeps_focus_on_the_owner() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let fixture = append_input_with_datalist_options(&mut root);
        root.focus(fixture.owner_key).expect("input should focus");

        let handled = root
            .apply_presented_options_text_input_keyboard_navigation_for_owner(
                fixture.owner_key,
                &keyboard_input(ButtonState::Pressed, KeyCode::ArrowDown, Key::ArrowDown),
                true,
            )
            .expect("input presented-options open should succeed");

        assert!(handled);
        assert_eq!(root.focused_element_key(), Some(fixture.owner_key));
        assert!(
            root.presented_options_popup_shell_key(fixture.owner_key)
                .is_some()
        );
    }

    #[test]
    fn moving_presented_options_highlight_does_not_change_owner_focus() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let fixture = append_select_with_options(&mut root);
        root.focus(fixture.owner_key).expect("select should focus");
        root.open_select_popup_for_owner(fixture.owner_key)
            .expect("open should succeed");

        let initial_highlight = root
            .presented_options_highlighted_row_key(fixture.owner_key)
            .expect("open popup should keep one highlighted row");

        let handled = root
            .apply_select_keyboard_default_for_owner(
                fixture.owner_key,
                &keyboard_input(ButtonState::Pressed, KeyCode::ArrowDown, Key::ArrowDown),
                true,
            )
            .expect("highlight move should succeed");

        let popup_row_keys = root.presented_options_popup_row_keys(fixture.owner_key);
        let moved_highlight = root
            .presented_options_highlighted_row_key(fixture.owner_key)
            .expect("open popup should keep one highlighted row");

        assert!(handled);
        assert_eq!(root.focused_element_key(), Some(fixture.owner_key));
        assert_ne!(moved_highlight, initial_highlight);
        assert_eq!(moved_highlight, popup_row_keys[1]);
    }

    #[test]
    fn clicking_presented_option_row_commits_without_moving_focus_off_owner() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let fixture = append_select_with_options(&mut root);
        root.focus(fixture.owner_key).expect("select should focus");
        root.open_select_popup_for_owner(fixture.owner_key)
            .expect("open should succeed");

        let second_row_key = root
            .presented_options_popup_row_keys(fixture.owner_key)
            .get(1)
            .copied()
            .expect("popup should contain a second row");

        let handled = root
            .apply_select_click_default_for_target(second_row_key)
            .expect("row click commit should succeed");

        assert!(handled);
        assert_eq!(root.focused_element_key(), Some(fixture.owner_key));
        assert_eq!(element_value(&root, fixture.owner_key), "beta");
        assert_eq!(popup_state(&root, fixture.owner_key), Some("closing"));
    }

    #[test]
    fn clicking_input_datalist_option_text_commits_without_moving_focus_off_owner() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let fixture = append_input_with_datalist_options(&mut root);
        root.focus(fixture.owner_key).expect("input should focus");
        root.open_presented_options_popup_for_owner(fixture.owner_key)
            .expect("open should succeed");

        let second_row_key = root
            .presented_options_popup_row_keys(fixture.owner_key)
            .get(1)
            .copied()
            .expect("popup should contain a second row");
        let row_text_key = root
            .node(second_row_key)
            .and_then(DomNode::first_child_key_for_store)
            .expect("popup row should contain text");

        let handled = root
            .apply_presented_options_click_default_for_target(row_text_key)
            .expect("row text click commit should succeed");

        assert!(handled);
        assert_eq!(root.focused_element_key(), Some(fixture.owner_key));
        assert_eq!(element_value(&root, fixture.owner_key), "beta");
        assert_eq!(popup_state(&root, fixture.owner_key), Some("closing"));
    }

    #[test]
    fn blurring_presented_options_owner_starts_popup_close_and_clears_focus() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let fixture = append_select_with_options(&mut root);
        root.focus(fixture.owner_key).expect("select should focus");
        root.open_select_popup_for_owner(fixture.owner_key)
            .expect("open should succeed");

        root.blur();

        assert_eq!(root.focused_element_key(), None);
        assert_eq!(root.selection(), DomSelection::None);
        assert_eq!(popup_state(&root, fixture.owner_key), Some("closing"));
    }

    #[test]
    fn disabling_the_focused_control_blurs_it_and_clears_selection() {
        // Remaining focus-repair gaps:
        // - focused textarea and select owners disabled mid-interaction
        // - focused presented-option owners disabled while popup shells are open
        // - parity when focus is lost because the focused node is detached instead of disabled
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let input = root.create_element(DomTag::Input).expect("input create");
        root.append_child(root.document_node().0, input)
            .expect("input attach");
        root.set_attr_str(input, "value", "hello")
            .expect("input value");
        root.focus(input).expect("input should focus");
        root.selection = DomSelection::TextSelected(TextSelection {
            target_element: input,
            selection_start: 1,
            selection_end: 4,
            selection_direction: TextSelectionDirection::Forward,
        });

        root.set_attr(
            input,
            DomAttribute::Disabled,
            Some(DomAttributeValue::Bool(true)),
        )
        .expect("disable should apply");

        assert_eq!(root.focused_element_key(), None);
        assert_eq!(root.selection(), DomSelection::None);
    }

    #[test]
    fn transition_end_queues_closing_presented_options_popup_shell_for_removal() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let fixture = append_select_with_options(&mut root);
        root.focus(fixture.owner_key).expect("select should focus");
        root.open_select_popup_for_owner(fixture.owner_key)
            .expect("open should succeed");
        let shell_key = root
            .presented_options_popup_shell_key(fixture.owner_key)
            .expect("open popup should attach shell");

        root.close_presented_options_popup_for_owner(fixture.owner_key, false)
            .expect("close should succeed");
        root.dispatch_and_record_transition_event(
            shell_key,
            DomEventName::TransitionEnd,
            DomAttribute::Custom("opacity".to_string()),
            0.12,
        );

        assert_eq!(root.pending_layer_remove_roots, vec![shell_key]);
    }

    #[test]
    fn transition_shorthand_writes_longhand_attrs() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let node = root.create_element(DomTag::Div).expect("node create");
        root.append_child(root.document_node().0, node)
            .expect("node attach");

        root.set_attr_str(node, "transition", "left 180ms ease-out 40ms")
            .expect("transition shorthand should set");

        let node_ref = root.node(node).expect("node should exist");
        assert_eq!(
            node_ref.attr(&DomAttribute::TransitionProperty),
            Some(&DomAttributeValue::String("left".to_string()))
        );
        assert_eq!(
            node_ref.attr(&DomAttribute::TransitionDuration),
            Some(&DomAttributeValue::String("0.18s".to_string()))
        );
        assert_eq!(
            node_ref.attr(&DomAttribute::TransitionTimingFunction),
            Some(&DomAttributeValue::String("ease-out".to_string()))
        );
        assert_eq!(
            node_ref.attr(&DomAttribute::TransitionDelay),
            Some(&DomAttributeValue::String("0.04s".to_string()))
        );
    }

    #[test]
    fn transition_shorthand_unset_clears_longhand_attrs() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let node = root.create_element(DomTag::Div).expect("node create");
        root.append_child(root.document_node().0, node)
            .expect("node attach");
        root.set_attr_str(node, "transition", "left 180ms ease-out 40ms")
            .expect("transition shorthand should set");

        root.set_attr_str(node, "transition", "unset")
            .expect("transition shorthand should unset");

        let node_ref = root.node(node).expect("node should exist");
        assert_eq!(node_ref.attr(&DomAttribute::TransitionProperty), None);
        assert_eq!(node_ref.attr(&DomAttribute::TransitionDuration), None);
        assert_eq!(node_ref.attr(&DomAttribute::TransitionTimingFunction), None);
        assert_eq!(node_ref.attr(&DomAttribute::TransitionDelay), None);
    }

    #[test]
    fn transition_longhands_schedule_listed_property_only() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let node = root.create_element(DomTag::Div).expect("node create");
        root.append_child(root.document_node().0, node)
            .expect("node attach");
        root.set_attr_str(node, "left", "0px")
            .expect("initial left");
        root.set_attr_str(node, "top", "0px").expect("initial top");
        root.set_attr_str(node, "transition-property", "left")
            .expect("transition-property");
        root.set_attr_str(node, "transition-duration", "180ms")
            .expect("transition-duration");

        root.set_attr_str(node, "left", "100px")
            .expect("left should transition");
        assert_eq!(
            root.node(node)
                .expect("node should exist")
                .presented_attr(&DomAttribute::Left),
            Some(&DomAttributeValue::Float(0.0))
        );

        root.set_attr_str(node, "top", "100px")
            .expect("top should set without transition");
        assert_eq!(
            root.node(node)
                .expect("node should exist")
                .presented_attr(&DomAttribute::Top),
            Some(&DomAttributeValue::String("100px".to_string()))
        );
    }

    #[test]
    fn invalid_transition_longhand_config_is_inert() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let node = root.create_element(DomTag::Div).expect("node create");
        root.append_child(root.document_node().0, node)
            .expect("node attach");
        root.set_attr_str(node, "left", "0px")
            .expect("initial left");
        root.set_attr_str(node, "transition-property", "left")
            .expect("transition-property");
        root.set_attr_str(node, "transition-duration", "bogus")
            .expect("transition-duration should store authored user data");

        root.set_attr_str(node, "left", "100px")
            .expect("invalid transition config should not abort the attr change");

        assert_eq!(
            root.node(node)
                .expect("node should exist")
                .presented_attr(&DomAttribute::Left),
            Some(&DomAttributeValue::String("100px".to_string()))
        );
    }

    #[test]
    fn acquire_external_layer_target_rejects_detached_root() {
        let mut world = World::new();
        let root = RootDom::new(RootDomContext::new_gpu());
        let document = root.document_node().0;

        let result = root.acquire_external_layer_target(&mut world, document);

        assert_eq!(result, Err(DomError::InvalidParentTarget));
    }

    #[test]
    fn acquire_external_layer_target_rejects_non_layer_node() {
        let (mut world, mut root) = attached_external_target_root();
        let child = root
            .create_element(DomTag::Div)
            .expect("child should insert");
        root.append_child(root.document_node().0, child)
            .expect("child should append");

        let result = root.acquire_external_layer_target(&mut world, child);

        assert_eq!(result, Err(DomError::InvalidNodeKey));
    }

    #[test]
    fn acquire_external_layer_target_rejects_removed_node() {
        let (mut world, mut root) = attached_external_target_root();
        let child = root
            .create_element(DomTag::Div)
            .expect("child should insert");
        root.append_child(root.document_node().0, child)
            .expect("child should append");
        root.remove_node(child).expect("child should remove");

        let result = root.acquire_external_layer_target(&mut world, child);

        assert_eq!(result, Err(DomError::InvalidNodeKey));
    }

    #[test]
    fn acquire_external_layer_target_returns_document_layer_image() {
        let (mut world, root) = attached_external_target_root();
        let document = root.document_node().0;

        let target = root
            .acquire_external_layer_target(&mut world, document)
            .expect("document layer should expose external target");

        assert_eq!(
            Some(target.lease),
            root.layer_lease(document),
            "target should identify the document layer lease"
        );
        assert_eq!(target.image_extent_px.x, 64);
        assert!(
            target.image_extent_px.y >= 1,
            "target image height should be a valid texture extent"
        );
        assert!(
            target
                .origin_offset_px
                .iter()
                .all(|value| value.abs() <= f32::EPSILON),
            "untranslated document layer origin should align with image origin"
        );
    }

    #[test]
    fn acquire_external_layer_target_does_not_reinvalidate_when_policy_is_already_external() {
        let (mut world, root) = attached_external_target_root();
        let document = root.document_node().0;

        root.acquire_external_layer_target(&mut world, document)
            .expect("first target acquire should succeed");
        world
            .resource_mut::<LayerPoolRenderJobQueue>()
            .clear_jobs_for_test();
        root.acquire_external_layer_target(&mut world, document)
            .expect("second target acquire should succeed");

        assert_eq!(
            world
                .resource::<LayerPoolRenderJobQueue>()
                .queued_job_count_for_test(),
            0,
            "already-external layers should not queue repeat invalidation"
        );
    }

    #[test]
    fn external_layer_target_update_clears_dom_paint_dirty() {
        let (mut world, mut root) = attached_external_target_root();
        let document = root.document_node().0;
        root.acquire_external_layer_target(&mut world, document)
            .expect("target acquire should succeed");
        root.mark_paint_dirty_for_attribute_change(document);

        root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("external target update should succeed");

        assert!(
            !root
                .node(document)
                .expect("document node should exist")
                .paint_repaint_state()
                .is_dirty(),
            "normal DOM paint pass should clear dirty state even when external policy suppresses retained updates"
        );
    }

    #[test]
    fn layer_offset_px_returns_composite_offset_for_layer_root() {
        let (mut world, mut root) = attached_external_target_root();
        let child = root.create_element(DomTag::Div).expect("child create");
        root.append_child(root.document_node().0, child)
            .expect("child append");
        root.set_attr_str(child, "position", "fixed")
            .expect("fixed position");
        root.set_attr_str(child, "left", "12px").expect("left");
        root.set_attr_str(child, "top", "8px").expect("top");

        root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("update should succeed");

        assert_eq!(root.layer_offset_px(child), Some([12.0, 8.0]));
        assert_eq!(
            root.layer_offset_px(root.document_node().0),
            Some([0.0, 0.0])
        );
    }

    #[test]
    fn layer_offset_px_accumulates_over_nested_layer_chain() {
        let (mut world, mut root) = attached_external_target_root();
        let parent = root.create_element(DomTag::Div).expect("parent create");
        root.append_child(root.document_node().0, parent)
            .expect("parent append");
        root.set_attr_str(parent, "position", "fixed")
            .expect("fixed position");
        root.set_attr_str(parent, "left", "12px").expect("left");
        root.set_attr_str(parent, "top", "8px").expect("top");
        let child = root.create_element(DomTag::Div).expect("child create");
        root.append_child(parent, child).expect("child append");
        root.set_attr_str(child, "position", "absolute")
            .expect("absolute position");
        root.set_attr_str(child, "left", "3px").expect("left");
        root.set_attr_str(child, "top", "2px").expect("top");

        root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("update should succeed");

        assert_eq!(root.layer_offset_px(child), Some([15.0, 10.0]));
    }

    #[test]
    fn layer_offset_px_returns_none_for_non_layer_content() {
        let (mut world, mut root) = attached_external_target_root();
        let child = root.create_element(DomTag::Div).expect("child create");
        root.append_child(root.document_node().0, child)
            .expect("child append");

        root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("update should succeed");

        assert!(root.layer_offset_px(child).is_none());
    }

    fn attached_external_target_root() -> (World, RootDom) {
        let mut world = World::new();
        world.insert_resource(LayerPool::new(LayerConfig::default()));
        world.insert_resource(UiDomRenderSupport::direct_gpu());
        let parent = world
            .spawn((Sprite {
                custom_size: Some(Vec2::new(64.0, 64.0)),
                ..Default::default()
            },))
            .id();
        let mut root = RootDom::new(RootDomContext::new_gpu());
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("root should attach");
        world
            .resource_mut::<LayerPoolRenderJobQueue>()
            .clear_jobs_for_test();
        (world, root)
    }

    fn focus_event() -> DomEvent {
        DomEvent::new(
            DomEventName::Focus,
            DomEventDetail::Focus(DomFocusEvent::default()),
        )
    }

    struct PresentedOptionsFixture {
        owner_key: DomNodeSlotKey,
        option_keys: [DomNodeSlotKey; 3],
    }

    fn append_select_with_options(root: &mut RootDom) -> PresentedOptionsFixture {
        let owner_key = root.create_element(DomTag::Select).expect("select");
        let first_option = root.create_element(DomTag::Option).expect("first option");
        let second_option = root.create_element(DomTag::Option).expect("second option");
        let third_option = root.create_element(DomTag::Option).expect("third option");
        let first_text = root.create_text_node("alpha").expect("first text");
        let second_text = root.create_text_node("beta").expect("second text");
        let third_text = root.create_text_node("gamma").expect("third text");

        root.append_child(root.document_node().0, owner_key)
            .expect("attach select");
        for (option_key, text_key, value) in [
            (first_option, first_text, "alpha"),
            (second_option, second_text, "beta"),
            (third_option, third_text, "gamma"),
        ] {
            root.append_child(owner_key, option_key)
                .expect("attach option");
            root.append_child(option_key, text_key)
                .expect("attach option text");
            root.set_attr(
                option_key,
                DomAttribute::Value,
                Some(DomAttributeValue::String(value.to_string())),
            )
            .expect("set option value");
        }
        let root_key = root.root;
        set_box(root, root_key, 0.0, 0.0, 240.0, 240.0);
        set_box(root, owner_key, 0.0, 0.0, 120.0, 28.0);

        PresentedOptionsFixture {
            owner_key,
            option_keys: [first_option, second_option, third_option],
        }
    }

    fn append_input_with_datalist_options(root: &mut RootDom) -> PresentedOptionsFixture {
        let owner_key = root.create_element(DomTag::Input).expect("input");
        let list_key = root.create_element(DomTag::Datalist).expect("datalist");
        let first_option = root.create_element(DomTag::Option).expect("first option");
        let second_option = root.create_element(DomTag::Option).expect("second option");
        let third_option = root.create_element(DomTag::Option).expect("third option");
        let first_text = root.create_text_node("alpha").expect("first text");
        let second_text = root.create_text_node("beta").expect("second text");
        let third_text = root.create_text_node("gamma").expect("third text");

        root.append_child(root.document_node().0, owner_key)
            .expect("attach input");
        root.append_child(root.document_node().0, list_key)
            .expect("attach datalist");
        root.set_attr(
            owner_key,
            DomAttribute::List,
            Some(DomAttributeValue::String("pet-list".to_string())),
        )
        .expect("set list attr");
        root.set_attr(
            list_key,
            DomAttribute::Id,
            Some(DomAttributeValue::String("pet-list".to_string())),
        )
        .expect("set datalist id");
        for (option_key, text_key, value) in [
            (first_option, first_text, "alpha"),
            (second_option, second_text, "beta"),
            (third_option, third_text, "gamma"),
        ] {
            root.append_child(list_key, option_key)
                .expect("attach option");
            root.append_child(option_key, text_key)
                .expect("attach option text");
            root.set_attr(
                option_key,
                DomAttribute::Value,
                Some(DomAttributeValue::String(value.to_string())),
            )
            .expect("set option value");
        }
        let root_key = root.root;
        set_box(root, root_key, 0.0, 0.0, 240.0, 240.0);
        set_box(root, owner_key, 0.0, 0.0, 120.0, 28.0);

        PresentedOptionsFixture {
            owner_key,
            option_keys: [first_option, second_option, third_option],
        }
    }

    fn keyboard_input(state: ButtonState, key_code: KeyCode, logical_key: Key) -> KeyboardInput {
        KeyboardInput {
            key_code,
            logical_key,
            state,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        }
    }

    fn element_value(root: &RootDom, key: DomNodeSlotKey) -> String {
        root.nodes
            .get(key)
            .and_then(DomNode::as_element)
            .and_then(|element| element.attr(&DomAttribute::Value))
            .and_then(|value| match value {
                DomAttributeValue::String(value) => Some(value.clone()),
                DomAttributeValue::Bool(_)
                | DomAttributeValue::Integer(_)
                | DomAttributeValue::Float(_)
                | DomAttributeValue::Color(_) => None,
            })
            .expect("element value should stay string")
    }

    fn popup_state(root: &RootDom, owner_key: DomNodeSlotKey) -> Option<&str> {
        root.presented_options_popup_shell_key(owner_key)
            .and_then(|shell_key| root.nodes.get(shell_key))
            .and_then(DomNode::as_element)
            .and_then(|element| element.attr(&DomAttribute::PresentedOptionsState))
            .and_then(|value| match value {
                DomAttributeValue::String(value) => Some(value.as_str()),
                DomAttributeValue::Bool(_)
                | DomAttributeValue::Integer(_)
                | DomAttributeValue::Float(_)
                | DomAttributeValue::Color(_) => None,
            })
    }

    fn set_box(
        root: &mut RootDom,
        key: DomNodeSlotKey,
        left_px: f32,
        top_px: f32,
        width_px: f32,
        height_px: f32,
    ) {
        let node = root.nodes.get_mut(key).expect("node should exist");
        match node {
            DomNode::Document(document) => {
                document.base.box_model.border_box.left_px = left_px;
                document.base.box_model.border_box.top_px = top_px;
                document.base.box_model.border_box.width_px = width_px;
                document.base.box_model.border_box.height_px = height_px;
            }
            DomNode::DocumentFragment(fragment) => {
                fragment.base.box_model.border_box.left_px = left_px;
                fragment.base.box_model.border_box.top_px = top_px;
                fragment.base.box_model.border_box.width_px = width_px;
                fragment.base.box_model.border_box.height_px = height_px;
            }
            DomNode::Element(element) => {
                element.base.box_model.border_box.left_px = left_px;
                element.base.box_model.border_box.top_px = top_px;
                element.base.box_model.border_box.width_px = width_px;
                element.base.box_model.border_box.height_px = height_px;
            }
            DomNode::Text(text) => {
                text.base.box_model.border_box.left_px = left_px;
                text.base.box_model.border_box.top_px = top_px;
                text.base.box_model.border_box.width_px = width_px;
                text.base.box_model.border_box.height_px = height_px;
            }
        }
    }

    fn install_direct_gpu_support(world: &mut World) {
        world.init_resource::<Assets<Image>>();
        world.insert_resource(LayerPool::new(LayerConfig::default()));
        world.init_resource::<LayerPoolRenderJobQueue>();
        world.insert_resource(UiDomRenderSupport::direct_gpu());
    }

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= 0.001,
            "expected {expected}, got {actual}"
        );
    }

    fn append_select(root: &mut RootDom) -> DomNodeSlotKey {
        let select = root.create_element(DomTag::Select).expect("select");
        let first_option = root.create_element(DomTag::Option).expect("first option");
        let second_option = root.create_element(DomTag::Option).expect("second option");
        let first_text = root.create_text_node("alpha").expect("first text");
        let second_text = root.create_text_node("beta").expect("second text");
        root.append_child(root.root, select).expect("append select");
        root.append_child(select, first_option)
            .expect("append first option");
        root.append_child(select, second_option)
            .expect("append second option");
        root.append_child(first_option, first_text)
            .expect("append first text");
        root.append_child(second_option, second_text)
            .expect("append second text");
        root.set_attr_str(first_option, "value", "alpha")
            .expect("set first option value");
        root.set_attr_str(second_option, "value", "beta")
            .expect("set second option value");
        root.set_attr_str(select, "width", "120px").expect("width");
        root.set_attr_str(select, "height", "28px").expect("height");
        select
    }

    fn append_select_to_parent(root: &mut RootDom, parent: DomNodeSlotKey) -> DomNodeSlotKey {
        let select = root.create_element(DomTag::Select).expect("select");
        let first_option = root.create_element(DomTag::Option).expect("first option");
        let second_option = root.create_element(DomTag::Option).expect("second option");
        let first_text = root.create_text_node("alpha").expect("first text");
        let second_text = root.create_text_node("beta").expect("second text");
        root.append_child(parent, select).expect("append select");
        root.append_child(select, first_option)
            .expect("append first option");
        root.append_child(select, second_option)
            .expect("append second option");
        root.append_child(first_option, first_text)
            .expect("append first text");
        root.append_child(second_option, second_text)
            .expect("append second text");
        root.set_attr_str(first_option, "value", "alpha")
            .expect("set first option value");
        root.set_attr_str(second_option, "value", "beta")
            .expect("set second option value");
        root.set_attr_str(select, "width", "120px").expect("width");
        root.set_attr_str(select, "height", "28px").expect("height");
        select
    }

    fn attach_root_with_size(
        root: &mut RootDom,
        world: &mut World,
        width_px: f32,
        height_px: f32,
    ) -> Entity {
        install_direct_gpu_support(world);
        let parent = world
            .spawn((Sprite {
                custom_size: Some(Vec2::new(width_px, height_px)),
                ..Default::default()
            },))
            .id();
        root.attach_to_bevy_parent_entity(world, parent, 0.0)
            .expect("attach root");
        root.update(world, 0.0, DomPointerUpdate::Unchanged)
            .expect("present once");
        parent
    }

    fn pointer_update(x_px: f32, y_px: f32, primary_button_down: bool) -> DomPointerUpdate {
        DomPointerUpdate::Set(DomPointerState {
            x_px,
            y_px,
            primary_button_down,
            scroll_delta_y_px: 0.0,
        })
    }

    fn event_names(events: &[DomEvent]) -> Vec<DomEventName> {
        events.iter().map(|event| event.name).collect()
    }

    fn input_value(root: &RootDom, input: DomNodeSlotKey) -> String {
        let element = root
            .node(input)
            .and_then(DomNode::as_element)
            .expect("input element should exist");
        match element.attrs.attributes.get(&DomAttribute::Value) {
            Some(DomAttributeValue::String(value)) => value.clone(),
            other => panic!("expected input string value, got {other:?}"),
        }
    }

    struct SelectPopupProof {
        shell_entity: Entity,
        shell_box: DomRect,
        row_keys: Vec<DomNodeSlotKey>,
        popup_scroll_key: DomNodeSlotKey,
        popup_scroll_entity: Entity,
    }

    fn select_popup_proof(root: &RootDom, select: DomNodeSlotKey) -> SelectPopupProof {
        let shell_key = root
            .presented_options_popup_shell_key(select)
            .expect("select popup shell should exist");
        let shell_entity = root
            .layer_entity(shell_key)
            .expect("select popup shell entity");
        let shell_box = node_box_from_ref(root.node(shell_key).expect("shell")).border_box;
        let row_keys = root.presented_options_popup_row_keys(select);
        let first_row_key = row_keys
            .first()
            .copied()
            .expect("popup should contain at least one row");
        let popup_scroll_key = root
            .node(first_row_key)
            .and_then(DomNode::parent_key)
            .expect("popup row should have a scroll parent");
        let popup_scroll_entity = root
            .layer_entity(popup_scroll_key)
            .expect("popup scroll layer should have a Bevy entity");
        SelectPopupProof {
            shell_entity,
            shell_box,
            row_keys,
            popup_scroll_key,
            popup_scroll_entity,
        }
    }

    fn assert_popup_surface_order_and_text(
        root: &mut RootDom,
        world: &World,
        parent: Entity,
        proof: &SelectPopupProof,
    ) {
        assert!(
            proof.shell_box.height_px >= 56.0,
            "select popup shell should keep enough height for two rows, got {}",
            proof.shell_box.height_px
        );
        assert_eq!(proof.row_keys.len(), 2);
        let ordered_surface_entities = world
            .resource::<LayerPoolRenderJobQueue>()
            .ordered_surface_entities_for_test(parent.into())
            .expect("root update should publish retained surface order");
        assert_eq!(
            ordered_surface_entities
                .iter()
                .position(|entity| *entity == proof.shell_entity.into()),
            Some(1),
            "popup shell should be raised above the document layer"
        );
        assert_eq!(
            ordered_surface_entities
                .iter()
                .position(|entity| *entity == proof.popup_scroll_entity.into()),
            Some(2),
            "popup scroll child should render after its raised shell"
        );
        assert_popup_text_runs_inside_scroll_surface(root, proof.popup_scroll_key);
    }

    fn assert_popup_text_runs_inside_scroll_surface(
        root: &mut RootDom,
        popup_scroll_key: DomNodeSlotKey,
    ) {
        let mut layer_pool = LayerPool::new(LayerConfig::default());
        let popup_scroll_plan = build_layer_paint_plan(
            root,
            &mut layer_pool,
            popup_scroll_key,
            (120.0, 56.0),
            &InheritedStyleContext::default(),
        )
        .expect("popup scroll paint plan");
        assert_eq!(popup_scroll_plan.text_runs.len(), 2);
        assert_close(popup_scroll_plan.text_runs[0].position_px[0], 8.0);
        assert_close(popup_scroll_plan.text_runs[0].position_px[1], 5.0);
        assert_close(popup_scroll_plan.text_runs[0].size_px[1], 18.0);
        assert_close(popup_scroll_plan.text_runs[0].line_height_px, 18.0);
        assert_close(popup_scroll_plan.text_runs[1].position_px[0], 8.0);
        assert_close(popup_scroll_plan.text_runs[1].position_px[1], 33.0);
        assert_close(popup_scroll_plan.text_runs[1].size_px[1], 18.0);
        assert_close(popup_scroll_plan.text_runs[1].line_height_px, 18.0);
        for text_run in &popup_scroll_plan.text_runs {
            assert!(
                text_run.size_px[0] > 0.0 && text_run.size_px[1] > 0.0,
                "popup option text run should paint with non-zero size: {text_run:?}"
            );
            assert!(
                text_run.position_px[0] >= 0.0
                    && text_run.position_px[0] < 120.0
                    && text_run.position_px[1] >= 0.0
                    && text_run.position_px[1] < 56.0,
                "popup option text run should paint inside the scroll surface: {text_run:?}"
            );
        }
    }

    fn assert_second_popup_row_mouse_click_commits(
        root: &mut RootDom,
        world: &mut World,
        parent: Entity,
        select: DomNodeSlotKey,
        proof: &SelectPopupProof,
    ) {
        let second_row_point = PointPx {
            x_px: 48.0,
            y_px: 130.0,
        };
        let second_row_layer_hit =
            topmost_attached_layer_hit_from_root_point(root, world, second_row_point)
                .expect("popup row layer hit should resolve");
        let second_row_subtree_hit = second_row_layer_hit.and_then(|layer_hit| {
            hit_test_from_dom_subtree_cluster_local(
                root,
                layer_hit.node_key,
                layer_hit.point,
                layer_hit.effective_rect,
            )
            .expect("popup row subtree hit should resolve")
        });
        let root_box = node_box_from_ref(root.node(root.root).expect("root")).border_box;
        assert_eq!(
            root.point_event_target_key_from_attached_point(world, parent, second_row_point)
                .expect("popup row hit target should resolve"),
            Some(proof.row_keys[1]),
            "pointer hit target should be the popup row under the cursor; root box: {root_box:?}; shell box: {:?}; layer hit: {second_row_layer_hit:?}; subtree hit: {second_row_subtree_hit:?}",
            proof.shell_box,
        );
        let down_events = root
            .update(
                world,
                0.0,
                pointer_update(second_row_point.x_px, second_row_point.y_px, true),
            )
            .expect("second row down");
        let target_after_down = root
            .point_event_target_key_from_attached_point(world, parent, second_row_point)
            .expect("target after row down should resolve");
        assert_eq!(
            target_after_down,
            Some(proof.row_keys[1]),
            "popup row should remain the hit target after pointer down; down events: {down_events:?}",
        );
        let up_events = root
            .update(
                world,
                0.0,
                pointer_update(second_row_point.x_px, second_row_point.y_px, false),
            )
            .expect("second row up");
        assert_eq!(
            input_value(root, select),
            "beta",
            "mouse click on a raised popup row should commit the row value; down events: {down_events:?}; up events: {up_events:?}",
        );
    }

    #[test]
    fn pointer_click_opens_select_presented_options_after_click_activation() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let select = append_select(&mut root);
        root.set_attr_str(select, "margin-left", "12px")
            .expect("select margin left");
        root.set_attr_str(select, "margin-top", "40px")
            .expect("select margin top");
        root.set_border_box_for_test(root.root, 0.0, 0.0, 180.0, 180.0);
        let mut world = World::new();
        attach_root_with_size(&mut root, &mut world, 180.0, 180.0);
        root.reset_metrics();
        let point = PointPx {
            x_px: 17.0,
            y_px: 45.0,
        };

        let _ = root
            .update(
                &mut world,
                0.0,
                pointer_update(point.x_px, point.y_px, true),
            )
            .expect("down");
        let up_events = root
            .update(
                &mut world,
                0.0,
                pointer_update(point.x_px, point.y_px, false),
            )
            .expect("up");

        assert_eq!(
            event_names(&up_events),
            vec![DomEventName::PointerUp, DomEventName::Click]
        );
        let shell_key = root
            .presented_options_popup_shell_key(select)
            .expect("select popup shell should exist");
        assert!(
            root.layer_entity(shell_key).is_some(),
            "select popup shell should materialize as a layer"
        );
        assert!(
            root.raised_layer_roots().contains(&shell_key),
            "select popup shell should use raised layer ordering"
        );
        let shell_entity = root
            .layer_entity(shell_key)
            .expect("select popup shell entity");
        let shell_transform = world
            .get::<Transform>(shell_entity)
            .expect("select popup shell transform");
        assert_close(shell_transform.translation.x, 12.0);
        assert_close(shell_transform.translation.y, -68.0);
    }

    #[test]
    fn pointer_click_anchors_select_popup_to_nested_owner_box() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let wrapper = root.create_element(DomTag::Div).expect("wrapper");
        root.append_child(root.root, wrapper)
            .expect("append wrapper");
        root.set_attr_str(wrapper, "display", "block")
            .expect("wrapper display");
        root.set_attr_str(wrapper, "margin-left", "31px")
            .expect("wrapper margin left");
        root.set_attr_str(wrapper, "margin-top", "53px")
            .expect("wrapper margin top");
        root.set_attr_str(wrapper, "padding-left", "9px")
            .expect("wrapper padding left");
        root.set_attr_str(wrapper, "padding-top", "7px")
            .expect("wrapper padding top");
        root.set_attr_str(root.root, "width", "240px")
            .expect("root width");
        root.set_attr_str(root.root, "height", "240px")
            .expect("root height");
        let select = append_select_to_parent(&mut root, wrapper);
        let mut world = World::new();
        let parent = attach_root_with_size(&mut root, &mut world, 240.0, 240.0);
        root.reset_metrics();
        let point = PointPx {
            x_px: 45.0,
            y_px: 65.0,
        };

        let _ = root
            .update(
                &mut world,
                0.0,
                pointer_update(point.x_px, point.y_px, true),
            )
            .expect("down");
        let _ = root
            .update(
                &mut world,
                0.0,
                pointer_update(point.x_px, point.y_px, false),
            )
            .expect("up");

        let proof = select_popup_proof(&root, select);
        let shell_transform = world
            .get::<Transform>(proof.shell_entity)
            .expect("select popup shell transform");
        assert_popup_surface_order_and_text(&mut root, &world, parent, &proof);
        assert_close(shell_transform.translation.x, 40.0);
        assert_close(shell_transform.translation.y, -88.0);
        assert_second_popup_row_mouse_click_commits(&mut root, &mut world, parent, select, &proof);
    }
}
