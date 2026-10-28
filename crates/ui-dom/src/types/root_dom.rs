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
//! - `RootDom` keeps the default-behavior facade seams (focus, blur, pointer,
//!   keyboard); the per-kind default-behavior rows and the generic activation
//!   lifecycle live in `types/element_behaviors/` (PR 5, E6-a), and the seams
//!   resolve a control kind, look up the behavior table, and invoke the row's
//!   hook. The presented-options popup machinery lives in
//!   `types/element_behaviors/popup.rs`/`popup_shell.rs` (E6-b); what remains
//!   here is only the popup-adjacent transition-end hook (the layer-removal
//!   queue push) and the seams' invocations.
//! - attachment state here describes Bevy presentation lifecycle only
//! - node-level data must not absorb root-owned indexing or lifecycle policy
//!
//! Advice and code philosophy:
//!
//! - keep root APIs DOM-shaped and unsurprising
//! - push one-off helpers downward; keep ownership decisions here explicit
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use bevy::{
    input::keyboard::KeyboardInput,
    prelude::{Entity, World},
};

use crate::domains::composite::apply_vertical_scroll_from_point;
use crate::domains::layer_hit::any_layer_hit_from_root_point;
use crate::domains::layout::{layer_kind_for_node, layer_offset_from_dom_climb, node_box_from_ref};
use crate::domains::paint::{
    mark_full_repaint_up_to_layout_root, mark_node_dirty_up_to_layout_root,
};
use crate::domains::style::computed::{
    ComputedGroups, GroupDirtyClassification, ResolvedTransitionLonghands, TextFamilyList,
    for_each_moved_field,
};
use crate::domains::style::{
    ComputedStyleRef, DeclBlock, DeclDropReport, DeclStore, Declaration, GridAreasKey,
    GridTemplateAreas, GridTrackList, GridTrackListKey, ParsedDeclaration, PresentationProperty,
    StyleMut, TextFamilyListKey, TextInterner, TextKey, decl_value_to_css_string,
    decl_value_to_dom_value, name_is_presentation, parse_declaration_block,
};
use crate::domains::visitor::event_target::{
    DomPointEventTarget, dispatch_event_from_point_and_finish, topmost_event_target_from_point,
};
use crate::domains::visitor::pseudo_state::{
    active_pointer_target_from_attached_point, active_pointer_target_from_point,
    sync_hover_from_attached_pointer,
};
use crate::domains::visitor::winners_fold::{CollectedWinners, collect_winners};
use crate::domains::{ExternalLayerTarget, LayerLease, LayerPool, LayerPoolPolicy};
use crate::element_controls::{
    ResolvedElementControl, resolved_element_control, resolved_text_edit_kind,
};
use crate::types::dirty_channel::{
    DirtyFlag, attribute_invalidation_from_classification, mark_dirty_up_to_layer_root,
    mark_newly_attached_subtree_dirty,
};
use crate::types::dom_event::{
    DomEvent, DomEventDetail, DomEventListener, DomEventListenerOptions, DomEventName,
    DomFocusEvent, DomModifierState, DomMouseButton, DomPointerEvent, DomPointerType,
    DomTransitionEvent, DomValueChangeEvent, DomWheelDeltaMode, DomWheelEvent,
};
use crate::types::dom_event_listener::{
    add_event_listener_to_node, remove_event_listener_from_node,
};
use crate::types::dom_node::{
    AttributeInvalidation, DomDocumentNode, DomIntrinsic, DomNode, DomNodeSlotKey, LayerOrder,
    attribute_invalidation,
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
use crate::types::dom_transition::{DomTransitionConfig, transition_spec_for_longhands};
use crate::types::element_behaviors::{
    ActiveRangeDragSession, DomEventDispatchOutcome, PRESENTED_OPTIONS_STATE_CLOSING,
    PressedPointerClickTarget, apply_default_range_drag_for_point,
    apply_default_text_behavior_and_dispatch_input_from_keyboard_input as apply_default_text_behavior,
    behavior_row, count_unicode_scalars, dispatch_click_and_apply_after_click,
    ensure_text_selection_for_target, finish_range_drag_change_if_dirty,
    presented_options_popup_state, pressed_pointer_click_target_from_raw_target, resolve_kind,
};
use crate::types::event_pointer::{
    event_point, hover_pointer_event, pointer_moved, pointer_pressed, pointer_released,
};
use crate::types::event_propagation::propagate;
use crate::types::id_index::{
    assert_id_is_available, assert_id_is_available_for, next_id_from_attr, replace_indexed_id,
};
use crate::types::primitives::PointPx;
use crate::types::purge::{clear_if_in, clear_if_in_by, remove_each_in, retain_not_in_vec};
use crate::types::root_dom_context::RootDomContext;
use crate::types::root_dom_fragment::RootDomFragment;
use crate::types::root_dom_metrics::RootDomMetricTally;
use crate::types::root_dom_nodes::RootDomNodes;
use crate::types::root_dom_nodes::{
    RootDomNodesMetricHistoryIter, RootDomNodesMetricName, RootDomNodesMetrics,
};
use crate::types::splice::{SpliceJournal, splice_fragment_children_into_store};
use crate::types::transition_projection::{
    decl_value_for_groups, transition_elapsed_s_at, validate_transition_payload,
};
use crate::types::tree_links::{
    append_child_to_parent, collect_subtree_keys, current_layer_root_key, detach_node_from_parent,
    node_is_connected_to_root, push_unique_key,
};
use crate::types::{
    DomAttribute, DomAttributeValue, DomCanonicalSnapshot, DomCanonicalStyleSnapshot, DomError,
    DomEventListenerOwner, LayoutScratch, parse_authored_value_input,
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
use crate::domains::style::Keyword;
#[cfg(test)]
use crate::domains::style::StyleRuleOrigin;
#[cfg(test)]
use crate::domains::style::computed::BoxLength;
#[cfg(test)]
use crate::domains::visitor::inherited_style::InheritedStyleContext;
#[cfg(test)]
use crate::domains::visitor::style_recompute::recompute_dirty_style_subtree_for_layer_root;
#[cfg(test)]
use crate::types::element_behaviors::{
    presented_options_popup_row_keys, presented_options_popup_shell_key,
};
#[cfg(test)]
use crate::types::parse_css_color_rgba;
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
    pub(crate) inline_decl_store: DeclStore,
    pseudo_state: RootDomPseudoState,
    selection: DomSelection,
    transitions: DomTransitionStore,
    /// The document's computed-text interner (spine 3.3a): computed grid
    /// line-name keys resolve here. (Computed `font-family` values moved to the
    /// inline store's family-list arena in text seam S2.) Written by the style
    /// pass's projection during recompute; read by grid placement and the
    /// computed read surface. `TextKey` values are document-local, exactly like
    /// `DomNodeSlotKey`.
    pub(crate) computed_texts: TextInterner,
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
    /// Per-document, per-update flex layout scratch: the item/order/line/
    /// resolution lanes, cleared at the start of each update's layout phase
    /// (see `LayoutScratch::reset_for_update`). Capacity is kept at the
    /// high-water mark so the flex solve reaches steady state with zero
    /// allocation. `pub(crate)` because the flex solve (in `domains::layout`)
    /// reads and writes the lanes during its measurement fold and solve.
    pub(crate) layout_scratch: LayoutScratch,
    /// Reused work buffer for the dirty-tree gate scan
    /// (`walker::tree_contains_dirty_node`). Cleared at the start of each scan;
    /// capacity is kept at the high-water mark so a warmed document's layout/
    /// paint gate scan allocates nothing (the crate's zero-allocation
    /// steady-state rule). `pub(crate)` because the scan is a
    /// `domains::layout` walk.
    pub(crate) dirty_scan_stack: Vec<DomNodeSlotKey>,
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
            inline_decl_store: DeclStore::new(),
            pseudo_state: RootDomPseudoState::default(),
            selection: DomSelection::None,
            transitions: DomTransitionStore::default(),
            computed_texts: TextInterner::default(),
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
            layout_scratch: LayoutScratch::default(),
            dirty_scan_stack: Vec::new(),
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
        // Update-scoped layout scratch reset (FLEX_STEP1_EXECUTION.md §2
        // scratch-ownership amendment): the flex lanes are per-document,
        // per-update state. This runs exactly once per `update(...)` call,
        // before any style or layout pass, so every layout phase in this
        // update writes into a fresh range at the retained high-water mark.
        self.layout_scratch.reset_for_update();
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
        apply_default_range_drag_for_point(self, point)?;
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
        ensure_text_selection_for_target(self, key);
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
            mark_dirty_up_to_layer_root(&mut self.nodes, previous_parent_key, DirtyFlag::Layout);
            mark_full_repaint_up_to_layout_root(self, previous_parent_key);
        } else {
            mark_newly_attached_subtree_dirty(&mut self.nodes, appended_key);
            queue_newly_connected_layer_roots_in_subtree(self, appended_key);
        }
        mark_dirty_up_to_layer_root(&mut self.nodes, parent_key, DirtyFlag::Layout);
        // Style matching depends on tree position (type selectors, class
        // rules, inheritance), so an insertion must arm the enclosing layer
        // root's style phase just like attribute changes and hover do;
        // otherwise the new subtree is laid out and painted with unstyled
        // attrs until some later trigger marks the layer root style-dirty.
        mark_dirty_up_to_layer_root(&mut self.nodes, parent_key, DirtyFlag::Style);
        mark_full_repaint_up_to_layout_root(self, parent_key);
        if self
            .node(appended_key)
            .is_some_and(|node| layer_kind_for_node(node) != crate::types::DomLayerKind::None)
        {
            mark_dirty_up_to_layer_root(&mut self.nodes, appended_key, DirtyFlag::Layout);
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
        let mut journal = SpliceJournal {
            born_dirty: true,
            ..SpliceJournal::default()
        };
        let imported_keys = splice_fragment_children_into_store(
            self.root,
            &mut self.nodes,
            &mut self.id_lookup,
            parent_key,
            &fragment,
            &mut journal,
        )?;
        self.emit_splice_journal_metrics(&journal);
        // Imported nodes are born dirty on their own flags, but the style
        // and paint gates read the enclosing layer root, so the parent chain
        // must propagate all three flags for the first update to restyle,
        // relayout, and repaint the imported subtree.
        mark_dirty_up_to_layer_root(&mut self.nodes, parent_key, DirtyFlag::Layout);
        mark_dirty_up_to_layer_root(&mut self.nodes, parent_key, DirtyFlag::Style);
        mark_full_repaint_up_to_layout_root(self, parent_key);
        for imported_key in imported_keys {
            self.queue_layer_append_root(imported_key);
        }
        drop(fragment);
        Ok(())
    }

    /// Translates a filled `SpliceJournal` into root metric increments.
    ///
    /// The splice walk records what it did into the journal; metrics are
    /// emitted here, at the root mutation boundary, because metrics are never
    /// passed down to lower-level functions (`root_dom_metrics.rs`).
    fn emit_splice_journal_metrics(&mut self, journal: &SpliceJournal) {
        for _ in 0..journal.text_intrinsic_measurements {
            self.increment_metric(RootDomMetricName::TextIntrinsicMeasurement);
        }
        for _ in 0..journal.img_intrinsic_recomputations {
            self.increment_metric(RootDomMetricName::ImgIntrinsicRecomputation);
        }
        for _ in 0..journal.img_intrinsic_state_updates {
            self.increment_metric(RootDomMetricName::ImgIntrinsicStateUpdate);
        }
        for _ in 0..journal.node_slot_allocations {
            self.increment_metric(RootDomMetricName::NodeSlotAllocation);
        }
        for _ in 0..journal.text_storage_allocations {
            self.increment_metric(RootDomMetricName::TextStorageAllocation);
        }
    }

    /// Inserts one node into the root-owned slot store and updates the element
    /// id lookup if that node is an element carrying an `id` attribute.
    fn insert_node(&mut self, node: DomNode) -> Result<DomNodeSlotKey, DomError> {
        assert_id_is_available(
            &self.id_lookup,
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

    // Extraction note (PR 5, E3): the hovered-layers purge stays bespoke
    // rather than joining the purge table. Clearing a hovered layer removes
    // its chain entries AND clears each surviving ancestor's Hover pseudo
    // state, which marks that ancestor style-dirty. The purge and the dirty
    // mark are not separable without changing behavior. Back-off doctrine
    // verdict (b): purge + journal.
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

    /// Runs every purge-only dependent in the static dependents table, in the
    /// table's order (which is today's exact cleanup order).
    fn purge_dependents(&mut self, removed_keys: &[DomNodeSlotKey]) {
        for &(_, purge) in PURGE_DEPENDENTS {
            purge(self, removed_keys);
        }
    }

    fn purge_active_element(&mut self, removed_keys: &[DomNodeSlotKey]) {
        clear_if_in(&mut self.pseudo_state.active_element_key, removed_keys);
    }

    // Extraction note (PR 5, E3): the selection purge is bespoke (not a
    // `clear_if_in` leaf) because it is a two-trigger clear with a latent
    // gap: it clears only `TextSelected` whose target is removed, never
    // `DocumentSelected` (whose anchor/focus nodes would be removed). That
    // gap is unreachable today (`DocumentSelected` is never constructed), so
    // the behavior is preserved as-is rather than silently "fixed".
    fn purge_selection(&mut self, removed_keys: &[DomNodeSlotKey]) {
        if matches!(
            self.selection,
            DomSelection::TextSelected(selection) if removed_keys.contains(&selection.target_element)
        ) {
            self.selection = DomSelection::None;
        }
    }

    fn purge_pressed_pointer_target(&mut self, removed_keys: &[DomNodeSlotKey]) {
        clear_if_in_by(
            &mut self.pressed_pointer_click_target,
            removed_keys,
            |target| target.target_key,
        );
    }

    fn purge_active_range_drag_session(&mut self, removed_keys: &[DomNodeSlotKey]) {
        clear_if_in_by(
            &mut self.active_range_drag_session,
            removed_keys,
            |session| session.target_key,
        );
    }

    fn purge_scroll_offsets(&mut self, removed_keys: &[DomNodeSlotKey]) {
        remove_each_in(&mut self.vertical_scroll_offset_by_node, removed_keys);
    }

    fn purge_layer_append_roots(&mut self, removed_keys: &[DomNodeSlotKey]) {
        retain_not_in_vec(&mut self.pending_layer_append_roots, removed_keys);
    }

    fn purge_layer_remove_roots(&mut self, removed_keys: &[DomNodeSlotKey]) {
        retain_not_in_vec(&mut self.pending_layer_remove_roots, removed_keys);
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
        self.clear_hover_for_removed_keys(&removed_keys);
        self.purge_dependents(&removed_keys);

        let previous_parent_key = detach_node_from_parent(&mut self.nodes, key)?;

        if let Some(parent_key) = previous_parent_key {
            mark_dirty_up_to_layer_root(&mut self.nodes, parent_key, DirtyFlag::Layout);
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
            &mut self.inline_decl_store,
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
        // D5 enforcement + style-attribute routing (spine 3.3g-2a), BEFORE any
        // normalization/expansion: the `style` attribute itself routes into the
        // inline declaration store (never stored as an attr); a `Custom`
        // presentation name is a typed error pointing at `style_mut`; other
        // `Custom` names are content data (the documented escape hatch) and
        // content attributes continue unchanged.
        if attribute == DomAttribute::Style {
            return self.route_style_attribute(key, value);
        }
        if let DomAttribute::Custom(name) = &attribute
            && name_is_presentation(name)
        {
            return Err(DomError::PresentationRequiresStyleMut);
        }

        let is_id_attribute = attribute == DomAttribute::Id;
        let previous_id = self.current_element_id(key);
        let next_id = next_id_from_attr(&attribute, value.as_ref())?;
        assert_id_is_available_for(&self.id_lookup, key, next_id.as_deref())?;
        let attribute_for_refresh = attribute.clone();
        let current_node = self.nodes.get(key).ok_or(DomError::InvalidNodeKey)?;
        let previous_layer_kind = layer_kind_for_node(current_node);
        if current_node.attr(&attribute_for_refresh) == value.as_ref() {
            return Ok(());
        }
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
        let live_text_selection = match self.selection {
            DomSelection::TextSelected(selection) if selection.target_element == key => {
                Some(selection)
            }
            _ => None,
        };
        {
            let node = self.nodes.get_mut(key).ok_or(DomError::InvalidNodeKey)?;
            node.set_attr(attribute, value)?;
            let _intrinsic_changed =
                node.after_attr_write_intrinsic_update(prepared_intrinsic_after_attr_write);
        }
        let next_layer_kind = {
            let node = self.nodes.get(key).ok_or(DomError::InvalidNodeKey)?;
            layer_kind_for_node(node)
        };
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
            replace_indexed_id(&mut self.id_lookup, key, previous_id, next_id);
        }
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

    /// Routes the `style` attribute into the inline declaration store (spine
    /// 3.3g-2a). The style attribute is never stored as an attr: `None` clears
    /// the node's inline block (removal semantics), a `String` parses
    /// drop-and-continue (CSS 2.1 §4.1.7) and replaces the block wholesale
    /// (cssText semantics). Block replacement is one mutating write: it marks
    /// style-dirty and counts `StyleDeclarationUpsert` via
    /// `replace_inline_decl_block` (only when the stored declarations actually
    /// changed), plus `StyleDeclarationDropped` per dropped declaration —
    /// counted whenever the list dropped any malformed declaration, even when
    /// the surviving block is unchanged. Any other value kind is a typed error.
    fn route_style_attribute(
        &mut self,
        key: DomNodeSlotKey,
        value: Option<DomAttributeValue>,
    ) -> Result<(), DomError> {
        let node = self.nodes.get(key).ok_or(DomError::InvalidNodeKey)?;
        // Element-only target. Keep this check in sync with
        // `StyleMut::validate_target` (src/domains/style/style_mut.rs): the
        // winners fold processes elements only, so a write on `Text`,
        // `Document`, or `DocumentFragment` would be silently inert and is
        // rejected here instead.
        if !matches!(node, DomNode::Element(_)) {
            return Err(DomError::InvalidAttributeTarget);
        }
        let (block, report) = match value {
            None => (DeclBlock::new(), DeclDropReport::default()),
            Some(DomAttributeValue::String(text)) => {
                parse_declaration_block(&text, &mut self.inline_decl_store)
            }
            Some(_) => return Err(DomError::InvalidAttributeValue),
        };
        self.replace_inline_decl_block(key, block);
        // Drops are a parsing outcome, not a mutation outcome (AGENTS.md:
        // "every drop is counted by metric ... never a graceful silent
        // fallback"): count them whenever the declaration list dropped any
        // malformed declaration, independent of whether the surviving block
        // differed from the previous one.
        for _ in 0..report.dropped {
            self.increment_metric(RootDomMetricName::StyleDeclarationDropped);
        }
        Ok(())
    }

    /// Sets one node attribute from authored string name/value text.
    ///
    /// Use `"unset"` as the authored removal value.
    ///
    /// The name-based D5 gate runs FIRST: a presentation name (a longhand or
    /// shorthand property name) is a typed error pointing at `style_mut`; the
    /// `style` name routes into the inline declaration store; anything else
    /// parses into the CONTENT vocabulary (with `Custom` fallthrough for
    /// non-presentation unknowns).
    pub fn set_attr_str(
        &mut self,
        key: DomNodeSlotKey,
        attribute: &str,
        value: &str,
    ) -> Result<(), DomError> {
        if name_is_presentation(attribute) {
            return Err(DomError::PresentationRequiresStyleMut);
        }
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
        let family = inherited_context
            .effective_font_family()
            .and_then(|family_key| self.resolve_inline_family_list(family_key))
            .cloned()
            .unwrap_or_default();
        let text_style = crate::domains::text_layout::ResolvedTextStyle::new(
            family,
            inherited_context.effective_font_weight(),
            inherited_context.effective_font_style(),
            inherited_context.effective_text_direction(),
        );
        let next_intrinsic =
            crate::domains::paint::measure_text_intrinsic_with_font_size_line_height_white_space_and_wrap_policy(
                text,
                inherited_context.effective_font_size_px(),
                inherited_context.effective_line_height_px(),
                inherited_context.effective_white_space(),
                inherited_context.effective_overflow_wrap(),
                inherited_context.effective_word_break(),
                text_style,
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

    /// Returns the node's computed-presentation view (typed groups; each
    /// accessor merges the node's active-transition overlay). `None` for a
    /// missing or already-removed node key — a bad key is boundary data and
    /// must not fabricate a group.
    #[must_use]
    pub fn computed_style(&self, node: DomNodeSlotKey) -> Option<ComputedStyleRef<'_>> {
        self.nodes
            .contains_key(node)
            .then(|| ComputedStyleRef::new(self, node))
    }

    /// Resolves one document-local computed-text key back to its string.
    /// `None` for an out-of-range key (never a panic).
    #[must_use]
    pub(crate) fn resolve_computed_text(&self, key: TextKey) -> Option<&str> {
        self.computed_texts.resolve(key)
    }

    /// Debug-priced on-demand style query (PLAN.md open question 9: the
    /// `getComputedStyle`-freedom successor to the old attr-map fold). Re-folds
    /// the node's winners from CURRENT sources (behavior base + active
    /// pseudo-states + matched rules under one runtime lock + the node's inline
    /// block), seeds from the parent's retained merged groups, and projects each
    /// winning declaration to `(property, DomAttributeValue)` (text resolved via
    /// the fold's scratch interner; lengths/keywords/numbers formatted as
    /// strings through the surviving value formatter). This is NOT on the live
    /// recompute path: the retained pass resolves groups through the typed
    /// winners fold and never calls this.
    pub fn compute_style_map(
        &mut self,
        node_key: DomNodeSlotKey,
    ) -> Result<BTreeMap<PresentationProperty, DomAttributeValue>, DomError> {
        let CollectedWinners {
            winners, scratch, ..
        } = collect_winners(self, node_key)?;
        let mut map = BTreeMap::new();
        for property in PresentationProperty::ALL {
            let Some((value, _precedence)) = winners[property.index()] else {
                continue;
            };
            map.insert(
                property,
                decl_value_to_dom_value(
                    &value,
                    |key| scratch.resolve(key),
                    |key| self.resolve_inline_track_list(key),
                    |key| self.resolve_inline_grid_areas(key),
                    |key| self.resolve_inline_family_list(key),
                    |key| self.resolve_inline_text(key),
                ),
            );
        }
        Ok(map)
    }

    /// The canonical public style list for one node: the node's inline
    /// declarations in block order, projected to the public wire shape
    /// `(property-name, value)` (spine 3.3g-2a). `Text` values resolve through
    /// the inline store's interner; lengths/keywords/numbers/colors use the
    /// same formatter `compute_style_map` shares. An important declaration
    /// serializes EVERY value kind to its CSS string form with the
    /// CSS-compatible ` !important` suffix (a typed `Color` has no importance
    /// channel), so the list replays authored style inputs through the
    /// style-attribute route with importance preserved for every value kind.
    /// A missing node yields an empty list (it has no inline declarations).
    #[must_use]
    pub fn canonical_style_snapshot(&self, node_key: DomNodeSlotKey) -> DomCanonicalStyleSnapshot {
        let mut styles = Vec::new();
        if let Some(block) = self.inline_decl_block(node_key) {
            for (declaration, important) in block.declarations().iter().zip(block.importance()) {
                let value = if *important {
                    // An important declaration must replay through the
                    // style-attribute route unchanged: serialize every value
                    // kind to its CSS string (a typed `Color` would drop the
                    // importance channel on replay) and append the suffix.
                    let mut text = decl_value_to_css_string(
                        &declaration.value,
                        |key| self.resolve_inline_text(key),
                        |key| self.resolve_inline_track_list(key),
                        |key| self.resolve_inline_grid_areas(key),
                        |key| self.resolve_inline_family_list(key),
                        |key| self.resolve_inline_text(key),
                    );
                    text.push_str(" !important");
                    DomAttributeValue::String(text)
                } else {
                    decl_value_to_dom_value(
                        &declaration.value,
                        |key| self.resolve_inline_text(key),
                        |key| self.resolve_inline_track_list(key),
                        |key| self.resolve_inline_grid_areas(key),
                        |key| self.resolve_inline_family_list(key),
                        |key| self.resolve_inline_text(key),
                    )
                };
                styles.push((declaration.property.name().to_string(), value));
            }
        }
        DomCanonicalStyleSnapshot { styles }
    }

    /// The canonical public snapshot for one node: the inline-declaration style
    /// list plus the node-local layout snapshot (spine 3.3g-2a). The style side
    /// lives here because only the root can resolve the inline store.
    pub fn canonical_snapshot(
        &self,
        node_key: DomNodeSlotKey,
    ) -> Result<DomCanonicalSnapshot, DomError> {
        let layout = self
            .nodes
            .get(node_key)
            .ok_or(DomError::InvalidNodeKey)?
            .canonical_layout_snapshot();
        let style = self.canonical_style_snapshot(node_key);
        Ok(DomCanonicalSnapshot { style, layout })
    }

    /// Schedules one node-owned transition and keeps the node in the root
    /// scheduler until all of its active transitions have finished.
    pub fn set_transition(
        &mut self,
        key: DomNodeSlotKey,
        transition: DomTransition,
    ) -> Result<(), DomError> {
        let transition = validate_transition_payload(transition)?;
        let property = transition.property;
        let canceled_elapsed_s = self
            .nodes
            .get(key)
            .ok_or(DomError::InvalidNodeKey)?
            .active_transition(property)
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
                property,
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
            let previous_layer_root = if node.active_transition_properties().any(|property| {
                GroupDirtyClassification::for_group(property.group()).reevaluate_layer_membership
            }) {
                current_layer_root_key(&self.nodes, key)
            } else {
                None
            };
            let Some(node) = self.nodes.get_mut(key) else {
                continue;
            };
            let result = node.sync_transitions(self.clock_s);
            let next_layer_kind = layer_kind_for_node(node);
            if result.invalidation.layout {
                self.mark_layout_dirty_for_attribute_change(key);
            }
            if result.invalidation.paint {
                self.mark_paint_dirty_for_attribute_change(key);
            }
            // A7 `remeasure_text` (font/text) and CSS Cascade §6.1 (inherited
            // font/text/color interpolation re-seeds descendants): both need
            // the node's style phase re-run up to its layer root so the pass
            // remeasures text and re-seeds descendants with the interpolated
            // values.
            if result.invalidation.remeasure_text || result.inherited_progress {
                self.mark_style_dirty_for_attribute_change(key);
            }
            self.mark_previous_layer_dirty_when_membership_changes(
                key,
                attribute_invalidation_from_classification(result.invalidation),
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
            for finished in result.finished_properties {
                self.dispatch_and_record_transition_event(
                    key,
                    DomEventName::TransitionEnd,
                    finished.property,
                    finished.elapsed_s,
                );
            }
            if result.still_active {
                self.transitions.active_nodes.insert(key);
            }
        }
    }

    /// Schedules the pass-driven transitions for one element whose computed
    /// groups changed from `old_groups` to `new_groups` during this style pass
    /// (spine 3.3g-1b, D2 one-timing). For each moved field (A7's field-level
    /// compare), maps field → `PresentationProperty`, resolves the slot's spec
    /// (`transition_spec_for_longhands`), and installs a transition with `from`
    /// = the last-pass presented value (the active transition's presented value
    /// for a re-target; the raw old field otherwise) and `to` = the new field.
    /// Cancellation, the schedule metric, and `active_nodes` registration ride
    /// through `set_transition` — one `TransitionScheduleAllocation` increment
    /// per scheduled transition, the same semantic the counter has today.
    pub(crate) fn schedule_pass_transitions(
        &mut self,
        node_key: DomNodeSlotKey,
        old_groups: &ComputedGroups,
        new_groups: &ComputedGroups,
        slot: &ResolvedTransitionLonghands,
    ) -> Result<(), DomError> {
        let mut schedule_error = None;
        for_each_moved_field(old_groups, new_groups, |property| {
            if schedule_error.is_some() {
                return;
            }
            let Some(spec) = transition_spec_for_longhands(slot, property) else {
                return;
            };
            let from = {
                let node = self
                    .nodes
                    .get(node_key)
                    .expect("pass-scheduled transition node should exist");
                match node.active_transition(property) {
                    Some(transition) => Some(transition.presented),
                    None => decl_value_for_groups(old_groups, property),
                }
            };
            let Some(from) = from else {
                return;
            };
            let Some(to) = decl_value_for_groups(new_groups, property) else {
                return;
            };
            if from == to {
                return;
            }
            let transition = DomTransition::new(DomTransitionConfig {
                node_key,
                property,
                from,
                to,
                started_at_s: self.clock_s,
                duration_s: spec.duration_s,
                delay_s: spec.delay_s,
                timing: spec.timing,
            });
            if let Err(error) = self.set_transition(node_key, transition) {
                schedule_error = Some(error);
            }
        });
        match schedule_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Applies A7's group-diff dirty classification for a pass-driven group
    /// change (spine 3.3g-1b; primary since 3.3g-2a, when the write path's
    /// static invalidation classes died with the attr-projection channel).
    /// `remeasure_text` is deliberately not mapped to a style mark: the pass
    /// already remeasures text intrinsics via
    /// `recompute_text_metric_intrinsic_for_style_pass`. Returns the computed
    /// classification so the caller can observe it (e.g. `StylePassNodeChanged`)
    /// without re-running the diff.
    pub(crate) fn apply_group_diff_classification(
        &mut self,
        node_key: DomNodeSlotKey,
        old_groups: &ComputedGroups,
        new_groups: &ComputedGroups,
    ) -> GroupDirtyClassification {
        let classification = GroupDirtyClassification::diff(old_groups, new_groups);
        if classification.layout {
            self.mark_layout_dirty_for_attribute_change(node_key);
        }
        if classification.paint {
            self.mark_paint_dirty_for_attribute_change(node_key);
        }
        if classification.reevaluate_layer_membership {
            self.reevaluate_layer_membership_for_pass(node_key, classification);
        }
        classification
    }

    /// The pass-side layer-membership re-evaluation (spine 3.3g-1b, staged for
    /// 3.3g-2 where it becomes primary). Idempotent against the attr-write
    /// path's own membership detection: it compares the node's stored
    /// `layout.layer_kind` (the last layout pass's kind) against the freshly
    /// derived `layer_kind_for_node` and only runs the flip machinery (previous
    /// layer remainder dirty + queue append/remove root) on an actual
    /// difference — the queues dedup, so a difference the attr path already
    /// handled no-ops.
    fn reevaluate_layer_membership_for_pass(
        &mut self,
        node_key: DomNodeSlotKey,
        classification: GroupDirtyClassification,
    ) {
        let (stored_layer_kind, next_layer_kind) = {
            let node = self
                .nodes
                .get(node_key)
                .expect("pass-classified node should exist");
            (node.layout().layer_kind, layer_kind_for_node(node))
        };
        if stored_layer_kind == next_layer_kind {
            return;
        }
        let previous_layer_root = if stored_layer_kind == crate::types::DomLayerKind::None {
            current_layer_root_key(&self.nodes, node_key)
        } else {
            Some(node_key)
        };
        self.mark_previous_layer_dirty_when_membership_changes(
            node_key,
            attribute_invalidation_from_classification(classification),
            previous_layer_root,
        );
        if stored_layer_kind != crate::types::DomLayerKind::None {
            self.queue_layer_remove_root(node_key);
        }
        if next_layer_kind != crate::types::DomLayerKind::None {
            self.queue_layer_append_root(node_key);
        }
    }

    /// Baseline layer-membership re-evaluation (spine 3.3g-2a). The write path
    /// no longer re-projects or queues layer membership synchronously, so a
    /// freshly-styled node that became a layer root on its FIRST pass (e.g.
    /// `position: fixed`) must have its layer materialized by the pass itself.
    /// Mirrors the deleted write-path flip: compare the node's stored (last
    /// layout) layer kind against the freshly derived kind and queue the
    /// append/remove — there is no previous layer remainder to mark on a
    /// baseline.
    pub(crate) fn reevaluate_layer_membership_on_baseline(&mut self, node_key: DomNodeSlotKey) {
        let (stored_layer_kind, next_layer_kind) = {
            let node = self
                .nodes
                .get(node_key)
                .expect("baseline-classified node should exist");
            (node.layout().layer_kind, layer_kind_for_node(node))
        };
        if stored_layer_kind == next_layer_kind {
            return;
        }
        if stored_layer_kind != crate::types::DomLayerKind::None {
            self.queue_layer_remove_root(node_key);
        }
        if next_layer_kind != crate::types::DomLayerKind::None {
            self.queue_layer_append_root(node_key);
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

    /// The text-input seam: delegates to the shared text-edit behavior in
    /// `element_behaviors/text_entry.rs`. Kept as a method because the paint
    /// caret test drives it directly.
    pub(crate) fn apply_default_text_behavior_and_dispatch_input_from_keyboard_input(
        &mut self,
        input: &KeyboardInput,
        shift_key: bool,
    ) -> Result<bool, DomError> {
        apply_default_text_behavior(self, input, shift_key)
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
        property: PresentationProperty,
        elapsed_s: f64,
    ) {
        self.increment_metric(RootDomMetricName::TransitionEventPathAllocation);
        let mut event = DomEvent::new(
            name,
            DomEventDetail::Transition(DomTransitionEvent {
                property,
                elapsed_s,
            }),
        );
        event.target_key = Some(key);
        let _ = self.dispatch_event_to_target_key_and_record_if_bubbled(key, event);
        if name == DomEventName::TransitionEnd
            && presented_options_popup_state(self, key) == Some(PRESENTED_OPTIONS_STATE_CLOSING)
        {
            // The popup transition-end hook (PR 5, E6-b): a closing presented-
            // options shell is queued for layer removal once its opacity
            // transition finishes. The popup-specific state check re-expresses
            // through `presented_options_popup_state` (the popup module's
            // helper); the layer-removal push stays inline here — E7 back-off
            // (c): an inline `&mut RootDom` mutation of the private
            // layer-removal queue inside the transition tick (no `pub(crate)`
            // push accessor exists, and the private `queue_layer_remove_root`
            // carries an attached-parent guard this hook deliberately omits).
            push_unique_key(&mut self.pending_layer_remove_roots, key);
        }
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

    fn current_element_id(&self, key: DomNodeSlotKey) -> Option<String> {
        self.nodes
            .get(key)
            .and_then(DomNode::as_element)
            .and_then(|element| element.attrs.id.clone())
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
        mark_dirty_up_to_layer_root(&mut self.nodes, key, DirtyFlag::Layout);
    }

    pub(crate) fn mark_paint_dirty_for_attribute_change(&mut self, key: DomNodeSlotKey) {
        mark_node_dirty_up_to_layout_root(self, key);
    }

    pub(crate) fn mark_style_dirty_for_attribute_change(&mut self, key: DomNodeSlotKey) {
        mark_dirty_up_to_layer_root(&mut self.nodes, key, DirtyFlag::Style);
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

impl RootDom {
    /// Returns the node's inline declaration block handle (css-style-attr:
    /// the style attribute / programmatic declaration store).
    pub fn style_mut(&mut self, node: DomNodeSlotKey) -> StyleMut<'_> {
        StyleMut::new(self, node)
    }

    /// Returns the node's inline declaration block, if it has one (absent
    /// means the node has never had an inline style written).
    pub(crate) fn inline_decl_block(&self, node: DomNodeSlotKey) -> Option<&DeclBlock> {
        let key = self.nodes.get(node)?.inline_decl()?;
        self.inline_decl_store.get(key)
    }

    /// Resolves one interned inline text key against the inline store's
    /// interner.
    pub(crate) fn resolve_inline_text(&self, key: TextKey) -> Option<&str> {
        self.inline_decl_store.resolve(key)
    }

    /// Mutable access to the inline store's interner, so declaration parsing
    /// interns text through the document's inline arena (a `TextKey` is
    /// store-relative).
    pub(crate) fn inline_texts_mut(&mut self) -> &mut TextInterner {
        self.inline_decl_store.texts_mut()
    }

    /// Mutable access to the inline declaration store (the interning owner:
    /// text AND track lists), for the declaration-parse entries.
    pub(crate) fn inline_decl_store_mut(&mut self) -> &mut DeclStore {
        &mut self.inline_decl_store
    }

    /// Resolves an inline-arena track-list key to its retained `GridTrackList`
    /// (the winners fold's reintern target reads through this).
    #[must_use]
    pub(crate) fn resolve_inline_track_list(
        &self,
        key: GridTrackListKey,
    ) -> Option<&GridTrackList> {
        self.inline_decl_store.resolve_track_list(key)
    }

    /// Resolves an inline-arena areas-map key to its retained
    /// `GridTemplateAreas` (the winners fold's reintern target reads through
    /// this).
    #[must_use]
    pub(crate) fn resolve_inline_grid_areas(
        &self,
        key: GridAreasKey,
    ) -> Option<&GridTemplateAreas> {
        self.inline_decl_store.resolve_grid_areas(key)
    }

    /// Resolves an inline-arena family-list key to its retained
    /// `TextFamilyList` (the winners fold's reintern target and the computed
    /// read surface read through this).
    #[must_use]
    pub(crate) fn resolve_inline_family_list(
        &self,
        key: TextFamilyListKey,
    ) -> Option<&TextFamilyList> {
        self.inline_decl_store.resolve_family_list(key)
    }

    /// Level accessor over retained state (observability doctrine: levels are
    /// read accessors, not emitted gauges): live inline declaration blocks.
    pub(crate) fn inline_decl_live_block_count(&self) -> usize {
        self.inline_decl_store.live_block_count()
    }

    /// Upserts parsed declarations (with their importance flags) into the
    /// node's inline block (created on first use; the key is written to
    /// `node.presentation.decl`, the single source). Returns whether the block
    /// changed.
    pub(crate) fn upsert_inline_declarations(
        &mut self,
        node: DomNodeSlotKey,
        declarations: &[ParsedDeclaration],
    ) -> bool {
        if declarations.is_empty() {
            return false;
        }
        let key = if let Some(key) = self.nodes.get(node).and_then(DomNode::inline_decl) {
            key
        } else {
            let new_key = self.inline_decl_store.insert(DeclBlock::new());
            self.nodes
                .get_mut(node)
                .expect("upsert target node should exist")
                .set_inline_decl(Some(new_key));
            new_key
        };
        let mut changed = false;
        {
            let Some(block) = self.inline_decl_store.get_mut(key) else {
                return false;
            };
            for parsed in declarations {
                changed |= block.upsert(
                    Declaration {
                        property: parsed.property,
                        value: parsed.value,
                    },
                    parsed.important,
                );
            }
        }
        if changed {
            self.mark_style_dirty_for_attribute_change(node);
            self.increment_metric(RootDomMetricName::StyleDeclarationUpsert);
        }
        changed
    }

    /// Replaces the node's inline block wholesale (cssText semantics): an empty
    /// block clears `presentation.decl` and frees the old block; otherwise the
    /// old block is freed and the new one stored. Returns whether the stored
    /// declarations differ from the previous block.
    pub(crate) fn replace_inline_decl_block(
        &mut self,
        node: DomNodeSlotKey,
        block: DeclBlock,
    ) -> bool {
        let old_key = self.nodes.get(node).and_then(DomNode::inline_decl);
        let changed = match old_key {
            Some(old_key) => {
                let differs = match self.inline_decl_store.get(old_key) {
                    Some(old_block) => old_block != &block,
                    None => true,
                };
                self.inline_decl_store.remove(old_key);
                if block.is_empty() {
                    self.nodes
                        .get_mut(node)
                        .expect("replace target node should exist")
                        .set_inline_decl(None);
                } else {
                    let new_key = self.inline_decl_store.insert(block);
                    self.nodes
                        .get_mut(node)
                        .expect("replace target node should exist")
                        .set_inline_decl(Some(new_key));
                }
                differs
            }
            None => {
                if block.is_empty() {
                    false
                } else {
                    let new_key = self.inline_decl_store.insert(block);
                    self.nodes
                        .get_mut(node)
                        .expect("replace target node should exist")
                        .set_inline_decl(Some(new_key));
                    true
                }
            }
        };
        if changed {
            self.mark_style_dirty_for_attribute_change(node);
            self.increment_metric(RootDomMetricName::StyleDeclarationUpsert);
        }
        changed
    }

    /// Removes the named properties from the node's inline block; frees the
    /// block and clears `presentation.decl` if it becomes empty. Returns
    /// whether anything was removed.
    pub(crate) fn remove_inline_declarations(
        &mut self,
        node: DomNodeSlotKey,
        properties: &[PresentationProperty],
    ) -> bool {
        let Some(key) = self.nodes.get(node).and_then(DomNode::inline_decl) else {
            return false;
        };
        let mut changed = false;
        let became_empty = {
            let Some(block) = self.inline_decl_store.get_mut(key) else {
                return false;
            };
            for property in properties {
                changed |= block.remove(*property);
            }
            block.is_empty()
        };
        if !changed {
            return false;
        }
        if became_empty {
            self.nodes
                .get_mut(node)
                .expect("remove target node should exist")
                .set_inline_decl(None);
            self.inline_decl_store.remove(key);
        }
        self.mark_style_dirty_for_attribute_change(node);
        self.increment_metric(RootDomMetricName::StyleDeclarationUpsert);
        true
    }
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
        Ok(DomEventDispatchOutcome::Dispatched(propagate(
            self, target_key, event,
        )?))
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
        let Some(kind) = resolve_kind(self, target_key) else {
            return Ok(false);
        };
        match behavior_row(kind).on_keyboard {
            Some(hook) => hook(self, target_key, input, shift_key, event_stayed_unblocked),
            None => Ok(false),
        }
    }

    pub(crate) fn apply_after_focus(&mut self, target_key: DomNodeSlotKey) -> Result<(), DomError> {
        let Some(kind) = resolve_kind(self, target_key) else {
            return Ok(());
        };
        match behavior_row(kind).on_focus {
            Some(hook) => hook(self, target_key),
            None => Ok(()),
        }
    }

    pub(crate) fn apply_on_blur(&mut self, target_key: DomNodeSlotKey) -> Result<(), DomError> {
        let Some(kind) = resolve_kind(self, target_key) else {
            return Ok(());
        };
        match behavior_row(kind).on_blur {
            Some(hook) => hook(self, target_key),
            None => Ok(()),
        }
    }

    pub(crate) fn apply_pointer_down_default_lifecycles(
        &mut self,
        point: PointPx,
        raw_target_key: Option<DomNodeSlotKey>,
        default_target: Option<DomPointEventTarget>,
    ) -> Result<(), DomError> {
        self.pressed_pointer_click_target =
            pressed_pointer_click_target_from_raw_target(self, raw_target_key);
        let Some(target) = default_target else {
            return Ok(());
        };
        let Some(kind) = resolve_kind(self, target.target_key) else {
            return Ok(());
        };
        match behavior_row(kind).on_pointer_down {
            Some(hook) => hook(self, target, point),
            None => Ok(()),
        }
    }

    pub(crate) fn apply_pointer_up_default_lifecycles(
        &mut self,
        point: PointPx,
        raw_target_key: Option<DomNodeSlotKey>,
    ) -> Result<(), DomError> {
        finish_range_drag_change_if_dirty(self)?;
        let pressed_click_target = self.pressed_pointer_click_target.take();
        let Some(pressed_click_target) = pressed_click_target else {
            return Ok(());
        };
        if raw_target_key != Some(pressed_click_target.target_key) {
            return Ok(());
        }
        let _ = dispatch_click_and_apply_after_click(self, pressed_click_target.target_key, point)?;
        Ok(())
    }
}

// Extraction note (PR 5, E7): the transition inverse-projection and
// payload-validation leaves (`decl_value_for_groups` + its edge/keyword
// mappers, `validate_transition_payload`, `transition_longhand_property`,
// `decl_value_matches_grammar`, `transition_elapsed_s_at`) re-homed to
// `types/transition_projection.rs` — they read computed groups and payloads,
// never `RootDom` state. The transition TICK
// (`sync_transitions_at_current_time`) STAYS here — back-off doctrine verdict
// (c): its per-node effects are inline `&mut RootDom` mutations (dirty walks,
// layer queues, event dispatch with the popup transition-end hook) whose
// marks-vs-events ordering is semantic; a journal would accumulate effects
// into a batch (rule 4) or need a borrow-conflicting closure over `&mut self`.

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

// Extraction note (PR 5, E5): the pure pointer-input leaves (`event_point`,
// `pointer_moved`, `hover_pointer_event`, `pointer_pressed`,
// `pointer_released`) re-homed to `types/event_pointer.rs` — they read only
// their parameters. `hover_layer_intersects_removed_keys` STAYS here: it is a
// hover-layer purge predicate (an E3-domain concern whose only caller is
// `clear_hover_for_removed_keys`), not a pointer-event constructor, and its
// parameter type `HoveredLayerEntry` is root-owned. Back-off doctrine verdict
// (c): genuine divergence from the pointer-event constructor group.
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
    node.computed_box().overflow.allows_vertical_scroll()
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

// Extraction note (PR 5, E4): this walk and its per-node helper
// `mark_previous_layer_node_dirty` STAY in `root_dom.rs` rather than joining
// `types/dirty_channel.rs`. They are not the plain ancestor dirty channel:
// the walk starts at the parent, skips marking until it has crossed the
// current layer root, marks the previous-layer remainder, and stops at the
// previous layer root — a layer-membership flip, not an attr-change dirty
// mark. Back-off doctrine verdict (c): genuine divergence.
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

/// One purge-only dependent's purge operation: clears the dependent store of
/// every key in `removed`.
type PurgeDependent = fn(&mut RootDom, &[DomNodeSlotKey]);

/// The static purge-on-remove dependents table (PR 5, E3).
///
/// Each row names one purge-only dependent and the purge that drops removed
/// keys from it. The row ORDER is today's exact `remove_node` cleanup order —
/// order preservation is behavior preservation.
///
/// The census that stays bespoke (NOT in this table) is recorded here so the
/// completeness test can assert the boundary:
///
/// - the focused element is purged by `blur()` (a focus-loss lifecycle with
///   events), not by a pure purge
/// - the hovered layers purge also marks surviving ancestors style-dirty
///   (`clear_hover_for_removed_keys`)
/// - the transitions active set, the inline declaration store, the id index,
///   and the node-slot removal all live in the final teardown loop
///   (`remove_collected_subtree_keys`): the inline/id purges are derived-key
///   (keyed by block key / id string, not node key) and are entangled with
///   node-slot removal in the same pass
///
/// Access-pattern audit against the pre-E3 shapes (PR 5, E3 follow-up): every
/// row's leaf must walk the same side its pre-E3 code walked, or the
/// extraction silently changed the complexity (see `purge.rs`'s two map-shaped
/// access patterns). Pre-E3 shapes are from
/// `git show e21db35~1:crates/ui-dom/src/types/root_dom.rs`:
///
/// - "active element" — pre-E3 `clear_active_for_removed_keys`: one
///   `is_some_and(|key| removed.contains(&key))` on an option. Leaf:
///   `clear_if_in` (option-shaped contains). O(removed), unchanged.
/// - "selection" — pre-E3 `clear_focus_selection_for_removed_keys`'s
///   `TextSelected` arm: one `removed.contains(&target)` on an option. Leaf:
///   `purge_selection` (contains). O(removed), unchanged.
/// - "pressed pointer target" — pre-E3 `clear_focus_selection_for_removed_keys`
///   `is_some_and(|target| removed.contains(&target.target_key))`. Leaf:
///   `clear_if_in_by`. O(removed), unchanged.
/// - "active range drag session" — same pre-E3 option-contains shape. Leaf:
///   `clear_if_in_by`. O(removed), unchanged.
/// - "scroll offsets" — pre-E3 `for removed_key in &removed_keys { map.remove
///   (removed_key) }` (remove-each, O(removed)). E3's `retain_not_in_map` was
///   the WRONG side (O(store × removed)); corrected to `remove_each_in`
///   (O(removed)) — the access pattern is restored, not changed.
/// - "layer append roots" — pre-E3 `pending_layer_append_roots.retain(|key|
///   !removed_keys.contains(key))` (retain, O(store × removed)). Leaf:
///   `retain_not_in_vec`. Unchanged.
/// - "layer remove roots" — same pre-E3 retain shape. Leaf:
///   `retain_not_in_vec`. Unchanged.
const PURGE_DEPENDENTS: &[(&str, PurgeDependent)] = &[
    ("active element", RootDom::purge_active_element),
    ("selection", RootDom::purge_selection),
    (
        "pressed pointer target",
        RootDom::purge_pressed_pointer_target,
    ),
    (
        "active range drag session",
        RootDom::purge_active_range_drag_session,
    ),
    ("scroll offsets", RootDom::purge_scroll_offsets),
    ("layer append roots", RootDom::purge_layer_append_roots),
    ("layer remove roots", RootDom::purge_layer_remove_roots),
];

fn remove_collected_subtree_keys(
    nodes: &mut SlotMap<DomNodeSlotKey, DomNode>,
    removed_keys: &[DomNodeSlotKey],
    active_transitions: &mut std::collections::BTreeSet<DomNodeSlotKey>,
    inline_decl_store: &mut DeclStore,
    id_lookup: &mut HashMap<String, DomNodeSlotKey>,
) {
    for &removed_key in removed_keys {
        active_transitions.remove(&removed_key);
        if let Some(block_key) = nodes.get(removed_key).and_then(DomNode::inline_decl) {
            inline_decl_store.remove(block_key);
        }
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

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use super::*;
    use bevy::{
        image::Image,
        prelude::{Assets, Entity, Transform, Vec2, World},
        sprite::Sprite,
    };

    #[test]
    fn presentation_writes_are_rejected_via_attr_and_custom_escape_hatch_stores() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let node = root.create_element(DomTag::Div).expect("node create");
        root.append_child(root.document_node().0, node)
            .expect("node attach");

        // Name-based D5 gate: a presentation longhand name errors loudly.
        assert_eq!(
            root.set_attr_str(node, "width", "10px"),
            Err(DomError::PresentationRequiresStyleMut)
        );
        // A presentation shorthand name errors too.
        assert_eq!(
            root.set_attr_str(node, "gap", "4px"),
            Err(DomError::PresentationRequiresStyleMut)
        );

        // The style attribute itself still routes into the inline store.
        root.set_attr_str(node, "style", "width: 10px")
            .expect("style attribute should route");
        assert_eq!(
            root.canonical_style_snapshot(node).styles,
            vec![(
                "width".to_string(),
                DomAttributeValue::String("10px".to_string())
            )]
        );

        // Typed `Custom` writes with a presentation name error; other `Custom`
        // names are content data (the documented escape hatch).
        assert_eq!(
            root.set_attr(
                node,
                DomAttribute::Custom("width".to_string()),
                Some(DomAttributeValue::String("10px".to_string())),
            ),
            Err(DomError::PresentationRequiresStyleMut)
        );
        root.set_attr(
            node,
            DomAttribute::Custom("my-data".to_string()),
            Some(DomAttributeValue::String("stored".to_string())),
        )
        .expect("custom content name should store");
        assert_eq!(
            root.node(node)
                .and_then(DomNode::as_element)
                .and_then(|element| element
                    .attrs
                    .attributes
                    .get(&DomAttribute::Custom("my-data".to_string()))),
            Some(&DomAttributeValue::String("stored".to_string()))
        );
    }

    #[test]
    fn style_attribute_route_rejects_non_element_targets() {
        let mut root = RootDom::new(RootDomContext::new_cpu());

        // Document: the fold processes elements only, so a document write is
        // rejected (mirrors `StyleMut::validate_target`).
        let document = root.document_node().0;
        assert_eq!(
            root.set_attr_str(document, "style", "width: 10px"),
            Err(DomError::InvalidAttributeTarget)
        );

        // DocumentFragment: not constructible through a public live-document
        // API, so create one directly to exercise the same route.
        let fragment = root
            .insert_node(DomNode::document_fragment(root.root))
            .expect("fragment");
        assert_eq!(
            root.set_attr_str(fragment, "style", "width: 10px"),
            Err(DomError::InvalidAttributeTarget)
        );

        // Text.
        let text = root.create_text_node("label").expect("text");
        assert_eq!(
            root.set_attr_str(text, "style", "width: 10px"),
            Err(DomError::InvalidAttributeTarget)
        );

        // Element is the one valid target: the write routes.
        let element = root.create_element(DomTag::Div).expect("div");
        root.set_attr_str(element, "style", "width: 10px")
            .expect("element style attribute should route");
        assert_eq!(
            root.canonical_style_snapshot(element).styles,
            vec![(
                "width".to_string(),
                DomAttributeValue::String("10px".to_string())
            )]
        );
    }

    #[test]
    fn style_attribute_route_counts_drops_even_when_block_is_unchanged() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let node = root.create_element(DomTag::Div).expect("div");

        root.set_attr_str(node, "style", "width: 10px")
            .expect("first style write");
        assert_eq!(root.count(RootDomMetricName::StyleDeclarationDropped), 0);

        // The second list drops `mystery` (unknown property), but the surviving
        // block (`width: 10px`) is unchanged, so `replace_inline_decl_block`
        // reports `changed == false`. The drop must still be counted — a drop
        // is a parsing outcome, not a mutation outcome.
        root.set_attr_str(node, "style", "width: 10px; mystery: 1px")
            .expect("second style write");
        assert_eq!(root.count(RootDomMetricName::StyleDeclarationDropped), 1);

        // The unchanged write is not a mutation: the upsert metric stays at one
        // (only the first write changed the block).
        assert_eq!(root.count(RootDomMetricName::StyleDeclarationUpsert), 1);
    }

    #[test]
    fn canonical_style_snapshot_serializes_important_with_suffix_and_round_trips() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let node = root.create_element(DomTag::Div).expect("div");

        root.style_mut(node)
            .set("width", "10px !important")
            .expect("important width");
        let snapshot = root.canonical_style_snapshot(node);
        assert_eq!(
            snapshot.styles,
            vec![(
                "width".to_string(),
                DomAttributeValue::String("10px !important".to_string())
            )]
        );

        // The suffix is CSS-compatible: re-serializing the snapshot's
        // (name, value) pairs to a declaration list and re-parsing through the
        // style-attribute route round-trips to the same snapshot.
        let replayed = snapshot
            .styles
            .iter()
            .map(|(name, value)| {
                let DomAttributeValue::String(text) = value else {
                    panic!("round-trip test replays string values only");
                };
                format!("{name}: {text}")
            })
            .collect::<Vec<_>>()
            .join("; ");
        root.set_attr_str(node, "style", &replayed)
            .expect("style round-trip");
        assert_eq!(root.canonical_style_snapshot(node), snapshot);
    }

    #[test]
    #[allow(clippy::float_cmp)] // the replayed color must equal the parsed pin bit-exactly
    fn canonical_style_snapshot_preserves_importance_for_color_values_and_replays() {
        let context = RootDomContext::new_cpu();
        context
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 1),
                ".x { background-color: #000000 }",
            )
            .expect("the normal background rule should load");
        let mut root = RootDom::new(context);
        let node = root.create_element(DomTag::Div).expect("div");
        root.append_child(root.document_node().0, node)
            .expect("node attach");
        root.set_attr_str(node, "class", "x")
            .expect("class should set");

        // An important Color-valued declaration serializes to its CSS string
        // form (not the typed `DomAttributeValue::Color`, which has no
        // importance channel), so importance survives replay.
        root.style_mut(node)
            .set("background-color", "#112233 !important")
            .expect("important background-color");
        root.style_mut(node)
            .set("caret-color", "#7aa2ff !important")
            .expect("important caret-color");
        let snapshot = root.canonical_style_snapshot(node);
        assert_eq!(
            snapshot.styles,
            vec![
                (
                    "background-color".to_string(),
                    DomAttributeValue::String("#112233 !important".to_string())
                ),
                (
                    "caret-color".to_string(),
                    DomAttributeValue::String("#7aa2ff !important".to_string())
                ),
            ]
        );

        // Replaying through the style-attribute route round-trips to the same
        // snapshot (both color strings re-parse through `parse_css_color_rgba`).
        let replayed = snapshot
            .styles
            .iter()
            .map(|(name, value)| {
                let DomAttributeValue::String(text) = value else {
                    panic!("important declarations serialize to string values only");
                };
                format!("{name}: {text}")
            })
            .collect::<Vec<_>>()
            .join("; ");
        root.set_attr_str(node, "style", &replayed)
            .expect("style round-trip");
        assert_eq!(root.canonical_style_snapshot(node), snapshot);

        // The replayed important background-color wins the cascade over the
        // normal `.x { background-color: #000000 }` rule (CSS Cascade 5 §6.4.1).
        let document_key = root.document_node().0;
        recompute_dirty_style_subtree_for_layer_root(&mut root, document_key).expect("recompute");
        assert_eq!(
            root.computed_style(node)
                .expect("node should exist")
                .as_visual()
                .background_rgba,
            parse_css_color_rgba("#112233").expect("#112233"),
            "the replayed important background-color should beat the normal rule",
        );
    }

    #[test]
    fn canonical_style_snapshot_preserves_importance_for_keyword_values_and_replays() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let node = root.create_element(DomTag::Div).expect("div");

        // `position` uses the closed `Keywords(POSITION_KEYWORDS)` grammar
        // (presentation_property.rs); `absolute` parses to
        // `DeclValue::Keyword(Keyword::Absolute)` and serializes to its
        // canonical name with the ` !important` suffix.
        root.style_mut(node)
            .set("position", "absolute !important")
            .expect("important position");
        let snapshot = root.canonical_style_snapshot(node);
        assert_eq!(
            snapshot.styles,
            vec![(
                "position".to_string(),
                DomAttributeValue::String("absolute !important".to_string())
            )]
        );

        // Replaying through the style-attribute route round-trips to the same
        // snapshot (the keyword re-parses through the closed POSITION_KEYWORDS
        // set).
        let replayed = snapshot
            .styles
            .iter()
            .map(|(name, value)| {
                let DomAttributeValue::String(text) = value else {
                    panic!("important declarations serialize to string values only");
                };
                format!("{name}: {text}")
            })
            .collect::<Vec<_>>()
            .join("; ");
        root.set_attr_str(node, "style", &replayed)
            .expect("style round-trip");
        assert_eq!(root.canonical_style_snapshot(node), snapshot);
    }

    #[test]
    fn canonical_style_snapshot_preserves_importance_for_number_values_and_replays() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let node = root.create_element(DomTag::Div).expect("div");

        // `opacity` uses the `Opacity` grammar (presentation_property.rs), which
        // clamps any finite f32 to [0, 1] and yields `DeclValue::Number`; 0.5
        // serializes to "0.5" with the ` !important` suffix.
        root.style_mut(node)
            .set("opacity", "0.5 !important")
            .expect("important opacity");
        let snapshot = root.canonical_style_snapshot(node);
        assert_eq!(
            snapshot.styles,
            vec![(
                "opacity".to_string(),
                DomAttributeValue::String("0.5 !important".to_string())
            )]
        );

        // Replaying through the style-attribute route round-trips to the same
        // snapshot (the number re-parses through the `Opacity` grammar).
        let replayed = snapshot
            .styles
            .iter()
            .map(|(name, value)| {
                let DomAttributeValue::String(text) = value else {
                    panic!("important declarations serialize to string values only");
                };
                format!("{name}: {text}")
            })
            .collect::<Vec<_>>()
            .join("; ");
        root.set_attr_str(node, "style", &replayed)
            .expect("style round-trip");
        assert_eq!(root.canonical_style_snapshot(node), snapshot);
    }

    #[test]
    fn canonical_style_snapshot_preserves_importance_for_integer_values_and_replays() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let node = root.create_element(DomTag::Div).expect("div");

        // `font-weight` uses the `FontWeight` grammar (presentation_property.rs);
        // the numeric arm (100..=900 step 100) yields `DeclValue::Integer`, so
        // 700 serializes to "700" with the ` !important` suffix.
        root.style_mut(node)
            .set("font-weight", "700 !important")
            .expect("important font-weight");
        let snapshot = root.canonical_style_snapshot(node);
        assert_eq!(
            snapshot.styles,
            vec![(
                "font-weight".to_string(),
                DomAttributeValue::String("700 !important".to_string())
            )]
        );

        // Replaying through the style-attribute route round-trips to the same
        // snapshot (the integer re-parses through the `FontWeight` numeric arm).
        let replayed = snapshot
            .styles
            .iter()
            .map(|(name, value)| {
                let DomAttributeValue::String(text) = value else {
                    panic!("important declarations serialize to string values only");
                };
                format!("{name}: {text}")
            })
            .collect::<Vec<_>>()
            .join("; ");
        root.set_attr_str(node, "style", &replayed)
            .expect("style round-trip");
        assert_eq!(root.canonical_style_snapshot(node), snapshot);
    }

    #[test]
    fn canonical_style_snapshot_preserves_importance_for_text_values_and_replays() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let node = root.create_element(DomTag::Div).expect("div");

        // `font-family` uses the `FamilyList` grammar (text seam S2), which
        // strips a quoted name's pair and re-serializes per CSS rules: a name
        // with whitespace is re-quoted, a generic is emitted bare. The
        // ` !important` suffix therefore round-trips through the
        // style-attribute route unchanged (the parsed serialization reproduces
        // the authored form).
        root.style_mut(node)
            .set("font-family", "\"My Font\", sans-serif !important")
            .expect("important font-family");
        let snapshot = root.canonical_style_snapshot(node);
        assert_eq!(
            snapshot.styles,
            vec![(
                "font-family".to_string(),
                DomAttributeValue::String("\"My Font\", sans-serif !important".to_string())
            )]
        );

        // Replaying through the style-attribute route round-trips to the same
        // snapshot (the family list re-parses to the same serialized form).
        let replayed = snapshot
            .styles
            .iter()
            .map(|(name, value)| {
                let DomAttributeValue::String(text) = value else {
                    panic!("important declarations serialize to string values only");
                };
                format!("{name}: {text}")
            })
            .collect::<Vec<_>>()
            .join("; ");
        root.set_attr_str(node, "style", &replayed)
            .expect("style round-trip");
        assert_eq!(root.canonical_style_snapshot(node), snapshot);
    }

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
    fn transition_shorthand_writes_longhand_attrs() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let node = root.create_element(DomTag::Div).expect("node create");
        root.append_child(root.document_node().0, node)
            .expect("node attach");

        root.style_mut(node)
            .set("transition", "left 180ms ease-out 40ms")
            .expect("transition shorthand should set");
        let document_key = root.document_node().0;
        recompute_dirty_style_subtree_for_layer_root(&mut root, document_key).expect("recompute");

        // The shorthand expands to its four longhands, resolved into the
        // scheduler-facing transition slot (spine 3.3g-2a: not stored as attrs).
        let view = root.computed_style(node).expect("node should exist");
        let longhands = view.transition_longhands();
        assert_eq!(
            longhands.properties.as_slice(),
            &[
                crate::domains::style::computed::TransitionLonghandProperty::Property(
                    PresentationProperty::Left
                )
            ]
        );
        assert_eq!(longhands.durations_s.as_slice(), &[0.18]);
        assert_eq!(longhands.timing_functions.as_slice(), &[Keyword::EaseOut]);
        assert_eq!(longhands.delays_s.as_slice(), &[0.04]);
    }

    #[test]
    fn transition_shorthand_removal_clears_longhand_attrs() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let node = root.create_element(DomTag::Div).expect("node create");
        root.append_child(root.document_node().0, node)
            .expect("node attach");
        root.style_mut(node)
            .set("transition", "left 180ms ease-out 40ms")
            .expect("transition shorthand should set");

        root.style_mut(node)
            .remove("transition")
            .expect("transition shorthand removal should clear its longhands");

        let view = root.computed_style(node).expect("node should exist");
        assert!(view.transition_longhands().is_empty());
    }

    #[test]
    fn transition_longhands_schedule_listed_property_only() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let node = root.create_element(DomTag::Div).expect("node create");
        root.append_child(root.document_node().0, node)
            .expect("node attach");
        root.style_mut(node)
            .set("left", "0px")
            .expect("initial left");
        root.style_mut(node).set("top", "0px").expect("initial top");
        root.style_mut(node)
            .set("transition-property", "left")
            .expect("transition-property");
        root.style_mut(node)
            .set("transition-duration", "180ms")
            .expect("transition-duration");
        let document_key = root.document_node().0;
        // Baseline pass establishes the node's computed groups (left=0) as the
        // pre-change value (D2 one-timing: scheduling moved to the pass).
        recompute_dirty_style_subtree_for_layer_root(&mut root, document_key)
            .expect("baseline recompute");

        root.style_mut(node)
            .set("left", "100px")
            .expect("left should transition");
        recompute_dirty_style_subtree_for_layer_root(&mut root, document_key)
            .expect("recompute should schedule the left transition");
        // The scheduled left transition overlays the computed box at its `from`
        // value (0px).
        assert_eq!(
            root.node(node)
                .expect("node should exist")
                .computed_box()
                .left,
            BoxLength::Px(0.0)
        );

        root.style_mut(node)
            .set("top", "100px")
            .expect("top should set without transition");
        recompute_dirty_style_subtree_for_layer_root(&mut root, document_key)
            .expect("recompute should not schedule a top transition");
        assert_eq!(
            root.node(node)
                .expect("node should exist")
                .computed_box()
                .top,
            BoxLength::Px(100.0)
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
        root.style_mut(child)
            .set("position", "fixed")
            .expect("fixed position");
        root.style_mut(child).set("left", "12px").expect("left");
        root.style_mut(child).set("top", "8px").expect("top");

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
        root.style_mut(parent)
            .set("position", "fixed")
            .expect("fixed position");
        root.style_mut(parent).set("left", "12px").expect("left");
        root.style_mut(parent).set("top", "8px").expect("top");
        let child = root.create_element(DomTag::Div).expect("child create");
        root.append_child(parent, child).expect("child append");
        root.style_mut(child)
            .set("position", "absolute")
            .expect("absolute position");
        root.style_mut(child).set("left", "3px").expect("left");
        root.style_mut(child).set("top", "2px").expect("top");

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
        root.style_mut(select).set("width", "120px").expect("width");
        root.style_mut(select)
            .set("height", "28px")
            .expect("height");
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
        root.style_mut(select).set("width", "120px").expect("width");
        root.style_mut(select)
            .set("height", "28px")
            .expect("height");
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
        let shell_key = presented_options_popup_shell_key(root, select)
            .expect("select popup shell should exist");
        let shell_entity = root
            .layer_entity(shell_key)
            .expect("select popup shell entity");
        let shell_box = node_box_from_ref(root.node(shell_key).expect("shell")).border_box;
        let row_keys = presented_options_popup_row_keys(root, select);
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
        root.style_mut(select)
            .set("margin-left", "12px")
            .expect("select margin left");
        root.style_mut(select)
            .set("margin-top", "40px")
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
        let shell_key = presented_options_popup_shell_key(&root, select)
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
        root.style_mut(wrapper)
            .set("display", "block")
            .expect("wrapper display");
        root.style_mut(wrapper)
            .set("margin-left", "31px")
            .expect("wrapper margin left");
        root.style_mut(wrapper)
            .set("margin-top", "53px")
            .expect("wrapper margin top");
        root.style_mut(wrapper)
            .set("padding-left", "9px")
            .expect("wrapper padding left");
        root.style_mut(wrapper)
            .set("padding-top", "7px")
            .expect("wrapper padding top");
        // The document node's inline style is not folded (the style pass skips
        // document/fragment nodes), so make the root's content tall enough to
        // keep the anchored popup inside the hit-test viewport instead of
        // sizing the document box.
        let select = append_select_to_parent(&mut root, wrapper);
        let spacer = root.create_element(DomTag::Div).expect("spacer");
        root.append_child(wrapper, spacer).expect("append spacer");
        root.style_mut(spacer)
            .set("height", "80px")
            .expect("spacer height");
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

    #[test]
    fn name_is_presentation_covers_all_presentation_properties_and_shorthands() {
        // Every longhand name of the 69-property vocabulary is presentation.
        for property in PresentationProperty::ALL {
            assert!(
                name_is_presentation(property.name()),
                "{} ({property:?}) should be a presentation name",
                property.name()
            );
        }
        // The shorthand names the declaration parser expands are presentation.
        for name in [
            "gap",
            "flex",
            "flex-flow",
            "place-content",
            "place-items",
            "place-self",
            "transition",
            "border-radius",
            "border-color",
            "border-top",
            "border-right",
            "border-bottom",
            "border-left",
            "margin",
            "padding",
        ] {
            assert!(name_is_presentation(name), "{name} should be presentation");
        }
        // Content and non-presentation names skip.
        for name in [
            "class",
            "id",
            "value",
            "src",
            "checked",
            "style",
            "disabled",
            "readonly",
            "lang",
            "placeholder",
            "data-role",
        ] {
            assert!(
                !name_is_presentation(name),
                "{name} should not be presentation"
            );
        }
    }

    #[test]
    fn purge_dependents_table_covers_the_census_in_todays_order() {
        let names: Vec<&str> = PURGE_DEPENDENTS.iter().map(|(name, _)| *name).collect();
        let census = [
            "active element",
            "selection",
            "pressed pointer target",
            "active range drag session",
            "scroll offsets",
            "layer append roots",
            "layer remove roots",
        ];
        assert_eq!(
            names,
            census.to_vec(),
            "purge table must match the census in today's exact order"
        );
    }

    #[test]
    fn remove_node_purges_every_purge_only_dependent() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let document = root.document_node().0;
        let removed = root.create_element(DomTag::Div).expect("removed create");
        let sibling = root.create_element(DomTag::Div).expect("sibling create");
        root.append_child(document, removed)
            .expect("append removed");
        root.append_child(document, sibling)
            .expect("append sibling");

        // Populate every purge-only dependent on the removed key. The focused
        // element stays on the sibling so the blur() lifecycle is not exercised
        // by this purge-table test (blur is a separate, event-emitting concern).
        root.pseudo_state.active_element_key = Some(removed);
        root.pseudo_state.focused_element_key = Some(sibling);
        root.selection = DomSelection::TextSelected(TextSelection {
            target_element: removed,
            selection_start: 0,
            selection_end: 0,
            selection_direction: TextSelectionDirection::None,
        });
        root.pressed_pointer_click_target = Some(PressedPointerClickTarget {
            target_key: removed,
        });
        root.active_range_drag_session = Some(ActiveRangeDragSession {
            target_key: removed,
            root_to_subtree_point_offset: PointPx::default(),
            focus_entry_value: String::new(),
            dirty: false,
        });
        root.vertical_scroll_offset_by_node.insert(removed, 42.0);
        root.pending_layer_append_roots.push(removed);
        root.pending_layer_remove_roots.push(removed);

        root.remove_node(removed).expect("remove node");

        assert_eq!(root.pseudo_state.active_element_key, None);
        assert_eq!(root.pseudo_state.focused_element_key, Some(sibling));
        assert_eq!(root.selection, DomSelection::None);
        assert_eq!(root.pressed_pointer_click_target, None);
        assert_eq!(root.active_range_drag_session, None);
        assert!(!root.vertical_scroll_offset_by_node.contains_key(&removed));
        assert!(!root.pending_layer_append_roots.contains(&removed));
        assert!(!root.pending_layer_remove_roots.contains(&removed));
    }

    #[test]
    fn scroll_offset_purge_drops_only_the_removed_keys_from_a_large_store() {
        // The removal-side fixture: a large retained scroll-offset store and a
        // small removed set. The scroll-offsets row must walk the removed set
        // (O(removed)), not the store — the complexity is documented in the
        // PURGE_DEPENDENTS audit note and in purge.rs's header; here the
        // observable contract is that only the removed keys leave the store.
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let document = root.document_node().0;

        let removed = root.create_element(DomTag::Div).expect("removed");
        root.append_child(document, removed)
            .expect("append removed");
        let mut survivors = Vec::new();
        for _ in 0..64 {
            let key = root.create_element(DomTag::Div).expect("survivor");
            root.append_child(document, key).expect("append survivor");
            root.vertical_scroll_offset_by_node.insert(key, 1.0);
            survivors.push(key);
        }
        root.vertical_scroll_offset_by_node.insert(removed, 7.5);

        root.remove_node(removed).expect("remove node");

        assert!(!root.vertical_scroll_offset_by_node.contains_key(&removed));
        for key in survivors {
            assert!(
                root.vertical_scroll_offset_by_node.contains_key(&key),
                "surviving scroll offset should remain"
            );
        }
    }
}
