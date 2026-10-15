#![allow(dead_code)]

//! `DomNode` is one authored node record inside a root-owned DOM store.
//!
//! What this type represents:
//!
//! - one authored DOM node that already belongs to exactly one owner root
//! - one node in either an attached document tree or a detached fragment tree
//! - one local record of node state such as attrs, text, event listeners,
//!   transitions, and parent/child/sibling links
//!
//! Invariants and rules:
//!
//! - every `DomNode` is owned by exactly one document root key
//! - `DomNode` does not own the document store and cannot resolve siblings,
//!   children, or parents without a root-owned lookup closure
//! - `Document` and `DocumentFragment` are the first node kinds in their trees
//! - every non-root node has exactly one parent
//! - child order is explicit and stable through the sibling links
//! - cycles are forbidden
//! - tree mutation authority belongs to the owner root, not to `DomNode`
//! - id indexing, transition scheduling, and subtree destruction are
//!   root-owned concerns
//!
//! Advice:
//!
//! - prefer DOM-shaped traversal chains over special-case shared helpers
//! - keep shared APIs low-level and composable: `get_element_by_id`, `node`,
//!   `first_child`, `next_sibling`, `parent_node`, `as_element`, `as_text`
//! - if one caller needs a repeated chain, keep that helper local to the
//!   caller rather than teaching `ui-dom` one wrapper-specific shortcut
//!
//! Examples:
//!
//! ```ignore
//! root_dom
//!     .get_element_by_id("confirm-dialog-title")
//!     .and_then(|(_, node)| node.first_child(|key| root_dom.node(key)))
//! ```
//!
//! ```ignore
//! root_dom
//!     .node(parent_key)
//!     .and_then(|node| node.first_child(|key| root_dom.node(key)))
//!     .and_then(|(_, node)| node.next_sibling(|key| root_dom.node(key)))
//! ```
//!
use std::{collections::BTreeMap, sync::Arc};

use crate::domains::layer_ops::TextRunId;
use crate::domains::paint::{measure_text_intrinsic, measure_text_intrinsic_with_font_size};
use crate::types::box_model::DomBoxModel;
use crate::types::dom_attr_parse::{
    layout_length_attr_uses_unset, normalize_attribute_value, parse_class_attr_list,
    parse_style_attr_map, validate_attribute_value,
};
#[cfg(test)]
use crate::types::dom_event::DomEventDispatchContext;
use crate::types::dom_event::{DomEventListener, DomEventListenerOptions, DomEventName};
use crate::types::dom_pseudo_state::{DomPseudoState, DomPseudoStateBits};
use crate::types::dom_tag::{DomElementBehavior, DomElementStateStyleDeclarations, DomTag};
use crate::types::dom_transition::{DomTransition, DomTransitionState};
use crate::types::element_behaviors::{
    commit_intrinsic_after_style_recompute, compute_intrinsic_on_create,
    preview_intrinsic_after_attr_write,
};
use crate::types::event_listener_name_bits::EventListenerNameBits;
use crate::types::{
    DomAttribute, DomAttributeNode, DomAttributeValue, DomAttrs, DomCanonicalLayoutSnapshot,
    DomCanonicalSnapshot, DomCanonicalStyleSnapshot, DomError,
};
use arrayvec::ArrayVec;
use serde::{Deserialize, Serialize};
use slotmap::new_key_type;
use ui_icons::UiIconId;

