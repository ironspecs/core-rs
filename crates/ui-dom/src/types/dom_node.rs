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
use std::sync::Arc;

use crate::domains::paint::{measure_text_intrinsic, measure_text_intrinsic_with_font_size};
use crate::domains::style::computed::project::{
    overflow_wrap as overflow_wrap_from_keyword, text_align as text_align_from_keyword,
    text_direction as text_direction_from_keyword, white_space as white_space_from_keyword,
    word_break as word_break_from_keyword,
};
use crate::domains::style::computed::{
    AlignContentKw, AlignItemsKw, BoxLength, BoxSizingKw, ComputedBoxGroup, ComputedColorGroup,
    ComputedFlexGroup, ComputedFontGroup, ComputedGroups, ComputedTextGroup,
    ComputedTransformGroup, ComputedVisualGroup, DisplayKw, FlexBasisKw, FlexDirectionKw,
    FlexWrapKw, GroupDirtyClassification, JustifyContentKw, OverflowKw, PositionKw,
    ResolvedTransitionLonghands,
};
use crate::domains::style::{DeclBlockKey, DeclLength, DeclValue, Keyword, PresentationProperty};
#[cfg(test)]
use crate::domains::text_layout::ResolvedTextStyle;
pub(crate) use crate::types::attribute_invalidation::AttributeInvalidation;
use crate::types::dom_attr_parse::parse_class_attr_list;
pub use crate::types::dom_document_fragment_node::DomDocumentFragmentNode;
pub use crate::types::dom_document_node::DomDocumentNode;
pub use crate::types::dom_element_node::DomElementNode;
#[cfg(test)]
use crate::types::dom_event::DomEventDispatchContext;
use crate::types::dom_event::{DomEventListener, DomEventListenerOptions, DomEventName};
use crate::types::dom_node_base::DomNodeBase;
pub use crate::types::dom_paint_state::DomPaintState;
pub use crate::types::dom_style_object::DomStyleObject;
use crate::types::dom_tag::DomTag;
pub use crate::types::dom_text_node::DomTextNode;
use crate::types::dom_transition::DomTransition;
use crate::types::element_behaviors::{
    commit_intrinsic_after_style_recompute, compute_intrinsic_on_create,
    preview_intrinsic_after_attr_write,
};
use crate::types::{
    DomAttribute, DomAttributeNode, DomAttributeValue, DomAttrs, DomCanonicalLayoutSnapshot,
    DomError, DomOverflowWrap, DomTextAlign, DomTextDirection, DomWhiteSpace, DomWordBreak,
};
use arrayvec::ArrayVec;
use serde::{Deserialize, Serialize};
use slotmap::new_key_type;
use ui_icons::UiIconId;