new_key_type! {
    pub struct DomNodeSlotKey;
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum DomLayerKind {
    #[default]
    None,
    Normal,
    Clipped,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum LayerOrder {
    #[default]
    Normal,
    Raised,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct DomLayoutState {
    pub dirty: bool,
    pub layer_kind: DomLayerKind,
    pub layer_order: LayerOrder,
    pub available_width_px: f32,
    pub available_height_px: f32,
    pub width_px: f32,
    pub height_px: f32,
    /// Retained child-content extent before viewport height clamping.
    pub content_height_px: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum DomPaintState {
    #[default]
    None,
    FullLayerRepaint,
    DamagedRectsRepaint(ArrayVec<crate::types::DomRect, 8>),
}

impl DomPaintState {
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        !matches!(self, Self::None)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DomStyleObject {
    pub dirty: bool,
    hover_declarations: DomElementStateStyleDeclarationsField,
    active_declarations: DomElementStateStyleDeclarationsField,
    focus_declarations: DomElementStateStyleDeclarationsField,
    focus_visible_declarations: DomElementStateStyleDeclarationsField,
}

type DomElementStateStyleDeclarationsField = crate::types::dom_tag::DomStyleDeclarationList;

impl DomStyleObject {
    pub(crate) fn set_state_style_declarations(
        &mut self,
        declarations: DomElementStateStyleDeclarations,
    ) {
        self.hover_declarations = declarations.hover;
        self.active_declarations = declarations.active;
        self.focus_declarations = declarations.focus;
        self.focus_visible_declarations = declarations.focus_visible;
    }

    #[must_use]
    pub(crate) fn pseudo_state_declarations(
        &self,
        pseudo_state: DomPseudoState,
    ) -> &[(DomAttribute, DomAttributeValue)] {
        match pseudo_state {
            DomPseudoState::Hover => self.hover_declarations.as_slice(),
            DomPseudoState::Active => self.active_declarations.as_slice(),
            DomPseudoState::Focus => self.focus_declarations.as_slice(),
            DomPseudoState::FocusVisible => self.focus_visible_declarations.as_slice(),
        }
    }

    #[must_use]
    pub(crate) fn has_pseudo_state_declarations(&self, pseudo_state: DomPseudoState) -> bool {
        !self.pseudo_state_declarations(pseudo_state).is_empty()
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct TextIntrinsic {
    /// Resolved font size used when this intrinsic was measured.
    pub font_size_px: f32,
    pub min_content_width_px: f32,
    pub max_content_width_px: f32,
    pub line_height_px: f32,
    pub generation: u64,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ImgIntrinsic {
    pub width_px: f32,
    pub height_px: f32,
    pub aspect_ratio: f32,
    pub generation: u64,
}

/// Default preferred size metrics for intrinsic element controls.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ElementDefaultPreferredSizeIntrinsic {
    /// Resolved font size used when this intrinsic was measured.
    pub font_size_px: f32,
    /// Content-box preferred width for this control.
    pub width_px: f32,
    /// Content-box preferred height for this control.
    pub height_px: f32,
    pub line_height_px: f32,
    pub generation: u64,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub enum DomIntrinsic {
    #[default]
    None,
    NotMeasured,
    Text(TextIntrinsic),
    Img(ImgIntrinsic),
    ElementDefaultPreferredSize(ElementDefaultPreferredSizeIntrinsic),
}

#[derive(Clone, Debug, PartialEq)]
pub struct DomNodeBase {
    /// Stable identity of the document that created this node.
    pub owner_document_key: DomNodeSlotKey,
    pub box_model: DomBoxModel,
    pub intrinsic: DomIntrinsic,
    pub layout: DomLayoutState,
    pub paint: DomPaintState,
    pub style: DomStyleObject,
    transitions: DomTransitionState,
    parent_key: Option<DomNodeSlotKey>,
    first_child_key: Option<DomNodeSlotKey>,
    last_child_key: Option<DomNodeSlotKey>,
    prev_sibling_key: Option<DomNodeSlotKey>,
    next_sibling_key: Option<DomNodeSlotKey>,
    event_listeners: BTreeMap<DomEventName, Vec<DomEventListenerRegistration>>,
    /// Exact event-name listener presence for non-bubbling dispatch gates.
    exact_listener_name_bits: EventListenerNameBits,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DomElementNode {
    pub base: DomNodeBase,
    pub tag: DomTag,
    pub attrs: DomAttrs,
    pub(crate) attribute_style_map: BTreeMap<DomAttribute, DomAttributeValue>,
    pub(crate) class_list: Vec<String>,
    lowered_image_source_kind: Option<LoweredImageSourceKind>,
    pseudo_state_bits: DomPseudoStateBits,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LoweredImageSourceKind {
    InternalIcon(UiIconId),
    External,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DomDocumentNode {
    /// Shared node links, caches, transitions, and owner identity.
    pub base: DomNodeBase,
    /// Authored root-level attrs that apply to the whole document tree.
    pub attrs: DomAttrs,
    pub(crate) attribute_style_map: BTreeMap<DomAttribute, DomAttributeValue>,
    pub(crate) class_list: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DomDocumentFragmentNode {
    /// Shared node links, caches, transitions, and owner identity.
    pub base: DomNodeBase,
    /// Authored fragment-container attrs used while staging subtree content.
    pub attrs: DomAttrs,
    pub(crate) attribute_style_map: BTreeMap<DomAttribute, DomAttributeValue>,
    pub(crate) class_list: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DomTextNode {
    pub base: DomNodeBase,
    text_run_id_hint: Option<TextRunId>,
    data: Arc<str>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DomNode {
    Document(DomDocumentNode),
    DocumentFragment(DomDocumentFragmentNode),
    Element(DomElementNode),
    Text(DomTextNode),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DomEventListenerRegistration {
    listener: DomEventListener,
    options: DomEventListenerOptions,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DomDispatchedEventListener {
    pub(crate) listener: DomEventListener,
    pub(crate) options: DomEventListenerOptions,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct AttributeInvalidation {
    bits: u8,
}

impl AttributeInvalidation {
    const LAYOUT: u8 = 1 << 0;
    const PAINT: u8 = 1 << 1;
    const STYLE: u8 = 1 << 2;
    const LAYER_MEMBERSHIP: u8 = 1 << 3;

    #[must_use]
    pub(crate) const fn with_layout(mut self) -> Self {
        self.bits |= Self::LAYOUT;
        self
    }

    #[must_use]
    pub(crate) const fn with_paint(mut self) -> Self {
        self.bits |= Self::PAINT;
        self
    }

    #[must_use]
    pub(crate) const fn with_style(mut self) -> Self {
        self.bits |= Self::STYLE;
        self
    }

    #[must_use]
    pub(crate) const fn with_layer_membership(mut self) -> Self {
        self.bits |= Self::LAYER_MEMBERSHIP;
        self
    }

    #[must_use]
    pub(crate) const fn layout(self) -> bool {
        self.bits & Self::LAYOUT != 0
    }

    #[must_use]
    pub(crate) const fn paint(self) -> bool {
        self.bits & Self::PAINT != 0
    }

    #[must_use]
    pub(crate) const fn style(self) -> bool {
        self.bits & Self::STYLE != 0
    }

    #[must_use]
    pub(crate) const fn layer_membership(self) -> bool {
        self.bits & Self::LAYER_MEMBERSHIP != 0
    }

    #[must_use]
    pub(crate) const fn merge(self, other: Self) -> Self {
        Self {
            bits: self.bits | other.bits,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct DomTransitionSyncResult {
    pub invalidation: AttributeInvalidation,
    pub still_active: bool,
    pub finished_attributes: Vec<FinishedDomTransition>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FinishedDomTransition {
    pub attribute: DomAttribute,
    pub elapsed_s: f64,
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

impl DomElementNode {
    /// Returns the document root key that owns this element.
    ///
    /// This is the stable ownership anchor used to prevent nodes from being
    /// attached into the wrong document tree. Callers should treat it as a
    /// document-local identity, not a global DOM id.
    #[must_use]
    pub fn owner_document_key(&self) -> DomNodeSlotKey {
        self.base.owner_document_key
    }

    #[must_use]
    pub(crate) fn new(owner_document_key: DomNodeSlotKey, tag: DomTag) -> Self {
        let descriptor = tag.descriptor();
        let mut base = DomNodeBase::new(owner_document_key);
        base.style
            .set_state_style_declarations(descriptor.state_style_declarations.clone());
        Self {
            base,
            tag,
            attrs: descriptor.default_attrs,
            attribute_style_map: BTreeMap::new(),
            class_list: Vec::new(),
            lowered_image_source_kind: None,
            pseudo_state_bits: DomPseudoStateBits::default(),
        }
    }

    #[must_use]
    pub(crate) const fn lowered_image_source_kind(&self) -> Option<LoweredImageSourceKind> {
        self.lowered_image_source_kind
    }

    pub(crate) fn refresh_lowered_image_source_kind(&mut self) -> Result<(), DomError> {
        if self.tag != DomTag::Img {
            self.lowered_image_source_kind = None;
            return Ok(());
        }
        self.lowered_image_source_kind = lowered_image_source_kind_from_attrs(&self.attrs)?;
        Ok(())
    }

    /// Returns the built-in behavior associated with this element's tag.
    ///
    /// This keeps tag-level behavior decisions centralized on the descriptor
    /// model instead of forcing callers to hard-code special cases. The result
    /// is intended for shared UI behavior, not app-local policy.
    #[must_use]
    pub fn behavior(&self) -> DomElementBehavior {
        self.tag.descriptor().behavior
    }

    #[must_use]
    pub(crate) fn style(&self) -> &DomStyleObject {
        &self.base.style
    }

    /// Returns the current pseudo-state flags for this element.
    #[must_use]
    pub fn pseudo_state_bits(&self) -> DomPseudoStateBits {
        self.pseudo_state_bits
    }

    /// Returns whether this element currently carries the provided pseudo-state.
    #[must_use]
    pub fn has_pseudo_state(&self, pseudo_state: DomPseudoState) -> bool {
        self.pseudo_state_bits.contains(pseudo_state)
    }

    pub(crate) fn set_pseudo_state(&mut self, pseudo_state: DomPseudoState) {
        self.pseudo_state_bits.insert(pseudo_state);
    }

    pub(crate) fn clear_pseudo_state(&mut self, pseudo_state: DomPseudoState) {
        self.pseudo_state_bits.remove(pseudo_state);
    }
}

impl DomAttributeNode for DomElementNode {
    fn attr(&self, attribute: &DomAttribute) -> Option<&DomAttributeValue> {
        stored_attribute_value(&self.attrs, attribute)
    }

    fn attrs(&self) -> &DomAttrs {
        &self.attrs
    }

    fn attribute_style_map(&self) -> &BTreeMap<DomAttribute, DomAttributeValue> {
        &self.attribute_style_map
    }

    fn class_list(&self) -> &[String] {
        &self.class_list
    }

    fn set_attr(
        &mut self,
        attribute: DomAttribute,
        value: Option<DomAttributeValue>,
    ) -> Result<(), DomError> {
        let value = normalize_attribute_value(&attribute, value)?;
        validate_attribute_value(&attribute, value.as_ref())?;
        write_attribute(
            &mut self.attrs,
            &mut self.attribute_style_map,
            &mut self.class_list,
            &attribute,
            value,
        )?;
        self.refresh_lowered_image_source_kind()
    }

    fn after_attr_write_updates_intrinsic(&self, attribute: &DomAttribute) -> bool {
        self.tag
            .descriptor()
            .behavior
            .after_attr_write_intrinsic_update
            .matches(attribute)
    }
}

impl DomDocumentNode {
    /// Returns the document root key that owns this document node.
    ///
    /// This lets callers confirm ownership integrity when they are holding a
    /// raw document node reference. The returned key is the same owner anchor
    /// stamped onto every node created by the same root document.
    #[must_use]
    pub fn owner_document_key(&self) -> DomNodeSlotKey {
        self.base.owner_document_key
    }

    #[must_use]
    pub(crate) fn new(owner_document_key: DomNodeSlotKey) -> Self {
        Self {
            base: DomNodeBase::new(owner_document_key),
            attrs: DomAttrs::default(),
            attribute_style_map: BTreeMap::new(),
            class_list: Vec::new(),
        }
    }
}

fn validate_document_like_attr_target(attribute: &DomAttribute) -> Result<(), DomError> {
    if *attribute == DomAttribute::Id {
        return Err(DomError::InvalidAttributeTarget);
    }
    Ok(())
}

impl DomAttributeNode for DomDocumentNode {
    fn attr(&self, attribute: &DomAttribute) -> Option<&DomAttributeValue> {
        stored_attribute_value(&self.attrs, attribute)
    }

    fn attrs(&self) -> &DomAttrs {
        &self.attrs
    }

    fn attribute_style_map(&self) -> &BTreeMap<DomAttribute, DomAttributeValue> {
        &self.attribute_style_map
    }

    fn class_list(&self) -> &[String] {
        &self.class_list
    }

    fn set_attr(
        &mut self,
        attribute: DomAttribute,
        value: Option<DomAttributeValue>,
    ) -> Result<(), DomError> {
        let value = normalize_attribute_value(&attribute, value)?;
        validate_document_like_attr_target(&attribute)?;
        validate_attribute_value(&attribute, value.as_ref())?;
        write_attribute(
            &mut self.attrs,
            &mut self.attribute_style_map,
            &mut self.class_list,
            &attribute,
            value,
        )
    }

    fn after_attr_write_updates_intrinsic(&self, _attribute: &DomAttribute) -> bool {
        false
    }
}

impl DomDocumentFragmentNode {
    /// Returns the document root key that owns this fragment node.
    ///
    /// This exposes the fragment-local owner anchor used while detached content
    /// is being staged. It exists so fragment nodes can still enforce same-root
    /// attachment rules before import.
    #[must_use]
    pub fn owner_document_key(&self) -> DomNodeSlotKey {
        self.base.owner_document_key
    }

    #[must_use]
    pub(crate) fn new(owner_document_key: DomNodeSlotKey) -> Self {
        Self {
            base: DomNodeBase::new(owner_document_key),
            attrs: DomAttrs::default(),
            attribute_style_map: BTreeMap::new(),
            class_list: Vec::new(),
        }
    }
}

impl DomAttributeNode for DomDocumentFragmentNode {
    fn attr(&self, attribute: &DomAttribute) -> Option<&DomAttributeValue> {
        stored_attribute_value(&self.attrs, attribute)
    }

    fn attrs(&self) -> &DomAttrs {
        &self.attrs
    }

    fn attribute_style_map(&self) -> &BTreeMap<DomAttribute, DomAttributeValue> {
        &self.attribute_style_map
    }

    fn class_list(&self) -> &[String] {
        &self.class_list
    }

    fn set_attr(
        &mut self,
        attribute: DomAttribute,
        value: Option<DomAttributeValue>,
    ) -> Result<(), DomError> {
        let value = normalize_attribute_value(&attribute, value)?;
        validate_document_like_attr_target(&attribute)?;
        validate_attribute_value(&attribute, value.as_ref())?;
        write_attribute(
            &mut self.attrs,
            &mut self.attribute_style_map,
            &mut self.class_list,
            &attribute,
            value,
        )
    }

    fn after_attr_write_updates_intrinsic(&self, _attribute: &DomAttribute) -> bool {
        false
    }
}

impl DomTextNode {
    /// Returns the document root key that owns this text node.
    ///
    /// This is the same ownership anchor used by element and document nodes.
    /// Text nodes expose it directly so ownership checks can stay uniform
    /// across node kinds.
    #[must_use]
    pub fn owner_document_key(&self) -> DomNodeSlotKey {
        self.base.owner_document_key
    }

    #[must_use]
    pub fn data(&self) -> &str {
        self.data.as_ref()
    }

    #[must_use]
    pub(crate) fn data_arc(&self) -> Arc<str> {
        self.data.clone()
    }

    #[must_use]
    pub(crate) fn text_run_id_hint(&self) -> Option<TextRunId> {
        self.text_run_id_hint
    }

    #[must_use]
    pub(crate) fn new(owner_document_key: DomNodeSlotKey, data: Arc<str>) -> Self {
        Self {
            base: DomNodeBase::new(owner_document_key),
            text_run_id_hint: None,
            data,
        }
    }

    pub(crate) fn set_data(&mut self, data: Arc<str>) {
        self.data = data;
    }

    pub(crate) fn set_text_run_id_hint(&mut self, text_run_id_hint: Option<TextRunId>) {
        self.text_run_id_hint = text_run_id_hint;
    }
}

impl DomNode {
    #[must_use]
    pub(crate) fn document(owner_document_key: DomNodeSlotKey) -> Self {
        Self::Document(DomDocumentNode::new(owner_document_key))
    }

    #[must_use]
    pub(crate) fn document_fragment(owner_document_key: DomNodeSlotKey) -> Self {
        Self::DocumentFragment(DomDocumentFragmentNode::new(owner_document_key))
    }

    /// Internal constructor.
    ///
    /// Prefer `RootDomFragment::create_element` for detached authoring, then
    /// consume the fragment through `RootDom::append_fragment`.
    ///
    /// Example:
    /// `let button = fragment.create_element(DomTag::Button)?;`
    #[must_use]
    pub(crate) fn element(owner_document_key: DomNodeSlotKey, tag: DomTag) -> Self {
        Self::Element(DomElementNode::new(owner_document_key, tag))
    }

    /// Internal constructor.
    ///
    /// Prefer `RootDomFragment::create_text_node` for detached authoring, then
    /// consume the fragment through `RootDom::append_fragment`.
    ///
    /// Example:
    /// `let label = fragment.create_text_node("Launch")?;`
    #[must_use]
    pub(crate) fn text(owner_document_key: DomNodeSlotKey, data: Arc<str>) -> Self {
        Self::Text(DomTextNode::new(owner_document_key, data))
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn fixture_text(owner_document_key: DomNodeSlotKey, data: &str) -> Self {
        Self::text(owner_document_key, Arc::<str>::from(data))
    }

    /// Returns the stable document root key that owns this node.
    ///
    /// This gives callers one uniform ownership query across all node kinds.
    /// The returned key is document-local and should be used for ownership
    /// validation rather than as an external identity.
    #[must_use]
    pub fn owner_document_key(&self) -> DomNodeSlotKey {
        self.base().owner_document_key
    }

    fn base(&self) -> &DomNodeBase {
        match self {
            Self::Document(node) => &node.base,
            Self::DocumentFragment(node) => &node.base,
            Self::Element(node) => &node.base,
            Self::Text(node) => &node.base,
        }
    }

    fn base_mut(&mut self) -> &mut DomNodeBase {
        match self {
            Self::Document(node) => &mut node.base,
            Self::DocumentFragment(node) => &mut node.base,
            Self::Element(node) => &mut node.base,
            Self::Text(node) => &mut node.base,
        }
    }

    pub(crate) fn first_child_key_for_store(&self) -> Option<DomNodeSlotKey> {
        self.base().first_child_key
    }

    pub(crate) fn last_child_key_for_store(&self) -> Option<DomNodeSlotKey> {
        self.base().last_child_key
    }

    pub(crate) fn previous_sibling_key_for_store(&self) -> Option<DomNodeSlotKey> {
        self.base().prev_sibling_key
    }

    pub(crate) fn next_sibling_key_for_store(&self) -> Option<DomNodeSlotKey> {
        self.base().next_sibling_key
    }

    pub(crate) fn set_parent_key_for_store(&mut self, key: Option<DomNodeSlotKey>) {
        self.base_mut().parent_key = key;
    }

    pub(crate) fn set_first_child_key_for_store(&mut self, key: Option<DomNodeSlotKey>) {
        self.base_mut().first_child_key = key;
    }

    pub(crate) fn set_last_child_key_for_store(&mut self, key: Option<DomNodeSlotKey>) {
        self.base_mut().last_child_key = key;
    }

    pub(crate) fn set_previous_sibling_key_for_store(&mut self, key: Option<DomNodeSlotKey>) {
        self.base_mut().prev_sibling_key = key;
    }

    pub(crate) fn set_next_sibling_key_for_store(&mut self, key: Option<DomNodeSlotKey>) {
        self.base_mut().next_sibling_key = key;
    }

    pub(crate) fn reset_tree_links_for_store(&mut self) {
        let base = self.base_mut();
        base.parent_key = None;
        base.first_child_key = None;
        base.last_child_key = None;
        base.prev_sibling_key = None;
        base.next_sibling_key = None;
    }

    pub(crate) fn set_owner_document_key_for_store(&mut self, owner_document_key: DomNodeSlotKey) {
        self.base_mut().owner_document_key = owner_document_key;
    }

    pub(crate) fn set_intrinsic_for_store(&mut self, intrinsic: DomIntrinsic) {
        self.base_mut().intrinsic = intrinsic;
    }

    #[must_use]
    pub(crate) fn clone_for_owner_document(&self, owner_document_key: DomNodeSlotKey) -> Self {
        let mut cloned = self.clone();
        cloned.set_owner_document_key_for_store(owner_document_key);
        cloned.reset_tree_links_for_store();
        if let Self::Text(node) = &mut cloned {
            node.set_text_run_id_hint(None);
        }
        cloned
    }

    /// Returns this node as an element node when the underlying kind matches.
    ///
    /// This keeps enum matching out of callers that only need read-only element
    /// access. It returns `None` for non-element node kinds instead of
    /// inventing placeholder element views.
    #[must_use]
    pub fn as_element(&self) -> Option<&DomElementNode> {
        match self {
            Self::Element(node) => Some(node),
            Self::Document(_) | Self::DocumentFragment(_) | Self::Text(_) => None,
        }
    }

    /// Returns this node as a mutable element node when the kind matches.
    ///
    /// This is the mutation counterpart to `as_element` and exists so root code
    /// can change element-owned state without repeating enum matches. It stays
    /// strict by returning `None` for non-element nodes.
    pub fn as_element_mut(&mut self) -> Option<&mut DomElementNode> {
        match self {
            Self::Element(node) => Some(node),
            Self::Document(_) | Self::DocumentFragment(_) | Self::Text(_) => None,
        }
    }

    /// Returns this node as a text node when the underlying kind matches.
    ///
    /// This keeps text-specific access explicit while preserving one shared
    /// `DomNode` surface. Non-text node kinds return `None` instead of a fake
    /// text view.
    #[must_use]
    pub fn as_text(&self) -> Option<&DomTextNode> {
        match self {
            Self::Document(_) | Self::DocumentFragment(_) | Self::Element(_) => None,
            Self::Text(node) => Some(node),
        }
    }

    #[must_use]
    pub fn intrinsic(&self) -> &DomIntrinsic {
        &self.base().intrinsic
    }

    /// Returns the canonical public snapshot for this node.
    ///
    /// This is the primary public export surface for replaying browser-style
    /// style and layout calculations from one node-owned record.
    #[must_use]
    pub fn snapshot(&self) -> DomCanonicalSnapshot {
        DomCanonicalSnapshot {
            style: self.canonical_style_snapshot(),
            layout: self.canonical_layout_snapshot(),
        }
    }

    /// Returns the canonical public layout variables for this node.
    ///
    /// This surface is limited to intrinsic sizing, available size, and used
    /// size so external callers can replay browser-style layout calculations
    /// without depending on internal node state bundles.
    #[must_use]
    pub fn canonical_layout_snapshot(&self) -> DomCanonicalLayoutSnapshot {
        let layout = self.layout();
        DomCanonicalLayoutSnapshot {
            intrinsic: self.intrinsic().clone(),
            available_width_px: layout.available_width_px,
            available_height_px: layout.available_height_px,
            used_width_px: layout.width_px,
            used_height_px: layout.height_px,
        }
    }

    /// Returns the canonical public style list for this node.
    ///
    /// The returned declaration list is the public authored-style boundary for
    /// this node and does not expose internal style caches or pseudo-state
    /// declarations.
    #[must_use]
    pub fn canonical_style_snapshot(&self) -> DomCanonicalStyleSnapshot {
        let styles = match self {
            Self::Document(node) => node
                .attribute_style_map
                .iter()
                .map(|(attribute, value)| (attribute.clone(), value.clone()))
                .collect(),
            Self::DocumentFragment(node) => node
                .attribute_style_map
                .iter()
                .map(|(attribute, value)| (attribute.clone(), value.clone()))
                .collect(),
            Self::Element(node) => node
                .attribute_style_map
                .iter()
                .map(|(attribute, value)| (attribute.clone(), value.clone()))
                .collect(),
            Self::Text(_) => Vec::new(),
        };
        DomCanonicalStyleSnapshot { styles }
    }

    pub(crate) fn intrinsic_mut(&mut self) -> &mut DomIntrinsic {
        &mut self.base_mut().intrinsic
    }

    #[must_use]
    pub(crate) fn layout(&self) -> &DomLayoutState {
        &self.base().layout
    }

    #[must_use]
    pub(crate) fn layer_order(&self) -> LayerOrder {
        self.base().layout.layer_order
    }

    pub(crate) fn after_create_intrinsic_update(&mut self) -> Result<(), DomError> {
        self.set_intrinsic_for_store(compute_intrinsic_on_create(self)?);
        Ok(())
    }

    #[must_use]
    pub(crate) fn after_attr_write_updates_intrinsic(&self, attribute: &DomAttribute) -> bool {
        match self {
            Self::Document(node) => {
                DomAttributeNode::after_attr_write_updates_intrinsic(node, attribute)
            }
            Self::DocumentFragment(node) => {
                DomAttributeNode::after_attr_write_updates_intrinsic(node, attribute)
            }
            Self::Element(node) => {
                DomAttributeNode::after_attr_write_updates_intrinsic(node, attribute)
            }
            Self::Text(_) => false,
        }
    }

    pub(crate) fn before_attr_write_intrinsic_update(
        &self,
        attribute: &DomAttribute,
        value: Option<&DomAttributeValue>,
    ) -> Result<Option<DomIntrinsic>, DomError> {
        preview_intrinsic_after_attr_write(self, attribute, value)
    }

    pub(crate) fn after_attr_write_intrinsic_update(
        &mut self,
        next_intrinsic: Option<DomIntrinsic>,
    ) -> bool {
        let Some(next_intrinsic) = next_intrinsic else {
            return false;
        };
        self.replace_intrinsic_if_changed(next_intrinsic)
    }

    pub(crate) fn after_style_recompute_intrinsic_update(
        &mut self,
        inherited_font_size_px: f32,
        inherited_line_height_px: Option<f32>,
        inherited_white_space: crate::types::DomWhiteSpace,
        inherited_overflow_wrap: crate::types::DomOverflowWrap,
        inherited_word_break: crate::types::DomWordBreak,
    ) -> bool {
        commit_intrinsic_after_style_recompute(
            self,
            inherited_font_size_px,
            inherited_line_height_px,
            inherited_white_space,
            inherited_overflow_wrap,
            inherited_word_break,
        )
    }

    fn replace_intrinsic_if_changed(&mut self, next_intrinsic: DomIntrinsic) -> bool {
        if self.intrinsic() == &next_intrinsic {
            return false;
        }
        self.set_intrinsic_for_store(next_intrinsic);
        true
    }

    /// Returns whether this node kind can legally own child nodes.
    ///
    /// This is the shared parent-check predicate used by root-owned tree
    /// mutation code. It keeps parent validity explicit without leaking enum
    /// matching throughout the crate.
    #[must_use]
    pub fn is_parent_node(&self) -> bool {
        matches!(
            self,
            Self::Document(_) | Self::DocumentFragment(_) | Self::Element(_)
        )
    }

    /// Sets one node attribute through the enum surface while keeping text
    /// nodes strict about not accepting element attributes.
    pub fn set_attr(
        &mut self,
        attribute: DomAttribute,
        value: Option<DomAttributeValue>,
    ) -> Result<(), DomError> {
        match self {
            Self::Document(node) => DomAttributeNode::set_attr(node, attribute, value),
            Self::DocumentFragment(node) => DomAttributeNode::set_attr(node, attribute, value),
            Self::Element(node) => DomAttributeNode::set_attr(node, attribute, value),
            Self::Text(_) => Err(DomError::InvalidAttributeTarget),
        }
    }

    /// Schedules one property transition on the node so the root can keep
    /// ticking it until the property settles.
    pub fn set_transition(&mut self, transition: DomTransition) {
        self.base_mut()
            .transitions
            .active
            .insert(transition.attribute.clone(), transition);
    }

    pub(crate) fn clear_transition(&mut self, attribute: &DomAttribute) {
        self.base_mut().transitions.active.remove(attribute);
    }

    #[must_use]
    pub(crate) fn active_transition(&self, attribute: &DomAttribute) -> Option<&DomTransition> {
        self.base().transitions.active.get(attribute)
    }

    /// Returns one presented value by preferring the active transition output
    /// over the authored attribute value when that property is animating.
    #[must_use]
    pub fn presented_attr(&self, attribute: &DomAttribute) -> Option<&DomAttributeValue> {
        if let Some(transition) = self.base().transitions.active.get(attribute) {
            return Some(&transition.presented);
        }
        match self {
            Self::Document(node) => stored_attribute_value(&node.attrs, attribute),
            Self::DocumentFragment(node) => stored_attribute_value(&node.attrs, attribute),
            Self::Element(node) => stored_attribute_value(&node.attrs, attribute),
            Self::Text(_) => None,
        }
    }

    pub(crate) fn presented_custom_attr(&self, name: &str) -> Option<&DomAttributeValue> {
        if let Some(value) = self.base().transitions.active.iter().find_map(
            |(attribute, transition)| match attribute {
                DomAttribute::Custom(attribute_name) if attribute_name == name => {
                    Some(&transition.presented)
                }
                _ => None,
            },
        ) {
            return Some(value);
        }
        match self {
            Self::Document(node) => stored_custom_attribute_value(&node.attrs, name),
            Self::DocumentFragment(node) => stored_custom_attribute_value(&node.attrs, name),
            Self::Element(node) => stored_custom_attribute_value(&node.attrs, name),
            Self::Text(_) => None,
        }
    }

    #[must_use]
    pub fn attr(&self, attribute: &DomAttribute) -> Option<&DomAttributeValue> {
        match self {
            Self::Document(node) => stored_attribute_value(&node.attrs, attribute),
            Self::DocumentFragment(node) => stored_attribute_value(&node.attrs, attribute),
            Self::Element(node) => stored_attribute_value(&node.attrs, attribute),
            Self::Text(_) => None,
        }
    }

    /// Returns one parent key without resolving the slot through the root.
    ///
    /// This exposes the stored DOM link directly for callers that need to walk
    /// the tree without immediately resolving the parent node. It keeps link
    /// ownership on the node while leaving lookup authority with the root.
    #[must_use]
    pub fn parent_key(&self) -> Option<DomNodeSlotKey> {
        self.base().parent_key
    }

    /// Marks the node for layout and reports whether layout was already dirty
    /// before this call.
    pub fn mark_layout_dirty(&mut self) -> bool {
        self.mark_layout_axis_dirty(true, true)
    }

    /// Marks one or both layout axes dirty and reports whether layout was
    /// already dirty before this call.
    ///
    /// This is the axis-aware invalidation primitive used by root-owned dirty
    /// propagation. It keeps only the coarse retained layout dirty bit on the
    /// node and treats width/height as transient propagation inputs.
    pub fn mark_layout_axis_dirty(&mut self, _dirty_width: bool, _dirty_height: bool) -> bool {
        let base = self.base_mut();
        let was_dirty = base.layout.dirty;
        base.layout.dirty = true;
        was_dirty
    }

    /// Marks the node for full-layer repaint and reports whether it was
    /// already paint-dirty before this call.
    pub fn mark_full_layer_repaint(&mut self) -> bool {
        let base = self.base_mut();
        let already_dirty = !matches!(base.paint, DomPaintState::None);
        base.paint = DomPaintState::FullLayerRepaint;
        already_dirty
    }

    /// Adds one layer-root-local damage rect to the node repaint state and
    /// reports whether the node was already paint-dirty before this call.
    pub fn mark_damaged_rect_repaint(&mut self, damaged_rect: crate::types::DomRect) -> bool {
        let base = self.base_mut();
        let already_dirty = !matches!(base.paint, DomPaintState::None);
        match &mut base.paint {
            DomPaintState::None => {
                let mut damaged_rects = ArrayVec::new();
                damaged_rects.push(damaged_rect);
                base.paint = DomPaintState::DamagedRectsRepaint(damaged_rects);
            }
            DomPaintState::FullLayerRepaint => {}
            DomPaintState::DamagedRectsRepaint(damaged_rects) => {
                if !coalesce_or_append_damage_rect(damaged_rects, damaged_rect) {
                    base.paint = DomPaintState::FullLayerRepaint;
                }
            }
        }
        already_dirty
    }

    /// Clears one node's repaint state back to the clean retained baseline.
    pub fn clear_paint_repaint_state(&mut self) {
        self.base_mut().paint = DomPaintState::None;
    }

    /// Returns one node's retained repaint state.
    #[must_use]
    pub fn paint_repaint_state(&self) -> &DomPaintState {
        &self.base().paint
    }

    #[must_use]
    pub(crate) fn style_object(&self) -> &DomStyleObject {
        &self.base().style
    }

    /// Marks the node style object dirty and reports whether it was already
    /// dirty before this call.
    pub fn mark_style_dirty(&mut self) -> bool {
        let base = self.base_mut();
        let already_dirty = base.style.dirty;
        base.style.dirty = true;
        already_dirty
    }

    /// Advances node-owned transitions to the provided timestamp and reports
    /// whether the node still needs scheduler ticks afterward.
    pub(crate) fn sync_transitions(&mut self, now_s: f64) -> DomTransitionSyncResult {
        let base = self.base_mut();
        let mut invalidation = AttributeInvalidation::default();
        let mut finished_attributes = Vec::new();
        base.transitions.active.retain(|attribute, transition| {
            if transition.sync(now_s) {
                invalidation = invalidation.merge(attribute_invalidation(attribute));
            }
            if transition.is_finished(now_s) {
                finished_attributes.push(FinishedDomTransition {
                    attribute: attribute.clone(),
                    elapsed_s: transition.effective_end_s() - transition.effective_start_s(),
                });
                return false;
            }
            true
        });
        DomTransitionSyncResult {
            invalidation,
            still_active: !base.transitions.active.is_empty(),
            finished_attributes,
        }
    }

    pub(crate) fn active_transition_attributes(&self) -> impl Iterator<Item = &DomAttribute> + '_ {
        self.base().transitions.active.keys()
    }

    pub(crate) fn dispatched_listeners_for_phase(
        &self,
        event_name: DomEventName,
        capture: bool,
    ) -> Vec<DomDispatchedEventListener> {
        let Some(listeners) = self.base().event_listeners.get(&event_name) else {
            return Vec::new();
        };
        listeners
            .iter()
            .filter(|registration| registration.options.capture == capture)
            .map(|registration| DomDispatchedEventListener {
                listener: registration.listener.clone(),
                options: registration.options,
            })
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn dispatch_event_with_root_for_test(
        &mut self,
        root: &mut crate::types::RootDom,
        dispatch: &mut DomEventDispatchContext,
    ) -> bool {
        let event_name = dispatch.event().name;
        let capture_listeners = self.dispatched_listeners_for_phase(event_name, true);
        let bubble_listeners = self.dispatched_listeners_for_phase(event_name, false);
        invoke_dispatched_listeners_for_test(root, self, dispatch, event_name, &capture_listeners);
        if !dispatch.immediate_propagation_stopped() {
            invoke_dispatched_listeners_for_test(
                root,
                self,
                dispatch,
                event_name,
                &bubble_listeners,
            );
        }
        !dispatch.default_prevented()
    }

    #[cfg(test)]
    pub(crate) fn dispatch_target_listeners_with_root_for_test(
        &mut self,
        root: &mut crate::types::RootDom,
        dispatch: &mut DomEventDispatchContext,
    ) {
        let event_name = dispatch.event().name;
        let capture_listeners = self.dispatched_listeners_for_phase(event_name, true);
        invoke_dispatched_listeners_for_test(root, self, dispatch, event_name, &capture_listeners);
        if dispatch.immediate_propagation_stopped() {
            return;
        }
        let bubble_listeners = self.dispatched_listeners_for_phase(event_name, false);
        invoke_dispatched_listeners_for_test(root, self, dispatch, event_name, &bubble_listeners);
    }

    /// Attaches behavior to one node using DOM-shaped event names while
    /// keeping listener options optional and defaulted.
    pub fn add_event_listener(
        &mut self,
        event_name: DomEventName,
        listener: impl Into<DomEventListener>,
        options: Option<DomEventListenerOptions>,
    ) {
        let listener = listener.into();
        let options = options.unwrap_or_default();
        {
            let listeners = self
                .base_mut()
                .event_listeners
                .entry(event_name)
                .or_default();
            if listeners.iter().any(|registration| {
                event_listener_registration_matches(registration, &listener, options)
            }) {
                return;
            }
            listeners.push(DomEventListenerRegistration { listener, options });
        }
        self.base_mut().exact_listener_name_bits.set(event_name);
    }

    #[must_use]
    pub(crate) fn has_event_listener(
        &self,
        event_name: DomEventName,
        listener: impl Into<DomEventListener>,
        options: Option<DomEventListenerOptions>,
    ) -> bool {
        let listener = listener.into();
        let options = options.unwrap_or_default();
        self.base()
            .event_listeners
            .get(&event_name)
            .is_some_and(|listeners| {
                listeners.iter().any(|registration| {
                    event_listener_registration_matches(registration, &listener, options)
                })
            })
    }

    #[must_use]
    pub(crate) fn has_listener_for_event_name(&self, event_name: DomEventName) -> bool {
        self.base().exact_listener_name_bits.has(event_name)
    }

    /// Detaches node behavior by event name and capture mode, matching the
    /// usual DOM listener removal semantics.
    pub fn remove_event_listener(
        &mut self,
        event_name: DomEventName,
        listener: impl Into<DomEventListener>,
        options: Option<DomEventListenerOptions>,
    ) -> bool {
        let listener = listener.into();
        let options = options.unwrap_or_default();
        let base = self.base_mut();
        let (removed, bucket_empty) = {
            let Some(listeners) = base.event_listeners.get_mut(&event_name) else {
                return false;
            };

            let original_len = listeners.len();
            listeners.retain(|registration| {
                !event_listener_registration_matches(registration, &listener, options)
            });
            (original_len != listeners.len(), listeners.is_empty())
        };
        if !removed {
            return false;
        }
        if bucket_empty {
            base.exact_listener_name_bits
                .sync_event_name_to_event_listener(event_name, &mut base.event_listeners);
        }
        true
    }

    /// Resolves the parent relation while preserving the external slot key
    /// needed for later mutable access at the root boundary.
    pub fn parent_node<'a>(
        &self,
        node_for: impl Fn(DomNodeSlotKey) -> Option<&'a DomNode>,
    ) -> Option<(DomNodeSlotKey, &'a DomNode)> {
        self.base()
            .parent_key
            .and_then(|key| node_for(key).map(|node| (key, node)))
    }

    /// Resolves the first child without exposing a child collection or
    /// inventing a self id on the node.
    pub fn first_child<'a>(
        &self,
        node_for: impl Fn(DomNodeSlotKey) -> Option<&'a DomNode>,
    ) -> Option<(DomNodeSlotKey, &'a DomNode)> {
        self.base()
            .first_child_key
            .and_then(|key| node_for(key).map(|node| (key, node)))
    }

    /// Resolves the last child directly instead of walking the whole sibling
    /// chain.
    pub fn last_child<'a>(
        &self,
        node_for: impl Fn(DomNodeSlotKey) -> Option<&'a DomNode>,
    ) -> Option<(DomNodeSlotKey, &'a DomNode)> {
        self.base()
            .last_child_key
            .and_then(|key| node_for(key).map(|node| (key, node)))
    }

    /// Traverses backward across siblings while keeping the sibling key for
    /// later root-owned mutation.
    pub fn previous_sibling<'a>(
        &self,
        node_for: impl Fn(DomNodeSlotKey) -> Option<&'a DomNode>,
    ) -> Option<(DomNodeSlotKey, &'a DomNode)> {
        self.base()
            .prev_sibling_key
            .and_then(|key| node_for(key).map(|node| (key, node)))
    }

    /// Traverses forward across siblings while keeping the sibling key for
    /// later root-owned mutation.
    pub fn next_sibling<'a>(
        &self,
        node_for: impl Fn(DomNodeSlotKey) -> Option<&'a DomNode>,
    ) -> Option<(DomNodeSlotKey, &'a DomNode)> {
        self.base()
            .next_sibling_key
            .and_then(|key| node_for(key).map(|node| (key, node)))
    }

    /// Provides the minimal non-allocating child-presence check on the linked
    /// child/sibling model.
    #[must_use]
    pub fn has_children(&self) -> bool {
        self.base().first_child_key.is_some()
    }

    /// Provides the DOM-shaped name for `has_children`, so call sites can stay
    /// close to browser terminology.
    #[must_use]
    pub fn has_child_nodes(&self) -> bool {
        self.has_children()
    }

    /// Provides an escape hatch for child traversal without exposing a `Vec`
    /// or iterator surface. This is Vec bait: if callers keep collecting from
    /// it, add a more specific DOM-shaped helper instead.
    pub fn with_children<'a>(
        &self,
        node_for: impl Fn(DomNodeSlotKey) -> Option<&'a DomNode>,
        mut visit: impl FnMut(DomNodeSlotKey, &'a DomNode),
    ) {
        let mut child_key = self.base().first_child_key;
        while let Some(current_key) = child_key {
            let Some(child) = node_for(current_key) else {
                return;
            };
            child_key = child.base().next_sibling_key;
            visit(current_key, child);
        }
    }

    /// Answers the common browser-style "how many children are there" question
    /// without copying the child chain into an owned collection.
    pub fn child_count<'a>(
        &self,
        node_for: impl Fn(DomNodeSlotKey) -> Option<&'a DomNode>,
    ) -> usize {
        let mut count = 0;
        self.with_children(node_for, |_, _| {
            count += 1;
        });
        count
    }

    /// Supports indexed child lookup on top of linked siblings, which is a
    /// common browser implementation need even without storing a `Vec`.
    pub fn child_at<'a>(
        &self,
        index: usize,
        node_for: impl Fn(DomNodeSlotKey) -> Option<&'a DomNode>,
    ) -> Option<(DomNodeSlotKey, &'a DomNode)> {
        let mut current_index = 0;
        let mut found = None;
        self.with_children(node_for, |child_key, child| {
            if found.is_none() && current_index == index {
                found = Some((child_key, child));
            }
            current_index += 1;
        });
        found
    }

    /// Lets ordering logic ask for a specific child's position without first
    /// materializing every child into a temporary collection.
    pub fn index_of<'a>(
        &self,
        child_key: DomNodeSlotKey,
        node_for: impl Fn(DomNodeSlotKey) -> Option<&'a DomNode>,
    ) -> Option<usize> {
        let mut current_index = 0;
        let mut found = None;
        self.with_children(node_for, |current_key, _| {
            if found.is_none() && current_key == child_key {
                found = Some(current_index);
            }
            current_index += 1;
        });
        found
    }

    /// Lets insertion and reordering code ask for the previous child directly
    /// instead of manually walking from the front every time.
    pub fn child_before<'a>(
        &self,
        child_key: DomNodeSlotKey,
        node_for: impl Fn(DomNodeSlotKey) -> Option<&'a DomNode>,
    ) -> Option<(DomNodeSlotKey, &'a DomNode)> {
        let child = node_for(child_key)?;
        let previous_key = child.base().prev_sibling_key?;
        node_for(previous_key).map(|node| (previous_key, node))
    }

    /// Lets insertion and reordering code ask for the next child directly
    /// instead of manually re-deriving sibling adjacency.
    pub fn child_after<'a>(
        &self,
        child_key: DomNodeSlotKey,
        node_for: impl Fn(DomNodeSlotKey) -> Option<&'a DomNode>,
    ) -> Option<(DomNodeSlotKey, &'a DomNode)> {
        let child = node_for(child_key)?;
        let next_key = child.base().next_sibling_key?;
        node_for(next_key).map(|node| (next_key, node))
    }
}

fn coalesce_or_append_damage_rect(
    damaged_rects: &mut ArrayVec<crate::types::DomRect, 8>,
    damaged_rect: crate::types::DomRect,
) -> bool {
    for stored_rect in damaged_rects.iter_mut() {
        if rects_overlap_or_touch(*stored_rect, damaged_rect) {
            *stored_rect = union_rect(*stored_rect, damaged_rect);
            return true;
        }
    }
    if damaged_rects.len() == damaged_rects.capacity() {
        return false;
    }
    damaged_rects.push(damaged_rect);
    true
}

fn rects_overlap_or_touch(left: crate::types::DomRect, right: crate::types::DomRect) -> bool {
    let left_right = left.left_px + left.width_px;
    let left_bottom = left.top_px + left.height_px;
    let right_right = right.left_px + right.width_px;
    let right_bottom = right.top_px + right.height_px;
    left.left_px <= right_right
        && right.left_px <= left_right
        && left.top_px <= right_bottom
        && right.top_px <= left_bottom
}

fn union_rect(left: crate::types::DomRect, right: crate::types::DomRect) -> crate::types::DomRect {
    let union_left = left.left_px.min(right.left_px);
    let union_top = left.top_px.min(right.top_px);
    let union_right = (left.left_px + left.width_px).max(right.left_px + right.width_px);
    let union_bottom = (left.top_px + left.height_px).max(right.top_px + right.height_px);
    crate::types::DomRect {
        left_px: union_left,
        top_px: union_top,
        width_px: (union_right - union_left).max(0.0),
        height_px: (union_bottom - union_top).max(0.0),
    }
}

pub(crate) fn measure_text_intrinsic_from_content(text: &str) -> TextIntrinsic {
    measure_text_intrinsic(text)
}

pub(crate) fn measure_text_intrinsic_from_content_and_font_size(
    text: &str,
    font_size_px: f32,
) -> TextIntrinsic {
    measure_text_intrinsic_with_font_size(text, font_size_px)
}

const DEFAULT_TEXT_CONTROL_COLUMNS: usize = 20;
const DEFAULT_TEXTAREA_ROWS: usize = 2;
const DEFAULT_NUMBER_INPUT_COLUMNS: usize = 10;

fn event_listener_registration_matches(
    registration: &DomEventListenerRegistration,
    listener: &DomEventListener,
    options: DomEventListenerOptions,
) -> bool {
    registration.listener == *listener && registration.options.capture == options.capture
}

#[cfg(test)]
fn invoke_dispatched_listeners_for_test(
    root: &mut crate::types::RootDom,
    node: &mut DomNode,
    dispatch: &mut DomEventDispatchContext,
    event_name: DomEventName,
    listeners: &[DomDispatchedEventListener],
) {
    for dispatched_listener in listeners {
        dispatched_listener.listener.invoke(root, dispatch);
        if dispatched_listener.options.once {
            let _removed = node.remove_event_listener(
                event_name,
                dispatched_listener.listener.clone(),
                Some(dispatched_listener.options),
            );
        }
        if dispatch.immediate_propagation_stopped() {
            return;
        }
    }
}

pub(crate) fn write_attribute(
    attrs: &mut DomAttrs,
    attribute_style_map: &mut BTreeMap<DomAttribute, DomAttributeValue>,
    class_list: &mut Vec<String>,
    attribute: &DomAttribute,
    value: Option<DomAttributeValue>,
) -> Result<(), DomError> {
    let next_attribute_style_map = parse_next_attribute_style_map(attribute, value.as_ref())?;
    let next_class_list = parse_next_class_list(attribute, value.as_ref());

    if *attribute == DomAttribute::Id {
        attrs.id = match value.as_ref() {
            Some(DomAttributeValue::String(value)) => Some(value.clone()),
            None => None,
            _ => unreachable!("validated id attribute must be string or none"),
        };
        match value {
            Some(value) => {
                attrs.attributes.insert(attribute.clone(), value);
            }
            None => {
                attrs.attributes.remove(attribute);
            }
        }
        return Ok(());
    }

    match value {
        Some(DomAttributeValue::String(value))
            if layout_length_attr_uses_unset(attribute) && value == "unset" =>
        {
            attrs.attributes.remove(attribute);
        }
        Some(value) => {
            attrs.attributes.insert(attribute.clone(), value);
        }
        None => {
            attrs.attributes.remove(attribute);
        }
    }

    if let Some(next_attribute_style_map) = next_attribute_style_map {
        *attribute_style_map = next_attribute_style_map;
    }
    if let Some(next_class_list) = next_class_list {
        *class_list = next_class_list;
    }
    Ok(())
}

fn parse_next_attribute_style_map(
    attribute: &DomAttribute,
    value: Option<&DomAttributeValue>,
) -> Result<Option<BTreeMap<DomAttribute, DomAttributeValue>>, DomError> {
    if *attribute != DomAttribute::Style {
        return Ok(None);
    }
    match value {
        None => Ok(Some(BTreeMap::new())),
        Some(DomAttributeValue::String(value)) => parse_style_attr_map(value).map(Some),
        _ => unreachable!("validated style attribute must be string or none"),
    }
}

fn parse_next_class_list(
    attribute: &DomAttribute,
    value: Option<&DomAttributeValue>,
) -> Option<Vec<String>> {
    if *attribute != DomAttribute::Class {
        return None;
    }
    match value {
        None => Some(Vec::new()),
        Some(DomAttributeValue::String(value)) => Some(parse_class_attr_list(value)),
        _ => unreachable!("validated class attribute must be string or none"),
    }
}

fn lowered_image_source_kind_from_attrs(
    attrs: &DomAttrs,
) -> Result<Option<LoweredImageSourceKind>, DomError> {
    let Some(src) = stored_attribute_value(attrs, &DomAttribute::Src) else {
        return Ok(None);
    };
    let DomAttributeValue::String(src) = src else {
        return Err(DomError::InvalidAttributeValue);
    };
    if src.is_empty() {
        return Err(DomError::InvalidAttributeValue);
    }
    let Some(icon_stem) = src.strip_prefix("icon:") else {
        return Ok(Some(LoweredImageSourceKind::External));
    };
    let Some(icon_id) = UiIconId::from_file_stem(icon_stem) else {
        return Err(DomError::InvalidAttributeValue);
    };
    Ok(Some(LoweredImageSourceKind::InternalIcon(icon_id)))
}

pub(crate) fn stored_attribute_value<'a>(
    attrs: &'a DomAttrs,
    attribute: &DomAttribute,
) -> Option<&'a DomAttributeValue> {
    attrs.attributes.get(attribute)
}

fn stored_custom_attribute_value<'a>(
    attrs: &'a DomAttrs,
    name: &str,
) -> Option<&'a DomAttributeValue> {
    attrs
        .attributes
        .iter()
        .find_map(|(attribute, value)| match attribute {
            DomAttribute::Custom(attribute_name) if attribute_name == name => Some(value),
            _ => None,
        })
}

pub(crate) fn attribute_invalidation(attribute: &DomAttribute) -> AttributeInvalidation {
    let layout = attribute_affects_layout(attribute);
    let mut invalidation = AttributeInvalidation::default();
    if layout {
        invalidation = invalidation.with_layout();
        invalidation = invalidation.with_paint();
    } else if attribute_affects_paint(attribute) {
        invalidation = invalidation.with_paint();
    }
    if attribute_affects_style_object(attribute) {
        invalidation = invalidation.with_style();
    }
    if attribute_changes_layer_membership(attribute) {
        invalidation = invalidation.with_layer_membership();
    }
    invalidation
}

pub(crate) fn attribute_affects_layout(attribute: &DomAttribute) -> bool {
    matches!(
        attribute,
        DomAttribute::Display
            | DomAttribute::Overflow
            | DomAttribute::Isolate
            | DomAttribute::Position
            | DomAttribute::FlexDirection
            | DomAttribute::FlexWrap
            | DomAttribute::FlexFlow
            | DomAttribute::FlexGrow
            | DomAttribute::FlexShrink
            | DomAttribute::FlexBasis
            | DomAttribute::Flex
            | DomAttribute::Order
            | DomAttribute::JustifyContent
            | DomAttribute::AlignItems
            | DomAttribute::AlignSelf
            | DomAttribute::AlignContent
            | DomAttribute::Gap
            | DomAttribute::RowGap
            | DomAttribute::ColumnGap
            | DomAttribute::PlaceContent
            | DomAttribute::PlaceItems
            | DomAttribute::PlaceSelf
            | DomAttribute::Width
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
            | DomAttribute::FontSize
            | DomAttribute::LineHeight
            | DomAttribute::WhiteSpace
            | DomAttribute::OverflowWrap
            | DomAttribute::WordBreak
    )
}

pub(crate) fn attribute_affects_paint(attribute: &DomAttribute) -> bool {
    match attribute {
        DomAttribute::Transform
        | DomAttribute::Src
        | DomAttribute::Label
        | DomAttribute::TextAlign
        | DomAttribute::Direction
        | DomAttribute::Value
        | DomAttribute::Checked
        | DomAttribute::Selected
        | DomAttribute::Multiple => true,
        DomAttribute::Custom(name) => matches!(
            name.as_str(),
            "background-color"
                | "color"
                | "border-color"
                | "border-top-color"
                | "border-right-color"
                | "border-bottom-color"
                | "border-left-color"
                | "outline-color"
                | "border-radius"
        ),
        _ => false,
    }
}

pub(crate) fn attribute_affects_style_object(attribute: &DomAttribute) -> bool {
    matches!(
        attribute,
        DomAttribute::Class
            | DomAttribute::Style
            | DomAttribute::LineHeight
            | DomAttribute::TextAlign
            | DomAttribute::Direction
            | DomAttribute::WhiteSpace
            | DomAttribute::OverflowWrap
            | DomAttribute::WordBreak
    )
}

pub(crate) fn attribute_changes_layer_membership(attribute: &DomAttribute) -> bool {
    matches!(
        attribute,
        DomAttribute::Display
            | DomAttribute::Overflow
            | DomAttribute::Transform
            | DomAttribute::Isolate
            | DomAttribute::Position
    )
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use super::{DomNode, DomNodeSlotKey, attribute_invalidation};
    use crate::types::dom_event::{
        DomEvent, DomEventDetail, DomEventDispatchContext, DomEventListenerOptions, DomEventName,
        DomEventPhase, DomModifierState, DomMouseButton, DomMouseEvent,
    };
    use crate::types::dom_tag::{DomElementEditKind, DomTag};
    use crate::types::element_behaviors::{input_default_width_sample, measured_text_width_px};
    use crate::types::{
        DomAttribute, DomAttributeNode, DomAttributeValue, DomElementIntrinsicKind, DomError,
        DomIntrinsic, DomWhiteSpace, parse_css_color_rgba,
    };
    fn assert_f32_eq(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= f32::EPSILON,
            "expected {expected}, got {actual}",
        );
    }

    fn test_owner_document_key() -> DomNodeSlotKey {
        let mut keys = slotmap::SlotMap::with_key();
        keys.insert(())
    }

    fn color(value: &str) -> DomAttributeValue {
        DomAttributeValue::Color(parse_css_color_rgba(value).expect("test color should parse"))
    }

    #[test]
    fn set_attr_does_not_recompute_layer_kind_for_overflow() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Div);
        let original_layer_kind = node
            .as_element()
            .expect("element node")
            .base
            .layout
            .layer_kind;

        node.set_attr(
            DomAttribute::Overflow,
            Some(DomAttributeValue::String("hidden".to_string())),
        )
        .expect("overflow should be accepted");

        let element = node.as_element().expect("element node");
        assert_eq!(element.base.layout.layer_kind, original_layer_kind);
    }

    #[test]
    fn set_attr_does_not_recompute_layer_kind_for_transform() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Div);
        let original_layer_kind = node
            .as_element()
            .expect("element node")
            .base
            .layout
            .layer_kind;

        node.set_attr(
            DomAttribute::Transform,
            Some(DomAttributeValue::String("scale(2)".to_string())),
        )
        .expect("transform should be accepted");

        let element = node.as_element().expect("element node");
        assert_eq!(element.base.layout.layer_kind, original_layer_kind);
    }

    #[test]
    fn checked_attribute_invalidates_paint() {
        let invalidation = attribute_invalidation(&DomAttribute::Checked);
        assert!(invalidation.paint());
        assert!(!invalidation.layout());
        assert!(!invalidation.style());
        assert!(!invalidation.layer_membership());
    }

    #[test]
    fn option_label_attribute_invalidates_paint() {
        let invalidation = attribute_invalidation(&DomAttribute::Label);
        assert!(invalidation.paint());
        assert!(!invalidation.layout());
        assert!(!invalidation.style());
        assert!(!invalidation.layer_membership());
    }

    #[test]
    fn line_height_attribute_invalidates_layout_paint_and_style() {
        let invalidation = attribute_invalidation(&DomAttribute::LineHeight);
        assert!(invalidation.layout());
        assert!(invalidation.paint());
        assert!(invalidation.style());
        assert!(!invalidation.layer_membership());
    }

    #[test]
    fn text_align_attribute_invalidates_paint_and_style_without_layout() {
        let invalidation = attribute_invalidation(&DomAttribute::TextAlign);
        assert!(!invalidation.layout());
        assert!(invalidation.paint());
        assert!(invalidation.style());
        assert!(!invalidation.layer_membership());
    }

    #[test]
    fn direction_attribute_invalidates_paint_and_style_without_layout() {
        let invalidation = attribute_invalidation(&DomAttribute::Direction);
        assert!(!invalidation.layout());
        assert!(invalidation.paint());
        assert!(invalidation.style());
        assert!(!invalidation.layer_membership());
    }

    #[test]
    fn text_wrap_policy_attributes_invalidate_layout_paint_and_style() {
        for attribute in [DomAttribute::OverflowWrap, DomAttribute::WordBreak] {
            let invalidation = attribute_invalidation(&attribute);
            assert!(invalidation.layout());
            assert!(invalidation.paint());
            assert!(invalidation.style());
            assert!(!invalidation.layer_membership());
        }
    }

    #[test]
    fn set_attr_accepts_supported_position_values() {
        for position in ["static", "relative", "absolute", "fixed"] {
            let mut node = DomNode::element(test_owner_document_key(), DomTag::Div);
            node.set_attr(
                DomAttribute::Position,
                Some(DomAttributeValue::String(position.to_string())),
            )
            .expect("position should be accepted");
        }
    }

    #[test]
    fn set_attr_rejects_unknown_position_value() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Div);

        let error = node
            .set_attr(
                DomAttribute::Position,
                Some(DomAttributeValue::String("sticky".to_string())),
            )
            .expect_err("unknown position should fail");

        assert_eq!(error, DomError::InvalidAttributeValue);
    }

    #[test]
    fn set_attr_rejects_non_string_position_value() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Div);

        let error = node
            .set_attr(DomAttribute::Position, Some(DomAttributeValue::Bool(true)))
            .expect_err("non-string position should fail");

        assert_eq!(error, DomError::InvalidAttributeValue);
    }

    #[test]
    fn set_attr_accepts_clearing_position_value() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Div);
        node.set_attr(
            DomAttribute::Position,
            Some(DomAttributeValue::String("relative".to_string())),
        )
        .expect("position should be accepted");

        node.set_attr(DomAttribute::Position, None)
            .expect("clearing position should be accepted");
    }

    #[test]
    fn set_attr_accepts_supported_display_values() {
        for display in [
            "block",
            "inline",
            "inline-block",
            "none",
            "flex",
            "inline-flex",
        ] {
            let mut node = DomNode::element(test_owner_document_key(), DomTag::Div);
            node.set_attr(
                DomAttribute::Display,
                Some(DomAttributeValue::String(display.to_string())),
            )
            .expect("display should be accepted");
        }
    }

    #[test]
    fn set_attr_rejects_unknown_display_value() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Div);

        let error = node
            .set_attr(
                DomAttribute::Display,
                Some(DomAttributeValue::String("grid".to_string())),
            )
            .expect_err("unknown display should fail");

        assert_eq!(error, DomError::InvalidAttributeValue);
    }

    #[test]
    fn pixel_attrs_accept_unset_zero_and_pixel_strings() {
        for attribute in [
            DomAttribute::Width,
            DomAttribute::MinWidth,
            DomAttribute::Top,
            DomAttribute::Right,
        ] {
            let mut node = DomNode::element(test_owner_document_key(), DomTag::Div);
            node.set_attr(
                attribute.clone(),
                Some(DomAttributeValue::String("unset".to_string())),
            )
            .expect("unset should be accepted");
            node.set_attr(
                attribute.clone(),
                Some(DomAttributeValue::String("0".to_string())),
            )
            .expect("zero should be accepted");
            node.set_attr(
                attribute,
                Some(DomAttributeValue::String("-12px".to_string())),
            )
            .expect("pixel string should be accepted");
        }
    }

    #[test]
    fn pixel_attrs_reject_invalid_values() {
        for value in ["12", "12%", "auto", "12p", "px", "--1px", "NaNpx", "infpx"] {
            let mut node = DomNode::element(test_owner_document_key(), DomTag::Div);
            let error = node
                .set_attr(
                    DomAttribute::Width,
                    Some(DomAttributeValue::String(value.to_string())),
                )
                .expect_err("invalid width should fail");
            assert_eq!(error, DomError::InvalidAttributeValue);
        }
    }

    #[test]
    fn pixel_attrs_reject_non_string_values() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Div);
        let error = node
            .set_attr(DomAttribute::Top, Some(DomAttributeValue::Integer(12)))
            .expect_err("non-string top should fail");

        assert_eq!(error, DomError::InvalidAttributeValue);
    }

    #[test]
    fn pixel_attrs_remove_authored_value_on_unset() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Div);
        node.set_attr(
            DomAttribute::Width,
            Some(DomAttributeValue::String("12px".to_string())),
        )
        .expect("width should be accepted");
        node.set_attr(
            DomAttribute::Width,
            Some(DomAttributeValue::String("unset".to_string())),
        )
        .expect("unset should clear width");

        let element = node.as_element().expect("element node");
        assert!(!element.attrs.attributes.contains_key(&DomAttribute::Width));
    }

    #[test]
    fn font_size_attr_accepts_zero_and_pixel_strings() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Div);
        node.set_attr(
            DomAttribute::FontSize,
            Some(DomAttributeValue::String("0".to_string())),
        )
        .expect("zero font-size should be accepted");
        node.set_attr(
            DomAttribute::FontSize,
            Some(DomAttributeValue::String("14px".to_string())),
        )
        .expect("pixel font-size should be accepted");
        node.set_attr(
            DomAttribute::FontSize,
            Some(DomAttributeValue::String("1em".to_string())),
        )
        .expect("em font-size should be accepted");
    }

    #[test]
    fn font_size_attr_rejects_invalid_strings() {
        for value in ["14", "auto", "unset", "NaNpx"] {
            let mut node = DomNode::element(test_owner_document_key(), DomTag::Div);
            let error = node
                .set_attr(
                    DomAttribute::FontSize,
                    Some(DomAttributeValue::String(value.to_string())),
                )
                .expect_err("invalid font-size should fail");
            assert_eq!(error, DomError::InvalidAttributeValue);
        }
    }

    #[test]
    fn dom_node_after_attr_write_updates_intrinsic_comes_from_element_behavior() {
        let img = DomNode::element(test_owner_document_key(), DomTag::Img);
        let input = DomNode::element(test_owner_document_key(), DomTag::Input);
        let textarea = DomNode::element(test_owner_document_key(), DomTag::Textarea);
        let div = DomNode::element(test_owner_document_key(), DomTag::Div);

        assert!(img.after_attr_write_updates_intrinsic(&DomAttribute::Src));
        assert!(img.after_attr_write_updates_intrinsic(&DomAttribute::Width));
        assert!(img.after_attr_write_updates_intrinsic(&DomAttribute::Height));
        assert!(input.after_attr_write_updates_intrinsic(&DomAttribute::Value));
        assert!(input.after_attr_write_updates_intrinsic(&DomAttribute::Placeholder));
        assert!(input.after_attr_write_updates_intrinsic(&DomAttribute::LineHeight));
        assert!(input.after_attr_write_updates_intrinsic(&DomAttribute::Type));
        assert!(textarea.after_attr_write_updates_intrinsic(&DomAttribute::Value));
        assert!(textarea.after_attr_write_updates_intrinsic(&DomAttribute::Placeholder));
        assert!(textarea.after_attr_write_updates_intrinsic(&DomAttribute::LineHeight));
        assert!(!img.after_attr_write_updates_intrinsic(&DomAttribute::FontSize));
        assert!(!input.after_attr_write_updates_intrinsic(&DomAttribute::Width));
        assert!(!textarea.after_attr_write_updates_intrinsic(&DomAttribute::Type));
        assert!(!div.after_attr_write_updates_intrinsic(&DomAttribute::Width));
    }

    #[test]
    fn dom_node_before_attr_write_intrinsic_update_returns_none_without_behavior_match() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Div);
        node.set_attr(
            DomAttribute::Width,
            Some(DomAttributeValue::String("12px".to_string())),
        )
        .expect("width should set");

        let prepared_intrinsic = node
            .before_attr_write_intrinsic_update(
                &DomAttribute::Width,
                Some(&DomAttributeValue::String("12px".to_string())),
            )
            .expect("non-img width should not prepare intrinsic");

        assert_eq!(prepared_intrinsic, None);
        let intrinsic_changed = node.after_attr_write_intrinsic_update(prepared_intrinsic);
        assert!(!intrinsic_changed);
        assert_eq!(node.intrinsic(), &DomIntrinsic::None);
    }

    #[test]
    fn dom_node_before_and_after_attr_write_intrinsic_update_use_img_lifecycle() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Img);
        node.set_attr(
            DomAttribute::Src,
            Some(DomAttributeValue::String("memory://a.png".to_string())),
        )
        .expect("src");
        node.set_attr(
            DomAttribute::Width,
            Some(DomAttributeValue::String("48px".to_string())),
        )
        .expect("width");
        let prepared_intrinsic = node
            .before_attr_write_intrinsic_update(
                &DomAttribute::Height,
                Some(&DomAttributeValue::String("48px".to_string())),
            )
            .expect("img height should prepare intrinsic");
        node.set_attr(
            DomAttribute::Height,
            Some(DomAttributeValue::String("48px".to_string())),
        )
        .expect("height");
        let intrinsic_changed = node.after_attr_write_intrinsic_update(prepared_intrinsic);

        assert!(intrinsic_changed);
        assert!(matches!(node.intrinsic(), DomIntrinsic::Img(_)));
    }

    #[test]
    fn dom_node_before_and_after_attr_write_intrinsic_update_use_input_lifecycle() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Input);
        node.after_create_intrinsic_update()
            .expect("input create intrinsic should measure");
        let before = match node.intrinsic() {
            DomIntrinsic::ElementDefaultPreferredSize(intrinsic) => intrinsic.clone(),
            other => panic!("input should start with preferred-size intrinsic, got {other:?}"),
        };

        let prepared_intrinsic = node
            .before_attr_write_intrinsic_update(
                &DomAttribute::Placeholder,
                Some(&DomAttributeValue::String(
                    "Much wider placeholder than the default sample".to_string(),
                )),
            )
            .expect("input placeholder should prepare intrinsic");
        node.set_attr(
            DomAttribute::Placeholder,
            Some(DomAttributeValue::String(
                "Much wider placeholder than the default sample".to_string(),
            )),
        )
        .expect("placeholder");
        let intrinsic_changed = node.after_attr_write_intrinsic_update(prepared_intrinsic);

        assert!(intrinsic_changed);
        let DomIntrinsic::ElementDefaultPreferredSize(after) = node.intrinsic() else {
            panic!("input should keep preferred-size intrinsic after attr write");
        };
        assert_f32_eq(after.width_px, before.width_px);
        assert_f32_eq(after.font_size_px, before.font_size_px);
        assert_eq!(after.generation, before.generation.wrapping_add(1));
    }

    #[test]
    fn dom_node_before_and_after_attr_write_intrinsic_update_use_input_line_height() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Input);
        node.after_create_intrinsic_update()
            .expect("input create intrinsic should measure");

        let prepared_intrinsic = node
            .before_attr_write_intrinsic_update(
                &DomAttribute::LineHeight,
                Some(&DomAttributeValue::String("2".to_string())),
            )
            .expect("input line-height should prepare intrinsic");
        node.set_attr(
            DomAttribute::LineHeight,
            Some(DomAttributeValue::String("2".to_string())),
        )
        .expect("line-height");
        let intrinsic_changed = node.after_attr_write_intrinsic_update(prepared_intrinsic);

        assert!(intrinsic_changed);
        let DomIntrinsic::ElementDefaultPreferredSize(after) = node.intrinsic() else {
            panic!("input should keep preferred-size intrinsic after line-height write");
        };
        assert_f32_eq(after.line_height_px, 28.0);
        assert_f32_eq(after.height_px, 28.0);
    }

    #[test]
    fn checkbox_input_intrinsic_starts_square_and_non_zero() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Input);
        node.set_attr(
            DomAttribute::Type,
            Some(DomAttributeValue::String("checkbox".to_string())),
        )
        .expect("type");
        node.after_create_intrinsic_update()
            .expect("checkbox create intrinsic should measure");

        let DomIntrinsic::ElementDefaultPreferredSize(intrinsic) = node.intrinsic() else {
            panic!("checkbox should keep default preferred size intrinsic");
        };
        assert!(intrinsic.width_px > 0.0);
        assert_f32_eq(intrinsic.width_px, intrinsic.height_px);
    }

    #[test]
    fn number_input_intrinsic_includes_stepper_width() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Input);
        node.set_attr(
            DomAttribute::Type,
            Some(DomAttributeValue::String("number".to_string())),
        )
        .expect("type");
        node.set_attr(
            DomAttribute::Placeholder,
            Some(DomAttributeValue::String("42".to_string())),
        )
        .expect("placeholder");
        node.after_create_intrinsic_update()
            .expect("number create intrinsic should measure");

        let DomIntrinsic::ElementDefaultPreferredSize(number_intrinsic) = node.intrinsic() else {
            panic!("number input should keep default preferred size intrinsic");
        };
        let text_sample_width_px = measured_text_width_px(
            &input_default_width_sample("number"),
            number_intrinsic.font_size_px,
        );
        assert!(number_intrinsic.width_px > text_sample_width_px);
    }

    #[test]
    fn range_input_intrinsic_stays_slider_shaped() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Input);
        node.set_attr(
            DomAttribute::Type,
            Some(DomAttributeValue::String("range".to_string())),
        )
        .expect("type");
        node.after_create_intrinsic_update()
            .expect("range create intrinsic should measure");

        let DomIntrinsic::ElementDefaultPreferredSize(intrinsic) = node.intrinsic() else {
            panic!("range input should keep default preferred size intrinsic");
        };
        assert!(intrinsic.width_px > intrinsic.height_px);
    }

    #[test]
    fn dom_node_before_attr_write_intrinsic_update_rejects_invalid_img_src() {
        let node = DomNode::element(test_owner_document_key(), DomTag::Img);

        let error = node
            .before_attr_write_intrinsic_update(
                &DomAttribute::Src,
                Some(&DomAttributeValue::String(String::new())),
            )
            .expect_err("before attr write should reject empty img src");

        assert_eq!(error, DomError::InvalidAttributeValue);
    }

    #[test]
    fn dom_node_after_style_recompute_intrinsic_update_remeasures_text() {
        let mut node = DomNode::text(test_owner_document_key(), "hello".into());
        node.after_create_intrinsic_update()
            .expect("text create intrinsic should measure");
        let previous_intrinsic = match node.intrinsic() {
            DomIntrinsic::Text(intrinsic) => intrinsic.clone(),
            other => panic!("text should keep text intrinsic after create, got {other:?}"),
        };

        let intrinsic_changed = node.after_style_recompute_intrinsic_update(
            20.0,
            None,
            DomWhiteSpace::Normal,
            crate::types::DomOverflowWrap::Normal,
            crate::types::DomWordBreak::Normal,
        );

        assert!(intrinsic_changed);
        let DomIntrinsic::Text(next_intrinsic) = node.intrinsic() else {
            panic!("text should keep text intrinsic after style stage");
        };
        assert_f32_eq(next_intrinsic.font_size_px, 20.0);
        assert_eq!(
            next_intrinsic.generation,
            previous_intrinsic.generation.wrapping_add(1)
        );
    }

    #[test]
    fn dom_node_after_style_recompute_intrinsic_update_remeasures_input() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Input);
        node.after_create_intrinsic_update()
            .expect("input create intrinsic should measure");
        let previous_intrinsic = match node.intrinsic() {
            DomIntrinsic::ElementDefaultPreferredSize(intrinsic) => intrinsic.clone(),
            other => panic!("input should start with preferred-size intrinsic, got {other:?}"),
        };

        let intrinsic_changed = node.after_style_recompute_intrinsic_update(
            20.0,
            None,
            DomWhiteSpace::Normal,
            crate::types::DomOverflowWrap::Normal,
            crate::types::DomWordBreak::Normal,
        );

        assert!(intrinsic_changed);
        let DomIntrinsic::ElementDefaultPreferredSize(next_intrinsic) = node.intrinsic() else {
            panic!("input should keep preferred-size intrinsic after style stage");
        };
        assert_f32_eq(next_intrinsic.font_size_px, 20.0);
        assert_eq!(
            next_intrinsic.generation,
            previous_intrinsic.generation.wrapping_add(1)
        );
    }

    #[test]
    fn style_attr_stores_raw_string_and_parsed_style_map() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Div);

        node.set_attr(
            DomAttribute::Style,
            Some(DomAttributeValue::String(
                "width: 12px; background-color: #112233; width: 14px".to_string(),
            )),
        )
        .expect("style should parse");

        let element = node.as_element().expect("element node");
        assert_eq!(
            element.attrs.attributes.get(&DomAttribute::Style),
            Some(&DomAttributeValue::String(
                "width: 12px; background-color: #112233; width: 14px".to_string()
            ))
        );
        assert_eq!(
            element.attribute_style_map().get(&DomAttribute::Width),
            Some(&DomAttributeValue::String("14px".to_string()))
        );
        assert_eq!(
            element
                .attribute_style_map()
                .get(&DomAttribute::Custom("background-color".to_string())),
            Some(&color("#112233"))
        );
        assert_eq!(element.attrs.attributes.get(&DomAttribute::Width), None);
    }

    #[test]
    fn color_attribute_strings_store_normalized_color_values() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Div);

        node.set_attr(
            DomAttribute::Custom("background-color".to_string()),
            Some(DomAttributeValue::String("#112233".to_string())),
        )
        .expect("color string should parse");

        let element = node.as_element().expect("element node");
        assert_eq!(
            element
                .attrs
                .attributes
                .get(&DomAttribute::Custom("background-color".to_string())),
            Some(&color("#112233"))
        );
    }

    #[test]
    fn color_attribute_accepts_direct_color_and_rejects_other_value_types() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Div);
        let attribute = DomAttribute::Custom("color".to_string());

        node.set_attr(attribute.clone(), Some(color("rgba(255, 0, 0, 0.5)")))
            .expect("direct color should store");
        let error = node
            .set_attr(attribute, Some(DomAttributeValue::Float(1.0)))
            .expect_err("non-color payload should fail");

        assert_eq!(error, DomError::InvalidAttributeValue);
    }

    #[test]
    fn invalid_style_attr_leaves_existing_raw_and_parsed_state_unchanged() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Div);
        node.set_attr(
            DomAttribute::Style,
            Some(DomAttributeValue::String("width: 12px".to_string())),
        )
        .expect("style should parse");

        let error = node
            .set_attr(
                DomAttribute::Style,
                Some(DomAttributeValue::String("width 12px".to_string())),
            )
            .expect_err("invalid style should fail");

        assert_eq!(error, DomError::InvalidAttributeValue);
        let element = node.as_element().expect("element node");
        assert_eq!(
            element.attrs.attributes.get(&DomAttribute::Style),
            Some(&DomAttributeValue::String("width: 12px".to_string()))
        );
        assert_eq!(
            element.attribute_style_map().get(&DomAttribute::Width),
            Some(&DomAttributeValue::String("12px".to_string()))
        );
    }

    #[test]
    fn style_attr_rejects_non_style_property_names() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Div);

        let error = node
            .set_attr(
                DomAttribute::Style,
                Some(DomAttributeValue::String("slot: panel".to_string())),
            )
            .expect_err("slot should fail");

        assert_eq!(error, DomError::InvalidAttributeValue);
        let element = node.as_element().expect("element node");
        assert_eq!(element.attrs.attributes.get(&DomAttribute::Style), None);
        assert!(element.attribute_style_map().is_empty());
    }

    #[test]
    fn clearing_style_attr_removes_raw_string_and_parsed_style_map() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Div);
        node.set_attr(
            DomAttribute::Style,
            Some(DomAttributeValue::String("width: 12px".to_string())),
        )
        .expect("style should parse");

        node.set_attr(DomAttribute::Style, None)
            .expect("clearing style should work");

        let element = node.as_element().expect("element node");
        assert_eq!(element.attrs.attributes.get(&DomAttribute::Style), None);
        assert!(element.attribute_style_map().is_empty());
    }

    #[test]
    fn class_attr_stores_raw_string_and_parsedclass_list() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Div);

        node.set_attr(
            DomAttribute::Class,
            Some(DomAttributeValue::String(
                " hero   primary hero  ".to_string(),
            )),
        )
        .expect("class should parse");

        let element = node.as_element().expect("element node");
        assert_eq!(
            element.attrs.attributes.get(&DomAttribute::Class),
            Some(&DomAttributeValue::String(
                " hero   primary hero  ".to_string()
            ))
        );
        assert_eq!(
            element.class_list(),
            [
                "hero".to_string(),
                "primary".to_string(),
                "hero".to_string()
            ]
        );
    }

    #[test]
    fn clearing_class_attr_removes_raw_string_and_parsedclass_list() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Div);
        node.set_attr(
            DomAttribute::Class,
            Some(DomAttributeValue::String("hero primary".to_string())),
        )
        .expect("class should parse");

        node.set_attr(DomAttribute::Class, None)
            .expect("clearing class should work");

        let element = node.as_element().expect("element node");
        assert_eq!(element.attrs.attributes.get(&DomAttribute::Class), None);
        assert!(element.class_list().is_empty());
    }

    #[test]
    fn text_nodes_reject_style_attr() {
        let mut node = DomNode::fixture_text(test_owner_document_key(), "hello");

        let error = node
            .set_attr(
                DomAttribute::Style,
                Some(DomAttributeValue::String("width: 12px".to_string())),
            )
            .expect_err("text nodes should reject style");

        assert_eq!(error, DomError::InvalidAttributeTarget);
    }

    #[test]
    fn text_nodes_reject_class_attr() {
        let mut node = DomNode::fixture_text(test_owner_document_key(), "hello");

        let error = node
            .set_attr(
                DomAttribute::Class,
                Some(DomAttributeValue::String("hero".to_string())),
            )
            .expect_err("text nodes should reject class");

        assert_eq!(error, DomError::InvalidAttributeTarget);
    }

    #[test]
    fn edge_attrs_remove_authored_value_on_unset() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Div);
        node.set_attr(
            DomAttribute::Top,
            Some(DomAttributeValue::String("-12px".to_string())),
        )
        .expect("top should be accepted");
        node.set_attr(
            DomAttribute::Top,
            Some(DomAttributeValue::String("unset".to_string())),
        )
        .expect("unset should clear top");

        let element = node.as_element().expect("element node");
        assert!(!element.attrs.attributes.contains_key(&DomAttribute::Top));
    }

    #[test]
    fn margin_attrs_remove_authored_value_on_unset() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Div);
        node.set_attr(
            DomAttribute::MarginLeft,
            Some(DomAttributeValue::String("12px".to_string())),
        )
        .expect("margin-left should be accepted");
        node.set_attr(
            DomAttribute::MarginLeft,
            Some(DomAttributeValue::String("unset".to_string())),
        )
        .expect("unset should clear margin-left");

        let element = node.as_element().expect("element node");
        assert!(
            !element
                .attrs
                .attributes
                .contains_key(&DomAttribute::MarginLeft)
        );
    }

    #[test]
    fn padding_attrs_remove_authored_value_on_unset() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Div);
        node.set_attr(
            DomAttribute::PaddingTop,
            Some(DomAttributeValue::String("8px".to_string())),
        )
        .expect("padding-top should be accepted");
        node.set_attr(
            DomAttribute::PaddingTop,
            Some(DomAttributeValue::String("unset".to_string())),
        )
        .expect("unset should clear padding-top");

        let element = node.as_element().expect("element node");
        assert!(
            !element
                .attrs
                .attributes
                .contains_key(&DomAttribute::PaddingTop)
        );
    }

    #[test]
    fn border_attrs_remove_authored_value_on_unset() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Div);
        node.set_attr(
            DomAttribute::BorderBottom,
            Some(DomAttributeValue::String("2px".to_string())),
        )
        .expect("border-bottom should be accepted");
        node.set_attr(
            DomAttribute::BorderBottom,
            Some(DomAttributeValue::String("unset".to_string())),
        )
        .expect("unset should clear border-bottom");

        let element = node.as_element().expect("element node");
        assert!(
            !element
                .attrs
                .attributes
                .contains_key(&DomAttribute::BorderBottom)
        );
    }

    #[test]
    fn set_attr_rejects_attributes_on_text_nodes() {
        let mut node = DomNode::fixture_text(test_owner_document_key(), "hello");

        let error = node
            .set_attr(
                DomAttribute::Id,
                Some(DomAttributeValue::String("x".to_string())),
            )
            .expect_err("text nodes should reject attrs");

        assert_eq!(error, DomError::InvalidAttributeTarget);
    }

    #[test]
    fn set_attr_rejects_id_on_document_nodes() {
        let mut node = DomNode::document(test_owner_document_key());

        let error = node
            .set_attr(
                DomAttribute::Id,
                Some(DomAttributeValue::String("doc".to_string())),
            )
            .expect_err("document nodes should reject ids");

        assert_eq!(error, DomError::InvalidAttributeTarget);
    }

    #[test]
    fn set_attr_rejects_id_on_document_fragment_nodes() {
        let mut node = DomNode::document_fragment(test_owner_document_key());

        let error = node
            .set_attr(
                DomAttribute::Id,
                Some(DomAttributeValue::String("fragment".to_string())),
            )
            .expect_err("document fragment nodes should reject ids");

        assert_eq!(error, DomError::InvalidAttributeTarget);
    }

    #[test]
    fn input_nodes_start_with_input_defaults() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Input);
        node.after_create_intrinsic_update()
            .expect("input create intrinsic should measure");
        let element = node.as_element().expect("element node");

        assert_eq!(
            element.attrs.attributes.get(&DomAttribute::Type),
            Some(&DomAttributeValue::String("text".to_string()))
        );
        assert_eq!(
            element.attrs.attributes.get(&DomAttribute::Display),
            Some(&DomAttributeValue::String("inline-block".to_string()))
        );
        assert!(element.behavior().focusable);
        assert_eq!(element.behavior().edit_kind, DomElementEditKind::SingleLine);
        assert_eq!(
            element.behavior().intrinsic_kind,
            DomElementIntrinsicKind::ElementDefaultPreferredSize
        );
        assert!(matches!(
            node.intrinsic(),
            DomIntrinsic::ElementDefaultPreferredSize(_)
        ));
    }

    #[test]
    fn owner_document_key_returns_creator_root_key() {
        let owner_document_key = test_owner_document_key();
        let node = DomNode::element(owner_document_key, DomTag::Button);

        assert_eq!(node.owner_document_key(), owner_document_key);
    }

    #[test]
    fn dispatch_event_keeps_unreached_once_listener_registered() {
        let mut root = crate::types::RootDom::new(crate::types::RootDomContext::new_cpu());
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Button);
        STOP_CAPTURE_CALLS.store(0, Ordering::SeqCst);
        ONCE_BUBBLE_CALLS.store(0, Ordering::SeqCst);
        node.add_event_listener(
            DomEventName::Click,
            stop_capture_listener,
            Some(DomEventListenerOptions {
                capture: true,
                once: false,
                passive: false,
            }),
        );
        node.add_event_listener(
            DomEventName::Click,
            once_bubble_listener,
            Some(DomEventListenerOptions {
                capture: false,
                once: true,
                passive: false,
            }),
        );
        node.add_event_listener(
            DomEventName::Click,
            once_bubble_listener,
            Some(DomEventListenerOptions {
                capture: false,
                once: true,
                passive: false,
            }),
        );

        let first_event = click_event();
        let mut dispatch = DomEventDispatchContext::new(first_event);
        dispatch.enter_phase(DomEventPhase::AtTarget, test_owner_document_key());
        node.dispatch_event_with_root_for_test(&mut root, &mut dispatch);
        assert_eq!(STOP_CAPTURE_CALLS.load(Ordering::SeqCst), 1);
        assert_eq!(ONCE_BUBBLE_CALLS.load(Ordering::SeqCst), 0);

        let removed = node.remove_event_listener(
            DomEventName::Click,
            once_bubble_listener,
            Some(DomEventListenerOptions {
                capture: false,
                once: true,
                passive: false,
            }),
        );

        assert!(removed);
    }

    #[test]
    fn add_event_listener_sets_exact_listener_name_bit_once() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Button);

        assert!(!node.has_listener_for_event_name(DomEventName::Click));

        node.add_event_listener(DomEventName::Click, once_bubble_listener, None);
        assert!(node.has_listener_for_event_name(DomEventName::Click));

        node.add_event_listener(DomEventName::Click, once_bubble_listener, None);
        assert!(node.has_listener_for_event_name(DomEventName::Click));
    }

    #[test]
    fn remove_event_listener_clears_exact_listener_name_bit_after_last_listener() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Button);
        node.add_event_listener(
            DomEventName::Click,
            stop_capture_listener,
            Some(DomEventListenerOptions {
                capture: true,
                once: false,
                passive: false,
            }),
        );
        node.add_event_listener(DomEventName::Click, once_bubble_listener, None);

        assert!(node.has_listener_for_event_name(DomEventName::Click));

        assert!(node.remove_event_listener(
            DomEventName::Click,
            stop_capture_listener,
            Some(DomEventListenerOptions {
                capture: true,
                once: false,
                passive: false,
            }),
        ));
        assert!(node.has_listener_for_event_name(DomEventName::Click));

        assert!(node.remove_event_listener(DomEventName::Click, once_bubble_listener, None));
        assert!(!node.has_listener_for_event_name(DomEventName::Click));
    }

    #[test]
    fn dispatch_event_clears_exact_listener_name_bit_after_once_listener_runs() {
        let mut root = crate::types::RootDom::new(crate::types::RootDomContext::new_cpu());
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Button);
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_listener = calls.clone();
        node.add_event_listener(
            DomEventName::Click,
            crate::types::dom_event::DomEventListener::new(move |_| {
                calls_for_listener.fetch_add(1, Ordering::SeqCst);
            }),
            Some(DomEventListenerOptions {
                capture: false,
                once: true,
                passive: false,
            }),
        );

        let mut dispatch = DomEventDispatchContext::new(click_event());
        dispatch.enter_phase(DomEventPhase::AtTarget, test_owner_document_key());
        node.dispatch_event_with_root_for_test(&mut root, &mut dispatch);

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!node.has_listener_for_event_name(DomEventName::Click));

        let mut second_dispatch = DomEventDispatchContext::new(click_event());
        second_dispatch.enter_phase(DomEventPhase::AtTarget, test_owner_document_key());
        node.dispatch_event_with_root_for_test(&mut root, &mut second_dispatch);

        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn dispatch_target_listeners_clears_exact_listener_name_bit_after_once_listener_runs() {
        let mut root = crate::types::RootDom::new(crate::types::RootDomContext::new_cpu());
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Button);
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_listener = calls.clone();
        node.add_event_listener(
            DomEventName::Click,
            crate::types::dom_event::DomEventListener::new(move |_| {
                calls_for_listener.fetch_add(1, Ordering::SeqCst);
            }),
            Some(DomEventListenerOptions {
                capture: false,
                once: true,
                passive: false,
            }),
        );

        let mut dispatch = DomEventDispatchContext::new(click_event());
        dispatch.enter_phase(DomEventPhase::AtTarget, test_owner_document_key());
        node.dispatch_target_listeners_with_root_for_test(&mut root, &mut dispatch);

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!node.has_listener_for_event_name(DomEventName::Click));
    }

    #[test]
    fn snapshot_uses_public_browser_style_variables() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Div);
        node.base_mut().intrinsic = DomIntrinsic::Text(crate::types::TextIntrinsic {
            font_size_px: 16.0,
            min_content_width_px: 24.0,
            max_content_width_px: 48.0,
            line_height_px: 20.0,
            generation: 3,
        });
        node.base_mut().layout.available_width_px = 320.0;
        node.base_mut().layout.available_height_px = 180.0;
        node.base_mut().layout.width_px = 120.0;
        node.base_mut().layout.height_px = 40.0;

        let snapshot = node.snapshot();

        assert!(snapshot.style.styles.is_empty());
        assert_eq!(snapshot.layout.intrinsic, *node.intrinsic());
        assert_eq!(snapshot.layout.available_width_px, 320.0);
        assert_eq!(snapshot.layout.available_height_px, 180.0);
        assert_eq!(snapshot.layout.used_width_px, 120.0);
        assert_eq!(snapshot.layout.used_height_px, 40.0);

        let encoded =
            serde_json::to_string(&snapshot).expect("canonical node snapshot should serialize");
        let decoded: crate::types::DomCanonicalSnapshot =
            serde_json::from_str(&encoded).expect("canonical node snapshot should deserialize");
        assert_eq!(decoded, snapshot);
    }

    #[test]
    fn snapshot_uses_the_public_style_list_only() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Div);
        node.set_attr(
            DomAttribute::Style,
            Some(DomAttributeValue::String(
                "width: 12px; color: #ffffff; width: 14px".to_string(),
            )),
        )
        .expect("style should parse");

        let snapshot = node.snapshot();

        assert_eq!(
            snapshot.style.styles,
            vec![
                (
                    DomAttribute::Width,
                    DomAttributeValue::String("14px".to_string()),
                ),
                (DomAttribute::Custom("color".to_string()), color("#ffffff")),
            ]
        );
        assert_eq!(
            snapshot.layout,
            crate::types::DomCanonicalLayoutSnapshot::default()
        );

        let encoded = snapshot
            .to_json()
            .expect("canonical node snapshot should serialize");
        let decoded: crate::types::DomCanonicalSnapshot =
            serde_json::from_str(&encoded).expect("canonical node snapshot should deserialize");
        assert_eq!(decoded, snapshot);
    }

    fn click_event() -> DomEvent {
        DomEvent::new(
            DomEventName::Click,
            DomEventDetail::Mouse(DomMouseEvent {
                button: Some(DomMouseButton::Primary),
                buttons: 1,
                client_x: 0.0,
                client_y: 0.0,
                modifiers: DomModifierState::default(),
            }),
        )
    }

    static STOP_CAPTURE_CALLS: AtomicUsize = AtomicUsize::new(0);
    static ONCE_BUBBLE_CALLS: AtomicUsize = AtomicUsize::new(0);

    fn stop_capture_listener(context: &mut crate::types::DomEventListenerContext<'_>) {
        STOP_CAPTURE_CALLS.fetch_add(1, Ordering::SeqCst);
        context.stop_immediate_propagation();
    }

    fn once_bubble_listener(_: &mut crate::types::DomEventListenerContext<'_>) {
        ONCE_BUBBLE_CALLS.fetch_add(1, Ordering::SeqCst);
    }
}