#[cfg(test)]
use crate::types::dom_event::{
    DomEvent, DomEventDetail, DomEventPhase, DomModifierState, DomMouseButton, DomMouseEvent,
};
#[cfg(test)]
use crate::types::element_behaviors::{input_default_width_sample, measured_text_width_px};
#[cfg(test)]
use crate::types::{DomEventListenerContext, RootDom, RootDomContext, parse_css_color_rgba};

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

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct TextIntrinsic {
    /// Resolved font size used when this intrinsic was measured.
    pub font_size_px: f32,
    pub min_content_width_px: f32,
    pub max_content_width_px: f32,
    pub line_height_px: f32,
    /// Width of the first unbreakable fragment (up to the first soft-wrap
    /// opportunity); equals min-content when the text is a single fragment.
    pub leading_fragment_width_px: f32,
    /// Width of the last unbreakable fragment.
    pub trailing_fragment_width_px: f32,
    /// The text begins with a collapsible whitespace/segment-break character
    /// in a collapse white-space mode, so a preceding run's trailing fragment
    /// does not join across this boundary (the boundary itself is a break).
    pub starts_with_wrap_opportunity: bool,
    /// The text ends with a collapsible whitespace/segment-break character in
    /// a collapse white-space mode (a following run's leading fragment does
    /// not join across this boundary).
    pub ends_with_wrap_opportunity: bool,
    /// Maximum line-height over the text's fragments (the run's strut cross
    /// size). Equals `line_height_px` for today's single-style text; kept as a
    /// forward-compatible shape for heterogeneous-font runs.
    pub fragment_line_height_max_px: f32,
    /// Min-content block size (CSS Sizing §5.1, flex arc step 4): the
    /// min-content buffer's run count × line height. The run count IS the
    /// fragment count (mode-aware; see `TextIntrinsicMetrics::v_min_content_px`).
    pub v_min_content_px: f32,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct ImgIntrinsic {
    pub width_px: f32,
    pub height_px: f32,
    pub aspect_ratio: f32,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LoweredImageSourceKind {
    InternalIcon(UiIconId),
    External,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DomNode {
    Document(DomDocumentNode),
    DocumentFragment(DomDocumentFragmentNode),
    Element(DomElementNode),
    Text(DomTextNode),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DomEventListenerRegistration {
    listener: DomEventListener,
    options: DomEventListenerOptions,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DomDispatchedEventListener {
    pub(crate) listener: DomEventListener,
    pub(crate) options: DomEventListenerOptions,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct DomTransitionSyncResult {
    pub invalidation: GroupDirtyClassification,
    pub inherited_progress: bool,
    pub still_active: bool,
    pub finished_properties: Vec<FinishedDomTransition>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FinishedDomTransition {
    pub property: PresentationProperty,
    pub elapsed_s: f64,
}

pub(crate) fn validate_document_like_attr_target(attribute: &DomAttribute) -> Result<(), DomError> {
    if *attribute == DomAttribute::Id {
        return Err(DomError::InvalidAttributeTarget);
    }
    Ok(())
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

    /// The node's stored seven computed groups, without the transition-overlay
    /// merge (spine 3.3d). This is the transition-free projection output: the
    /// write path and style pass compute it from stored attrs only. The
    /// inheritance seed now reads the *merged* accessors (`computed_*`) instead
    /// of this (CSS Cascade §6.1 — the transition tier wins the cascade, so the
    /// interpolated values are the cascaded values and inherit). Text nodes are
    /// never projected, so they return `ComputedGroups::initial()` here.
    #[must_use]
    pub(crate) fn stored_computed_groups(&self) -> ComputedGroups {
        self.base().presentation.computed
    }

    /// The node's computed groups as of the last *completed* style pass, or
    /// `None` before the first pass (spine 3.3g-1b). The pass diffs this
    /// against the freshly resolved groups to schedule transitions and classify
    /// dirtiness (D2 one-timing); the write path re-projects `computed` without
    /// touching this, so the last-pass value survives a direct write.
    #[must_use]
    pub(crate) fn previous_computed(&self) -> Option<ComputedGroups> {
        self.base().presentation.previous_computed
    }

    /// Records the freshly resolved groups as the last-pass baseline (spine
    /// 3.3g-1b). Called by the style pass after resolving the seven groups.
    #[allow(clippy::large_types_passed_by_value)] // `ComputedGroups` is Copy; by-value is deliberate
    pub(crate) fn set_previous_computed(&mut self, groups: ComputedGroups) {
        self.base_mut().presentation.previous_computed = Some(groups);
    }

    /// The node's resolved transition-longhand scheduler slot (S1.1), written
    /// by the pass's projection. The pass clones it before scheduling so it can
    /// drop the node borrow while mutating the scheduler.
    #[must_use]
    pub(crate) fn resolved_transition_longhands(&self) -> &ResolvedTransitionLonghands {
        &self.base().presentation.transition_longhands
    }

    /// The node's inline declaration block key (S1.1; the single source of the
    /// css-style-attr store), or `None` when the node's inline `style` was
    /// never written. The block lives and frees with the node: the store
    /// mutators write this key through the root and `remove_node` frees the
    /// block it names.
    #[must_use]
    pub(crate) fn inline_decl(&self) -> Option<DeclBlockKey> {
        self.base().presentation.decl
    }

    /// Records the node's inline declaration block key (S1.1). Only the
    /// root-owned store mutators call this, so the key always names a live
    /// block in the document's inline store.
    pub(crate) fn set_inline_decl(&mut self, key: Option<DeclBlockKey>) {
        self.base_mut().presentation.decl = key;
    }

    /// Writes the freshly resolved seven groups and transition-longhand slot
    /// (spine 3.3g-2a): the typed winners fold resolves and writes these
    /// directly on the node record — no attr-projection channel.
    #[allow(clippy::large_types_passed_by_value)] // Copy data; the by-value groups are deliberate
    pub(crate) fn set_computed_groups(
        &mut self,
        groups: ComputedGroups,
        longhands: ResolvedTransitionLonghands,
    ) {
        let presentation = &mut self.base_mut().presentation;
        presentation.computed = groups;
        presentation.transition_longhands = longhands;
    }

    /// The seven internal computed-group reads (PLAN.md spine 3.3b).
    ///
    /// These are the visit-local reads the layout/paint consumers use instead
    /// of re-parsing stringly presented attrs. Each returns the node's stored
    /// group *copy* (groups are `Copy`), then merges the active-transition
    /// overlay for its group's transitionable properties (the transition
    /// scheduler's presented value wins where a transition is running — CSS
    /// Transitions tier, above computed style).
    ///
    /// The merge only *converts* the transition's already-interpolated
    /// `presented` value into the field's type; it never interpolates. A
    /// `presented` `DeclValue` kind that cannot apply to a field (e.g. an `em`
    /// length where a resolved `f32` is expected) falls back to the stored
    /// group field — transition interpolation never produces those shapes for
    /// the transitionable set.
    #[must_use]
    pub fn computed_font(&self) -> ComputedFontGroup {
        let mut font = self.base().presentation.computed.font;
        if let Some(size_px) = self.presented_font_size() {
            font.size_px = size_px;
        }
        apply_optional_px(
            &mut font.line_height_px,
            self.presented_line_height(font.size_px),
        );
        if let Some(weight) = self.presented_font_weight() {
            font.weight = weight;
        }
        if let Some(style) = self.presented_font_style() {
            font.style = style;
        }
        // `family` is an interned `TextKey`; the overlay cannot re-intern a
        // presented string without the document interner, so it falls back to
        // the stored group value (family transitions are not a real path).
        font
    }

    #[must_use]
    pub fn computed_text(&self) -> ComputedTextGroup {
        let mut text = self.base().presentation.computed.text;
        if let Some(align) = self.presented_text_align() {
            text.align = align;
        }
        if let Some(direction) = self.presented_text_direction() {
            text.direction = direction;
        }
        if let Some(white_space) = self.presented_white_space() {
            text.white_space = white_space;
        }
        if let Some(overflow_wrap) = self.presented_overflow_wrap() {
            text.overflow_wrap = overflow_wrap;
        }
        if let Some(word_break) = self.presented_word_break() {
            text.word_break = word_break;
        }
        text
    }

    #[must_use]
    pub fn computed_color(&self) -> ComputedColorGroup {
        let mut color = self.base().presentation.computed.color;
        if let Some(rgba) = self.presented_rgba(PresentationProperty::Color) {
            color.color_rgba = rgba;
        }
        color
    }

    #[must_use]
    #[allow(clippy::too_many_lines)] // mechanical field-by-field overlay merge, low complexity
    pub fn computed_box(&self) -> ComputedBoxGroup {
        let mut box_ = self.base().presentation.computed.box_;
        if let Some(keyword) = self.presented_keyword(PresentationProperty::Display)
            && let Some(display) = DisplayKw::from_keyword(keyword)
        {
            box_.display = display;
        }
        if let Some(keyword) = self.presented_keyword(PresentationProperty::Position)
            && let Some(position) = PositionKw::from_keyword(keyword)
        {
            box_.position = position;
        }
        if let Some(keyword) = self.presented_keyword(PresentationProperty::Overflow)
            && let Some(overflow) = OverflowKw::from_keyword(keyword)
        {
            box_.overflow = overflow;
        }
        if let Some(isolate) = self.presented_isolate() {
            box_.isolate = isolate;
        }
        if let Some(keyword) = self.presented_keyword(PresentationProperty::BoxSizing)
            && let Some(box_sizing) = BoxSizingKw::from_keyword(keyword)
        {
            box_.box_sizing = box_sizing;
        }
        apply_box_length(
            &mut box_.width,
            self.presented_box_length(PresentationProperty::Width),
        );
        apply_box_length(
            &mut box_.height,
            self.presented_box_length(PresentationProperty::Height),
        );
        apply_box_length(
            &mut box_.min_width,
            self.presented_box_length(PresentationProperty::MinWidth),
        );
        apply_box_length(
            &mut box_.min_height,
            self.presented_box_length(PresentationProperty::MinHeight),
        );
        apply_box_length(
            &mut box_.max_width,
            self.presented_box_length(PresentationProperty::MaxWidth),
        );
        apply_box_length(
            &mut box_.max_height,
            self.presented_box_length(PresentationProperty::MaxHeight),
        );
        apply_box_length(
            &mut box_.top,
            self.presented_box_length(PresentationProperty::Top),
        );
        apply_box_length(
            &mut box_.left,
            self.presented_box_length(PresentationProperty::Left),
        );
        apply_box_length(
            &mut box_.right,
            self.presented_box_length(PresentationProperty::Right),
        );
        apply_box_length(
            &mut box_.bottom,
            self.presented_box_length(PresentationProperty::Bottom),
        );
        apply_box_length_value(
            &mut box_.margin[0],
            self.presented_margin_edge(PresentationProperty::MarginTop),
        );
        apply_box_length_value(
            &mut box_.margin[1],
            self.presented_margin_edge(PresentationProperty::MarginRight),
        );
        apply_box_length_value(
            &mut box_.margin[2],
            self.presented_margin_edge(PresentationProperty::MarginBottom),
        );
        apply_box_length_value(
            &mut box_.margin[3],
            self.presented_margin_edge(PresentationProperty::MarginLeft),
        );
        apply_box_length_value(
            &mut box_.padding[0],
            self.presented_padding_edge(PresentationProperty::PaddingTop),
        );
        apply_box_length_value(
            &mut box_.padding[1],
            self.presented_padding_edge(PresentationProperty::PaddingRight),
        );
        apply_box_length_value(
            &mut box_.padding[2],
            self.presented_padding_edge(PresentationProperty::PaddingBottom),
        );
        apply_box_length_value(
            &mut box_.padding[3],
            self.presented_padding_edge(PresentationProperty::PaddingLeft),
        );
        apply_px(
            &mut box_.border_width[0],
            self.presented_px(PresentationProperty::BorderTopWidth),
        );
        apply_px(
            &mut box_.border_width[1],
            self.presented_px(PresentationProperty::BorderRightWidth),
        );
        apply_px(
            &mut box_.border_width[2],
            self.presented_px(PresentationProperty::BorderBottomWidth),
        );
        apply_px(
            &mut box_.border_width[3],
            self.presented_px(PresentationProperty::BorderLeftWidth),
        );
        box_
    }

    #[must_use]
    pub fn computed_flex(&self) -> ComputedFlexGroup {
        let mut flex = self.base().presentation.computed.flex;
        if let Some(keyword) = self.presented_keyword(PresentationProperty::FlexDirection)
            && let Some(direction) = FlexDirectionKw::from_keyword(keyword)
        {
            flex.direction = direction;
        }
        if let Some(keyword) = self.presented_keyword(PresentationProperty::FlexWrap)
            && let Some(wrap) = FlexWrapKw::from_keyword(keyword)
        {
            flex.wrap = wrap;
        }
        if let Some(grow) = self.presented_non_negative_number(PresentationProperty::FlexGrow) {
            flex.grow = grow;
        }
        if let Some(shrink) = self.presented_non_negative_number(PresentationProperty::FlexShrink) {
            flex.shrink = shrink;
        }
        if let Some(basis) = self.presented_flex_basis() {
            flex.basis = basis;
        }
        if let Some(order) = self.presented_order() {
            flex.order = order;
        }
        if let Some(keyword) = self.presented_keyword(PresentationProperty::JustifyContent)
            && let Some(justify) = JustifyContentKw::from_keyword(keyword)
        {
            flex.justify_content = justify;
        }
        if let Some(keyword) = self.presented_keyword(PresentationProperty::AlignItems)
            && let Some(align) = AlignItemsKw::from_keyword(keyword)
        {
            flex.align_items = align;
        }
        if let Some(keyword) = self.presented_keyword(PresentationProperty::AlignSelf) {
            flex.align_self = if keyword == Keyword::Auto {
                None
            } else {
                AlignItemsKw::from_keyword(keyword)
            };
        }
        if let Some(keyword) = self.presented_keyword(PresentationProperty::AlignContent)
            && let Some(align) = AlignContentKw::from_keyword(keyword)
        {
            flex.align_content = align;
        }
        apply_px(
            &mut flex.gap[0],
            self.presented_px(PresentationProperty::RowGap),
        );
        apply_px(
            &mut flex.gap[1],
            self.presented_px(PresentationProperty::ColumnGap),
        );
        flex
    }

    #[must_use]
    pub fn computed_visual(&self) -> ComputedVisualGroup {
        let mut visual = self.base().presentation.computed.visual;
        if let Some(rgba) = self.presented_rgba(PresentationProperty::BackgroundColor) {
            visual.background_rgba = rgba;
        }
        if let Some(rgba) = self.presented_rgba(PresentationProperty::BorderTopColor) {
            visual.border_rgba[0] = rgba;
        }
        if let Some(rgba) = self.presented_rgba(PresentationProperty::BorderRightColor) {
            visual.border_rgba[1] = rgba;
        }
        if let Some(rgba) = self.presented_rgba(PresentationProperty::BorderBottomColor) {
            visual.border_rgba[2] = rgba;
        }
        if let Some(rgba) = self.presented_rgba(PresentationProperty::BorderLeftColor) {
            visual.border_rgba[3] = rgba;
        }
        if let Some(rgba) = self.presented_rgba(PresentationProperty::OutlineColor) {
            visual.outline_rgba = rgba;
        }
        if let Some(rgba) = self.presented_rgba(PresentationProperty::CaretColor) {
            visual.caret_rgba = Some(rgba);
        }
        if let Some(opacity) = self.presented_opacity() {
            visual.opacity = opacity;
        }
        apply_px(
            &mut visual.outline_width,
            self.presented_px(PresentationProperty::OutlineWidth),
        );
        apply_px(
            &mut visual.outline_offset,
            self.presented_px(PresentationProperty::OutlineOffset),
        );
        apply_px(
            &mut visual.radius[0],
            self.presented_px(PresentationProperty::BorderTopLeftRadius),
        );
        apply_px(
            &mut visual.radius[1],
            self.presented_px(PresentationProperty::BorderTopRightRadius),
        );
        apply_px(
            &mut visual.radius[2],
            self.presented_px(PresentationProperty::BorderBottomRightRadius),
        );
        apply_px(
            &mut visual.radius[3],
            self.presented_px(PresentationProperty::BorderBottomLeftRadius),
        );
        visual
    }

    #[must_use]
    pub fn computed_transform(&self) -> ComputedTransformGroup {
        let mut transform = self.base().presentation.computed.transform;
        if let Some((translate_px, has_transform)) = self.presented_transform() {
            transform.translate_px = translate_px;
            transform.has_transform = has_transform;
        }
        transform
    }

    /// The active transition's presented value for `property`, if any.
    ///
    /// This is the single lookup the overlay merge keys on (spine 3.3e:
    /// re-keyed from `DomAttribute` to `PresentationProperty`).
    fn active_presented(&self, property: PresentationProperty) -> Option<&DeclValue> {
        self.base()
            .transitions
            .active
            .get(&property)
            .map(|transition| &transition.presented)
    }

    /// Converts a presented value to a resolved px length for a plain-`f32`
    /// field (`margin`/`padding`/`border` edges, gap, radius, outline).
    /// `None` = not convertible (leave the stored field).
    fn presented_px(&self, property: PresentationProperty) -> Option<f32> {
        match self.active_presented(property)? {
            DeclValue::Length(DeclLength::Px(value)) => Some(*value),
            DeclValue::Length(DeclLength::Unset) => Some(0.0),
            DeclValue::Length(DeclLength::Em(_) | DeclLength::Percent(_))
            | DeclValue::Color(_)
            | DeclValue::Keyword(_)
            | DeclValue::Number(_)
            | DeclValue::Integer(_)
            | DeclValue::Text(_)
            | DeclValue::Translate(_)
            | DeclValue::TrackList(_)
            | DeclValue::GridAreas(_)
            | DeclValue::TrackSize(_)
            | DeclValue::GridAutoFlow(_)
            | DeclValue::GridLine(_)
            | DeclValue::FamilyList(_) => None,
        }
    }

    /// Converts a presented value to an `BoxLength` field (`width`, `height`,
    /// min/max, edges): `Set(Some(px))` writes a length, `Set(None)` is `unset`
    /// (the field clears to `Auto`), `Leave` means the value could not convert
    /// and the stored field is left untouched.
    fn presented_box_length(&self, property: PresentationProperty) -> PresentedBoxLength {
        match self.active_presented(property) {
            Some(DeclValue::Length(DeclLength::Px(value))) => {
                PresentedBoxLength::Set(BoxLength::Px(*value))
            }
            Some(DeclValue::Length(DeclLength::Unset) | DeclValue::Keyword(Keyword::Auto)) => {
                PresentedBoxLength::Set(BoxLength::Auto)
            }
            Some(DeclValue::Length(DeclLength::Percent(pct))) => {
                PresentedBoxLength::Set(BoxLength::Percent(*pct))
            }
            _ => PresentedBoxLength::Leave,
        }
    }

    /// Converts a presented value to a `BoxLength` margin edge: `Px`→`Px`,
    /// `Percent`→`Percent`, `Unset`→`Px(0.0)`, `Keyword(Auto)`→`Auto` (authored
    /// `auto`, distinct from absent).
    fn presented_margin_edge(&self, property: PresentationProperty) -> Option<BoxLength> {
        match self.active_presented(property)? {
            DeclValue::Length(DeclLength::Px(value)) => Some(BoxLength::Px(*value)),
            DeclValue::Length(DeclLength::Percent(pct)) => Some(BoxLength::Percent(*pct)),
            DeclValue::Length(DeclLength::Unset) => Some(BoxLength::Px(0.0)),
            DeclValue::Keyword(Keyword::Auto) => Some(BoxLength::Auto),
            _ => None,
        }
    }

    /// Converts a presented value to a `BoxLength` padding edge (no `auto`):
    /// `Px`→`Px`, `Percent`→`Percent`, `Unset`→`Px(0.0)`.
    fn presented_padding_edge(&self, property: PresentationProperty) -> Option<BoxLength> {
        match self.active_presented(property)? {
            DeclValue::Length(DeclLength::Px(value)) => Some(BoxLength::Px(*value)),
            DeclValue::Length(DeclLength::Percent(pct)) => Some(BoxLength::Percent(*pct)),
            DeclValue::Length(DeclLength::Unset) => Some(BoxLength::Px(0.0)),
            _ => None,
        }
    }

    /// Converts a presented value to a color (color interpolation lerps the
    /// normalized channels; `presented` is already a `Color`).
    fn presented_rgba(&self, property: PresentationProperty) -> Option<[f32; 4]> {
        match self.active_presented(property)? {
            DeclValue::Color(rgba) => Some(*rgba),
            _ => None,
        }
    }

    /// Converts a presented value to a `Keyword` (the transition's `presented`
    /// is already a `Keyword`; the field writes only when it applies).
    fn presented_keyword(&self, property: PresentationProperty) -> Option<Keyword> {
        match self.active_presented(property)? {
            DeclValue::Keyword(keyword) => Some(*keyword),
            _ => None,
        }
    }

    fn presented_opacity(&self) -> Option<f32> {
        match self.active_presented(PresentationProperty::Opacity)? {
            DeclValue::Number(value) if value.is_finite() => Some(*value),
            _ => None,
        }
    }

    fn presented_non_negative_number(&self, property: PresentationProperty) -> Option<f32> {
        match self.active_presented(property)? {
            DeclValue::Number(value) if value.is_finite() => Some(*value),
            _ => None,
        }
    }

    fn presented_order(&self) -> Option<i32> {
        match self.active_presented(PresentationProperty::Order)? {
            DeclValue::Integer(value) => Some(clamp_i64_to_i32(*value)),
            // `order` transitions step; a `Number` presented order is not a
            // shape interpolation produces for it.
            _ => None,
        }
    }

    fn presented_flex_basis(&self) -> Option<FlexBasisKw> {
        match self.active_presented(PresentationProperty::FlexBasis)? {
            DeclValue::Keyword(Keyword::Auto) => Some(FlexBasisKw::Auto),
            DeclValue::Keyword(Keyword::Content) => Some(FlexBasisKw::Content),
            DeclValue::Length(DeclLength::Px(value)) => Some(FlexBasisKw::Length(*value)),
            // `em`/`unset` need the own font size, which this accessor does not
            // carry; fall back to the stored group field.
            _ => None,
        }
    }

    fn presented_font_size(&self) -> Option<f32> {
        match self.active_presented(PresentationProperty::FontSize)? {
            DeclValue::Length(DeclLength::Px(value)) if value.is_finite() => Some(*value),
            _ => None,
        }
    }

    fn presented_font_weight(&self) -> Option<u16> {
        match self.active_presented(PresentationProperty::FontWeight)? {
            DeclValue::Keyword(Keyword::Normal) => Some(400),
            DeclValue::Keyword(Keyword::Bold) => Some(700),
            DeclValue::Integer(value) => u16::try_from(*value).ok(),
            // `bolder`/`lighter` resolve against the parent weight, which this
            // accessor does not carry; fall back to the stored group field.
            _ => None,
        }
    }

    fn presented_font_style(&self) -> Option<Keyword> {
        match self.active_presented(PresentationProperty::FontStyle)? {
            DeclValue::Keyword(keyword) => Some(*keyword),
            _ => None,
        }
    }

    fn presented_transform(&self) -> Option<([f32; 2], bool)> {
        match self.active_presented(PresentationProperty::Transform)? {
            DeclValue::Translate(v) => Some((*v, true)),
            DeclValue::Keyword(Keyword::None) => Some(([0.0, 0.0], false)),
            // A `Text` transform cannot be re-parsed without the document
            // interner; transform transitions step and the stored group already
            // holds the final resolved translate — fall back to it.
            _ => None,
        }
    }

    fn presented_isolate(&self) -> Option<bool> {
        match self.active_presented(PresentationProperty::Isolate)? {
            DeclValue::Keyword(Keyword::Isolate) => Some(true),
            DeclValue::Keyword(Keyword::Auto) => Some(false),
            _ => None,
        }
    }

    /// Converts a presented line-height to the resolved `Option<f32>` field via
    /// `PresentedOptionalPx`: `Set(None)` = `normal` (the field clears),
    /// `Set(Some(px))` = a resolved line box, `Leave` = not convertible.
    fn presented_line_height(&self, size_px: f32) -> PresentedOptionalPx {
        match self.active_presented(PresentationProperty::LineHeight) {
            Some(DeclValue::Keyword(Keyword::Normal)) => PresentedOptionalPx::Set(None),
            Some(DeclValue::Length(DeclLength::Px(px))) => {
                PresentedOptionalPx::Set(Some(px.max(1.0)))
            }
            Some(DeclValue::Length(DeclLength::Em(em))) => {
                PresentedOptionalPx::Set(Some((em * size_px).max(1.0)))
            }
            Some(DeclValue::Number(scale)) => {
                PresentedOptionalPx::Set(Some((scale * size_px).max(1.0)))
            }
            Some(DeclValue::Length(DeclLength::Percent(pct))) => {
                PresentedOptionalPx::Set(Some((pct / 100.0 * size_px).max(1.0)))
            }
            _ => PresentedOptionalPx::Leave,
        }
    }

    fn presented_text_align(&self) -> Option<DomTextAlign> {
        let DeclValue::Keyword(keyword) = self.active_presented(PresentationProperty::TextAlign)?
        else {
            return None;
        };
        text_align_from_keyword(*keyword)
    }

    fn presented_text_direction(&self) -> Option<DomTextDirection> {
        let DeclValue::Keyword(keyword) = self.active_presented(PresentationProperty::Direction)?
        else {
            return None;
        };
        text_direction_from_keyword(*keyword)
    }

    fn presented_white_space(&self) -> Option<DomWhiteSpace> {
        let DeclValue::Keyword(keyword) =
            self.active_presented(PresentationProperty::WhiteSpace)?
        else {
            return None;
        };
        white_space_from_keyword(*keyword)
    }

    fn presented_overflow_wrap(&self) -> Option<DomOverflowWrap> {
        let DeclValue::Keyword(keyword) =
            self.active_presented(PresentationProperty::OverflowWrap)?
        else {
            return None;
        };
        overflow_wrap_from_keyword(*keyword)
    }

    fn presented_word_break(&self) -> Option<DomWordBreak> {
        let DeclValue::Keyword(keyword) = self.active_presented(PresentationProperty::WordBreak)?
        else {
            return None;
        };
        word_break_from_keyword(*keyword)
    }

    /// Returns the canonical public layout variables for this node.
    ///
    /// This surface is limited to intrinsic sizing, available size, and used
    /// size so external callers can replay browser-style layout calculations
    /// without depending on internal node state bundles. The style side of the
    /// canonical snapshot lives on `RootDom::canonical_style_snapshot` (the node
    /// cannot resolve the inline declaration store).
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
        inherited_text_style: crate::domains::text_layout::ResolvedTextStyle,
    ) -> bool {
        commit_intrinsic_after_style_recompute(
            self,
            inherited_font_size_px,
            inherited_line_height_px,
            inherited_white_space,
            inherited_overflow_wrap,
            inherited_word_break,
            inherited_text_style,
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
            .insert(transition.property, transition);
    }

    pub(crate) fn clear_transition(&mut self, property: PresentationProperty) {
        self.base_mut().transitions.active.remove(&property);
    }

    #[must_use]
    pub(crate) fn active_transition(
        &self,
        property: PresentationProperty,
    ) -> Option<&DomTransition> {
        self.base().transitions.active.get(&property)
    }

    /// Returns one stored attribute value. Content-only (spine 3.3g-2a):
    /// presentation attrs are never stored anymore, so there is no
    /// transition-overlay materialization here — the typed overlay merge runs
    /// inside the `computed_*` accessors. The name stays for the content
    /// callers that read stored attrs (e.g. `pointer-events`, `id`, `class`).
    #[must_use]
    pub fn presented_attr(&self, attribute: &DomAttribute) -> Option<&DomAttributeValue> {
        match self {
            Self::Document(node) => stored_attribute_value(&node.attrs, attribute),
            Self::DocumentFragment(node) => stored_attribute_value(&node.attrs, attribute),
            Self::Element(node) => stored_attribute_value(&node.attrs, attribute),
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
        let mut invalidation = GroupDirtyClassification::NONE;
        let mut inherited_progress = false;
        let mut finished_properties = Vec::new();
        base.transitions.active.retain(|property, transition| {
            if transition.sync(now_s) {
                invalidation =
                    invalidation.union(GroupDirtyClassification::for_group(property.group()));
                if property.inherited() {
                    inherited_progress = true;
                }
            }
            if transition.is_finished(now_s) {
                finished_properties.push(FinishedDomTransition {
                    property: *property,
                    elapsed_s: transition.effective_end_s() - transition.effective_start_s(),
                });
                return false;
            }
            true
        });
        DomTransitionSyncResult {
            invalidation,
            inherited_progress,
            still_active: !base.transitions.active.is_empty(),
            finished_properties,
        }
    }

    pub(crate) fn active_transition_properties(
        &self,
    ) -> impl Iterator<Item = &PresentationProperty> + '_ {
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

/// Writes a presented px length into a plain-`f32` field when it converts;
/// leaves the field untouched otherwise (the transition-overlay merge's
/// fall-back-to-stored contract).
fn apply_px(target: &mut f32, presented: Option<f32>) {
    if let Some(px) = presented {
        *target = px;
    }
}

/// Writes a presented `BoxLength` length into an optional field.
/// `Set(px)` writes a length; `Set(Auto)` is `unset`/`auto` (the field clears);
/// `Leave` means the value could not convert and the stored field is left
/// untouched.
fn apply_box_length(target: &mut BoxLength, presented: PresentedBoxLength) {
    if let PresentedBoxLength::Set(value) = presented {
        *target = value;
    }
}

/// Writes a presented `BoxLength` edge value when it converts; leaves the field
/// untouched otherwise.
fn apply_box_length_value(target: &mut BoxLength, presented: Option<BoxLength>) {
    if let Some(value) = presented {
        *target = value;
    }
}

/// The outcome of converting a presented value to a `BoxLength` length field: a
/// resolved value (or `Auto`) to write, or a signal to leave the stored field
/// untouched.
#[derive(Clone, Copy)]
enum PresentedBoxLength {
    Set(BoxLength),
    Leave,
}

/// Writes a presented `Option<f32>` length into an optional field (line-height's
/// `Option<f32>` group field — the font group stays `f32`, not `BoxLength`).
/// `Set(Some(px))` writes a length; `Set(None)` is `normal` (the field clears);
/// `Leave` means the value could not convert and the stored field is left
/// untouched.
fn apply_optional_px(target: &mut Option<f32>, presented: PresentedOptionalPx) {
    if let PresentedOptionalPx::Set(value) = presented {
        *target = value;
    }
}

/// The outcome of converting a presented value to an `Option<f32>` length field
/// (line-height): a resolved value (or `normal`) to write, or a signal to leave
/// the stored field untouched.
#[derive(Clone, Copy)]
enum PresentedOptionalPx {
    Set(Option<f32>),
    Leave,
}

/// Clamps an authored order integer into the `i32` field range (mirrors the
/// projection's `order` resolution clamp).
fn clamp_i64_to_i32(value: i64) -> i32 {
    i32::try_from(value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)))
        .expect("clamped i64 order should fit into i32")
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
    class_list: &mut Vec<String>,
    attribute: &DomAttribute,
    value: Option<DomAttributeValue>,
) {
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
        return;
    }

    match value {
        Some(value) => {
            attrs.attributes.insert(attribute.clone(), value);
        }
        None => {
            attrs.attributes.remove(attribute);
        }
    }

    if let Some(next_class_list) = next_class_list {
        *class_list = next_class_list;
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

pub(crate) fn lowered_image_source_kind_from_attrs(
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

/// The content-attribute invalidation table (spine 3.3g-2a). Presentation
/// attributes error at the D5 enforcement gate before this runs, so their rows
/// are gone: only the content attributes that move layout/paint/style remain.
/// Intrinsic-driven layout marks (`src`, `type`, `placeholder`) arrive through
/// `before_attr_write_intrinsic_update`, never through this table.
pub(crate) fn attribute_invalidation(attribute: &DomAttribute) -> AttributeInvalidation {
    let mut invalidation = AttributeInvalidation::default();
    if attribute_affects_paint(attribute) {
        invalidation = invalidation.with_paint();
    }
    if attribute_affects_style_object(attribute) {
        invalidation = invalidation.with_style();
    }
    invalidation
}

/// The content attributes that change paint (form-control state and text
/// content): `Src`, `Label`, `Value`, `Checked`, `Selected`, `Multiple`.
pub(crate) fn attribute_affects_paint(attribute: &DomAttribute) -> bool {
    matches!(
        attribute,
        DomAttribute::Src
            | DomAttribute::Label
            | DomAttribute::Value
            | DomAttribute::Checked
            | DomAttribute::Selected
            | DomAttribute::Multiple
    )
}

/// The content attributes that change the style object (restyle the node's
/// matched rules): `Class` only.
pub(crate) fn attribute_affects_style_object(attribute: &DomAttribute) -> bool {
    matches!(attribute, DomAttribute::Class)
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use super::*;
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
    fn set_attr_rejects_invalid_content_values() {
        // Content attrs that have closed grammars still reject boundary data.
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Div);
        let error = node
            .set_attr(
                DomAttribute::Dir,
                Some(DomAttributeValue::String("diagonal".to_string())),
            )
            .expect_err("unknown dir should fail");
        assert_eq!(error, DomError::InvalidAttributeValue);
    }

    #[test]
    fn dom_node_after_attr_write_updates_intrinsic_comes_from_element_behavior() {
        let img = DomNode::element(test_owner_document_key(), DomTag::Img);
        let input = DomNode::element(test_owner_document_key(), DomTag::Input);
        let textarea = DomNode::element(test_owner_document_key(), DomTag::Textarea);
        let div = DomNode::element(test_owner_document_key(), DomTag::Div);

        assert!(img.after_attr_write_updates_intrinsic(&DomAttribute::Src));
        assert!(input.after_attr_write_updates_intrinsic(&DomAttribute::Value));
        assert!(input.after_attr_write_updates_intrinsic(&DomAttribute::Placeholder));
        assert!(input.after_attr_write_updates_intrinsic(&DomAttribute::Type));
        assert!(textarea.after_attr_write_updates_intrinsic(&DomAttribute::Value));
        assert!(textarea.after_attr_write_updates_intrinsic(&DomAttribute::Placeholder));
        assert!(!img.after_attr_write_updates_intrinsic(&DomAttribute::Value));
        assert!(!input.after_attr_write_updates_intrinsic(&DomAttribute::Src));
        assert!(!textarea.after_attr_write_updates_intrinsic(&DomAttribute::Type));
        assert!(!div.after_attr_write_updates_intrinsic(&DomAttribute::Src));
    }

    #[test]
    fn dom_node_before_attr_write_intrinsic_update_returns_none_without_behavior_match() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Div);

        // A plain `div` has no `src`-driven intrinsic behavior, so previewing a
        // `src` write returns no prepared intrinsic.
        let prepared_intrinsic = node
            .before_attr_write_intrinsic_update(
                &DomAttribute::Src,
                Some(&DomAttributeValue::String("icon:arrow".to_string())),
            )
            .expect("non-img src should not prepare intrinsic");

        assert_eq!(prepared_intrinsic, None);
        let intrinsic_changed = node.after_attr_write_intrinsic_update(prepared_intrinsic);
        assert!(!intrinsic_changed);
        assert_eq!(node.intrinsic(), &DomIntrinsic::None);
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

        // Placeholder is not a measured preferred-size field, so a
        // placeholder-only write must not manufacture a phantom change: the
        // removed `generation` counter used to make this report `true`.
        assert!(!intrinsic_changed);
        let DomIntrinsic::ElementDefaultPreferredSize(after) = node.intrinsic() else {
            panic!("input should keep preferred-size intrinsic after attr write");
        };
        assert_f32_eq(after.width_px, before.width_px);
        assert_f32_eq(after.font_size_px, before.font_size_px);
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
        let DomIntrinsic::Text(_) = node.intrinsic() else {
            panic!(
                "text should keep text intrinsic after create, got {:?}",
                node.intrinsic()
            )
        };

        let intrinsic_changed = node.after_style_recompute_intrinsic_update(
            20.0,
            None,
            DomWhiteSpace::Normal,
            DomOverflowWrap::Normal,
            DomWordBreak::Normal,
            ResolvedTextStyle::default(),
        );

        assert!(intrinsic_changed);
        let DomIntrinsic::Text(next_intrinsic) = node.intrinsic() else {
            panic!("text should keep text intrinsic after style stage");
        };
        assert_f32_eq(next_intrinsic.font_size_px, 20.0);
    }

    #[test]
    fn dom_node_after_style_recompute_intrinsic_update_remeasures_input() {
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Input);
        node.after_create_intrinsic_update()
            .expect("input create intrinsic should measure");
        let DomIntrinsic::ElementDefaultPreferredSize(_) = node.intrinsic() else {
            panic!(
                "input should start with preferred-size intrinsic, got {:?}",
                node.intrinsic()
            )
        };

        let intrinsic_changed = node.after_style_recompute_intrinsic_update(
            20.0,
            None,
            DomWhiteSpace::Normal,
            DomOverflowWrap::Normal,
            DomWordBreak::Normal,
            ResolvedTextStyle::default(),
        );

        assert!(intrinsic_changed);
        let DomIntrinsic::ElementDefaultPreferredSize(next_intrinsic) = node.intrinsic() else {
            panic!("input should keep preferred-size intrinsic after style stage");
        };
        assert_f32_eq(next_intrinsic.font_size_px, 20.0);
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
    fn owner_document_key_returns_creator_root_key() {
        let owner_document_key = test_owner_document_key();
        let node = DomNode::element(owner_document_key, DomTag::Button);

        assert_eq!(node.owner_document_key(), owner_document_key);
    }

    #[test]
    fn dispatch_event_keeps_unreached_once_listener_registered() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
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
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Button);
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_listener = calls.clone();
        node.add_event_listener(
            DomEventName::Click,
            DomEventListener::new(move |_| {
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
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let mut node = DomNode::element(test_owner_document_key(), DomTag::Button);
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_listener = calls.clone();
        node.add_event_listener(
            DomEventName::Click,
            DomEventListener::new(move |_| {
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

    fn stop_capture_listener(context: &mut DomEventListenerContext<'_>) {
        STOP_CAPTURE_CALLS.fetch_add(1, Ordering::SeqCst);
        context.stop_immediate_propagation();
    }

    fn once_bubble_listener(_: &mut DomEventListenerContext<'_>) {
        ONCE_BUBBLE_CALLS.fetch_add(1, Ordering::SeqCst);
    }
}
