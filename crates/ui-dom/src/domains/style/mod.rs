//! Selector grammar, matching, specificity, and cascade ordering for `ui-dom`.
//!
//! This file owns the stylesheet block parser (`parse_css_blocks`), the
//! selector subset parser (`parse_selector`), the selector AST types
//! (`SimpleSelector`, `RightMostSelector`), `SelectorSpecificity`, the
//! `ComputedStyleMap` type, right-to-left selector matching
//! (`selector_matches_node`), and the rule identity hash over typed
//! declarations. The typed winners fold lives in
//! `domains/visitor/winners_fold.rs`; rule and declaration storage plus every
//! index live in `style_runtime.rs`; this file never stores rules or
//! declaration blocks.
//!
//! Invariants and rules:
//!
//! - the selector grammar is a closed subset: `#id`, `.class` (multi-class),
//!   `tag`, `tag.class`, `*`, descendant combinators, and `:hover`, `:active`,
//!   `:focus`, `:focus-visible`; all other syntax is rejected with
//!   `DomError::InvalidSelector` at parse time
//! - matching walks right-to-left with a parent-chain scan per preceding
//!   compound; the right-most compound is the record `StyleRuntime` indexes
//! - specificity is the CSS (a, b, c) lexicographic triple via derived `Ord`
//!   field order; pseudo-classes count into `class`
//! - cascade precedence is `(tier, origin.priority, specificity, order)` where
//!   `tier` is `CascadeTier` (`ImportantAuthor > NormalAuthor > NormalUa`);
//!   `!important` is admitted (F stage 1), cascade layers are not
//! - declarations are typed (`DeclValue`) and arena-resident; `decl_value.rs`
//!   owns `decl_value_to_dom_value`, the formatter the debug-priced
//!   `compute_style_map` uses to project winners to the public wire shape
//! - the widget behavior sheet holds typed `Declaration`s folded before
//!   matched rules (the winners fold reads it); text-valued properties are
//!   unsupported there (interner-relative `TextKey`s with no owning store) and
//!   are skipped
//! - stylesheet text is boundary data: parse and match failures are typed
//!   errors, never panics

mod cascade_tier;
mod decl_block;
mod decl_drop_report;
mod decl_parse;
mod decl_store;
mod decl_value;
mod declaration;
mod keyword;
mod parsed_declaration;
mod presentation_property;
mod property_set;
mod style_mut;
mod style_rule_origin;
mod style_runtime;
mod text_interner;

pub(crate) mod computed;

use std::collections::{BTreeMap, HashSet};
use std::hash::{Hash, Hasher};

use slotmap::{SlotMap, new_key_type};

use crate::types::dom_node::{DomNode, DomNodeSlotKey};
use crate::types::dom_tag::DomTag;
use crate::types::{DomAttributeValue, DomError, DomPseudoState};

#[cfg(test)]
use crate::types::{RootDom, RootDomContext};

pub use cascade_tier::CascadeTier;
pub use decl_block::DeclBlock;
pub use decl_drop_report::{DeclDropReport, StylesheetLoadReport};
pub use decl_store::{DeclBlockKey, DeclStore};
pub use decl_value::{
    DeclLength, DeclValue, GridAreasKey, GridTrackListKey, Precedence, TextFamilyListKey, TextKey,
};
pub use declaration::Declaration;
pub use keyword::Keyword;
pub(crate) use parsed_declaration::ParsedDeclaration;
pub use presentation_property::{DeclGrammar, PresentationGroup, PresentationProperty};
pub use style_mut::StyleMut;
pub use style_rule_origin::StyleRuleOrigin;
pub use style_runtime::StyleRuntime;
pub(crate) use text_interner::TextInterner;

pub use computed::{
    AlignContentKw, AlignItemsKw, BoxLength, BoxSizingKw, ComputedBoxGroup, ComputedColorGroup,
    ComputedFlexGroup, ComputedFontGroup, ComputedGridGroup, ComputedGroups, ComputedStyleRef,
    ComputedTextGroup, ComputedTransformGroup, ComputedVisualGroup, DisplayKw, FlexBasisKw,
    FlexDirectionKw, FlexWrapKw, GridAutoFlowKw, GridLine, GridTrackSize, JustifyContentKw,
    JustifyItemsKw, JustifySelfKw, NodePresentation, OverflowKw, PositionKw,
    ResolvedTransitionLonghands, TRANSITION_LONGHAND_CAPACITY, TrackBreadth, TrackMaxBreadth,
    TransitionLonghandProperty,
};
pub(crate) use computed::{
    GRID_LINE_NAME_CAPACITY, GRID_TRACK_LIST_CAPACITY, GridTemplateAreas, GridTrackList,
};
pub(crate) use decl_parse::{name_is_presentation, parse_declaration_block};
pub(crate) use decl_value::{decl_value_to_css_string, decl_value_to_dom_value};

/// The public computed-style map: one winner per presentation property,
/// projected to its wire `DomAttributeValue` (spine 3.4: keyed by
/// `PresentationProperty` — the `DomAttribute`-keyed legacy map died with the
/// presentation variants).
pub type ComputedStyleMap = BTreeMap<PresentationProperty, DomAttributeValue>;

new_key_type! {
    pub struct StyleRuleKey;
}

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct SelectorSpecificity {
    pub id: u16,
    pub class: u16,
    pub tag: u16,
}

/// The cascade precedence key for one matched rule, in sort order:
/// `(origin.priority, specificity, order)`.
pub(crate) type PrecedenceTuple = (i32, SelectorSpecificity, u64);

/// One retained compiled rule. Declarations are no longer embedded: the rule
/// holds a `DeclBlockKey` into the arena's `DeclStore` (spine step 2). The
/// `selector` string is retained for diagnostics/debug output and has no
/// matching or hashing role (selector identity comes from `selector_parts`).
#[derive(Debug, PartialEq)]
pub(crate) struct CompiledRule {
    #[allow(dead_code)] // diagnostics/debug only until the step-3 winner fold
    pub(crate) selector: String,
    selector_parts: Vec<SimpleSelector>,
    right_most: RightMostSelector,
    pub(crate) decl: DeclBlockKey,
    pub(crate) specificity: SelectorSpecificity,
    pub(crate) origin: StyleRuleOrigin,
    pub(crate) order: u64,
    identity_hash: u64,
    pub(crate) active: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RightMostSelector {
    id: Option<String>,
    classes: Vec<String>,
    tag: Option<String>,
    universal: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CssBlock {
    selector: String,
    declarations: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SimpleSelector {
    id: Option<String>,
    classes: Vec<String>,
    tag: Option<String>,
    universal: bool,
    pseudo_state: Option<SelectorPseudoState>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum SelectorPseudoState {
    Hover,
    Active,
    Focus,
    FocusVisible,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[allow(clippy::struct_field_names)]
struct PseudoStateCounts {
    hover_count: usize,
    active_count: usize,
    focus_count: usize,
    focus_visible_count: usize,
}

#[derive(Clone, Copy)]
pub(crate) struct NodeLookup<'a> {
    pub(crate) nodes: &'a SlotMap<DomNodeSlotKey, DomNode>,
}

#[derive(Clone, Copy)]
pub(crate) struct NodeStyleIndex<'a> {
    pub(crate) id: Option<&'a str>,
    pub(crate) classes: &'a [String],
    pub(crate) tag: &'a str,
}

pub(crate) fn style_index_for_node(node: &DomNode) -> Option<NodeStyleIndex<'_>> {
    let element = node.as_element()?;
    Some(NodeStyleIndex {
        id: element.attrs.id.as_deref(),
        classes: element.class_list.as_slice(),
        tag: tag_selector_name(&element.tag),
    })
}

fn selector_matches_node(
    lookup: NodeLookup<'_>,
    node_key: DomNodeSlotKey,
    selector_parts: &[SimpleSelector],
) -> Result<bool, DomError> {
    if selector_parts.is_empty() {
        return Err(DomError::InvalidSelector);
    }
    let node = lookup.nodes.get(node_key).ok_or(DomError::InvalidNodeKey)?;
    if !simple_selector_matches(node, &selector_parts[selector_parts.len() - 1]) {
        return Ok(false);
    }

    let mut current_key = node_key;
    for selector in selector_parts[..selector_parts.len() - 1].iter().rev() {
        let mut cursor = lookup
            .nodes
            .get(current_key)
            .ok_or(DomError::InvalidNodeKey)?
            .parent_key();
        let mut found = false;
        while let Some(candidate_key) = cursor {
            let candidate_node = lookup
                .nodes
                .get(candidate_key)
                .ok_or(DomError::InvalidNodeKey)?;
            if simple_selector_matches(candidate_node, selector) {
                current_key = candidate_key;
                found = true;
                break;
            }
            cursor = candidate_node.parent_key();
        }
        if !found {
            return Ok(false);
        }
    }
    Ok(true)
}

fn append_rule_keys(
    target: &mut Vec<StyleRuleKey>,
    seen: &mut HashSet<StyleRuleKey>,
    bucket: Option<&[StyleRuleKey]>,
) {
    let Some(bucket) = bucket else {
        return;
    };
    for key in bucket {
        if seen.insert(*key) {
            target.push(*key);
        }
    }
}

fn parse_css_blocks(css_text: &str) -> Result<Vec<CssBlock>, DomError> {
    let css_text = strip_block_comments(css_text)?;
    let mut blocks = Vec::new();
    let mut cursor = css_text.as_str().trim();
    while !cursor.is_empty() {
        if cursor.starts_with('}') {
            return Err(DomError::InvalidSelector);
        }
        let Some(open_index) = cursor.find('{') else {
            return Err(DomError::InvalidSelector);
        };
        let selector = cursor[..open_index].trim();
        if selector.is_empty() {
            return Err(DomError::InvalidSelector);
        }
        let declaration_cursor = &cursor[open_index + 1..];
        let Some(close_index) = declaration_cursor.find('}') else {
            return Err(DomError::InvalidAttributeValue);
        };
        let declarations = declaration_cursor[..close_index].trim().to_string();
        blocks.push(CssBlock {
            selector: selector.to_string(),
            declarations,
        });
        cursor = declaration_cursor[close_index + 1..].trim();
    }
    Ok(blocks)
}

fn strip_block_comments(css_text: &str) -> Result<String, DomError> {
    let mut stripped = String::with_capacity(css_text.len());
    let mut chars = css_text.chars().peekable();
    let mut in_comment = false;
    while let Some(ch) = chars.next() {
        if in_comment {
            if ch == '*' && chars.peek().is_some_and(|next| *next == '/') {
                let _ = chars.next();
                in_comment = false;
            }
            continue;
        }
        if ch == '/' && chars.peek().is_some_and(|next| *next == '*') {
            let _ = chars.next();
            in_comment = true;
            continue;
        }
        if ch == '*' && chars.peek().is_some_and(|next| *next == '/') {
            return Err(DomError::InvalidSelector);
        }
        stripped.push(ch);
    }
    if in_comment {
        return Err(DomError::InvalidSelector);
    }
    Ok(stripped)
}

fn parse_selector(selector: &str) -> Result<Vec<SimpleSelector>, DomError> {
    let trimmed = selector.trim();
    if trimmed.is_empty() {
        return Err(DomError::InvalidSelector);
    }
    trimmed
        .split_ascii_whitespace()
        .map(parse_simple_selector)
        .collect()
}

fn parse_simple_selector(selector: &str) -> Result<SimpleSelector, DomError> {
    let (base_selector, pseudo_state) = split_selector_pseudo_state(selector)?;
    if base_selector.is_empty() || contains_unsupported_selector_syntax(base_selector) {
        return Err(DomError::InvalidSelector);
    }
    if base_selector == "*" {
        return Ok(SimpleSelector {
            id: None,
            classes: Vec::new(),
            tag: None,
            universal: true,
            pseudo_state,
        });
    }
    if let Some(id) = base_selector.strip_prefix('#') {
        return parse_id_selector(id, pseudo_state);
    }
    if let Some(class) = base_selector.strip_prefix('.') {
        return parse_class_selector(class, pseudo_state);
    }
    if base_selector.contains('#') {
        return Err(DomError::InvalidSelector);
    }
    if base_selector.contains('.') {
        return parse_tag_and_classes_selector(base_selector, pseudo_state);
    }
    if !is_valid_selector_ident(base_selector) {
        return Err(DomError::InvalidSelector);
    }
    Ok(SimpleSelector {
        id: None,
        classes: Vec::new(),
        tag: Some(base_selector.to_string()),
        universal: false,
        pseudo_state,
    })
}

fn contains_unsupported_selector_syntax(selector: &str) -> bool {
    selector.chars().any(|ch| {
        matches!(
            ch,
            '[' | ']' | '>' | '+' | '~' | ',' | '(' | ')' | '{' | '}' | '=' | '\n' | '\r' | '\t'
        )
    })
}

fn split_selector_pseudo_state(
    selector: &str,
) -> Result<(&str, Option<SelectorPseudoState>), DomError> {
    let Some((base_selector, pseudo_state)) = selector.split_once(':') else {
        return Ok((selector, None));
    };
    if base_selector.is_empty() || pseudo_state.is_empty() || pseudo_state.contains(':') {
        return Err(DomError::InvalidSelector);
    }
    let parsed_state = match pseudo_state {
        "hover" => SelectorPseudoState::Hover,
        "active" => SelectorPseudoState::Active,
        "focus" => SelectorPseudoState::Focus,
        "focus-visible" => SelectorPseudoState::FocusVisible,
        _ => return Err(DomError::InvalidSelector),
    };
    Ok((base_selector, Some(parsed_state)))
}

fn parse_id_selector(
    id: &str,
    pseudo_state: Option<SelectorPseudoState>,
) -> Result<SimpleSelector, DomError> {
    if id.is_empty() || id.contains('.') || id.contains('#') || !is_valid_selector_ident(id) {
        return Err(DomError::InvalidSelector);
    }
    Ok(SimpleSelector {
        id: Some(id.to_string()),
        classes: Vec::new(),
        tag: None,
        universal: false,
        pseudo_state,
    })
}

fn parse_class_selector(
    class: &str,
    pseudo_state: Option<SelectorPseudoState>,
) -> Result<SimpleSelector, DomError> {
    let classes = parse_selector_classes(class, class_count_in_selector(class) + 1)?;
    if classes.is_empty() {
        return Err(DomError::InvalidSelector);
    }
    Ok(SimpleSelector {
        id: None,
        classes,
        tag: None,
        universal: false,
        pseudo_state,
    })
}

fn parse_tag_and_classes_selector(
    selector: &str,
    pseudo_state: Option<SelectorPseudoState>,
) -> Result<SimpleSelector, DomError> {
    let Some(class_start_index) = selector.find('.') else {
        return Err(DomError::InvalidSelector);
    };
    let mut selector_parts = selector.split('.');
    let tag_part = selector_parts
        .next()
        .expect("split should always yield at least one selector part");
    if tag_part.is_empty() || tag_part.contains('#') || !is_valid_selector_ident(tag_part) {
        return Err(DomError::InvalidSelector);
    }
    let classes = parse_selector_classes(
        &selector[class_start_index + 1..],
        class_count_in_selector(selector),
    )?;
    if classes.is_empty() {
        return Err(DomError::InvalidSelector);
    }
    Ok(SimpleSelector {
        id: None,
        classes,
        tag: Some(tag_part.to_string()),
        universal: false,
        pseudo_state,
    })
}

fn class_count_in_selector(selector: &str) -> usize {
    selector.bytes().filter(|byte| *byte == b'.').count()
}

fn parse_selector_classes(selector: &str, capacity: usize) -> Result<Vec<String>, DomError> {
    let mut classes = Vec::with_capacity(capacity);
    for class_part in selector.split('.') {
        if class_part.is_empty() || class_part.contains('#') || !is_valid_selector_ident(class_part)
        {
            return Err(DomError::InvalidSelector);
        }
        classes.push(class_part.to_string());
    }
    Ok(classes)
}

fn is_valid_selector_ident(value: &str) -> bool {
    value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
}

fn extract_right_most_selector(
    selector_parts: &[SimpleSelector],
) -> Result<RightMostSelector, DomError> {
    let Some(right_most) = selector_parts.last() else {
        return Err(DomError::InvalidSelector);
    };
    Ok(RightMostSelector {
        id: right_most.id.clone(),
        classes: right_most.classes.clone(),
        tag: right_most.tag.clone(),
        universal: right_most.universal,
    })
}

fn specificity_for_selector_parts(selector_parts: &[SimpleSelector]) -> SelectorSpecificity {
    let mut specificity = SelectorSpecificity::default();
    for selector in selector_parts {
        if selector.id.is_some() {
            specificity.id = specificity.id.saturating_add(1);
        }
        for _class in &selector.classes {
            specificity.class = specificity.class.saturating_add(1);
        }
        if selector.pseudo_state.is_some() {
            specificity.class = specificity.class.saturating_add(1);
        }
        if selector.tag.is_some() {
            specificity.tag = specificity.tag.saturating_add(1);
        }
    }
    specificity
}

fn simple_selector_matches(node: &DomNode, selector: &SimpleSelector) -> bool {
    let Some(element) = node.as_element() else {
        return false;
    };
    if !selector.universal
        && let Some(id) = selector.id.as_deref()
        && element.attrs.id.as_deref() != Some(id)
    {
        return false;
    }
    if !selector.universal && !selector.classes.is_empty() {
        if element.class_list.len() < selector.classes.len() {
            return false;
        }
        for class in &selector.classes {
            if !element.class_list.iter().any(|value| value == class) {
                return false;
            }
        }
    }
    if !selector.universal
        && let Some(tag) = selector.tag.as_deref()
        && element.tag.selector_name() != tag
    {
        return false;
    }
    if let Some(pseudo_state) = selector.pseudo_state
        && !element.has_pseudo_state(map_selector_pseudo_state(pseudo_state))
    {
        return false;
    }
    true
}

fn map_selector_pseudo_state(pseudo_state: SelectorPseudoState) -> DomPseudoState {
    match pseudo_state {
        SelectorPseudoState::Hover => DomPseudoState::Hover,
        SelectorPseudoState::Active => DomPseudoState::Active,
        SelectorPseudoState::Focus => DomPseudoState::Focus,
        SelectorPseudoState::FocusVisible => DomPseudoState::FocusVisible,
    }
}

fn pseudo_state_counts(selector_parts: &[SimpleSelector]) -> PseudoStateCounts {
    let mut counts = PseudoStateCounts::default();
    for pseudo_state in selector_parts.iter().filter_map(|part| part.pseudo_state) {
        match pseudo_state {
            SelectorPseudoState::Hover => {
                counts.hover_count = counts.hover_count.saturating_add(1);
            }
            SelectorPseudoState::Active => {
                counts.active_count = counts.active_count.saturating_add(1);
            }
            SelectorPseudoState::Focus => {
                counts.focus_count = counts.focus_count.saturating_add(1);
            }
            SelectorPseudoState::FocusVisible => {
                counts.focus_visible_count = counts.focus_visible_count.saturating_add(1);
            }
        }
    }
    counts
}

fn tag_selector_name(tag: &DomTag) -> &str {
    tag.selector_name()
}

fn compute_rule_identity_hash(
    origin: &StyleRuleOrigin,
    specificity: SelectorSpecificity,
    selector_parts: &[SimpleSelector],
    declarations: &[Declaration],
    importance: &[bool],
) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    origin.key.hash(&mut hasher);
    origin.priority.hash(&mut hasher);
    specificity.id.hash(&mut hasher);
    specificity.class.hash(&mut hasher);
    specificity.tag.hash(&mut hasher);
    selector_parts.len().hash(&mut hasher);
    for selector in selector_parts {
        selector.id.hash(&mut hasher);
        selector.classes.hash(&mut hasher);
        selector.tag.hash(&mut hasher);
        selector.universal.hash(&mut hasher);
        selector.pseudo_state.hash(&mut hasher);
    }
    for declaration in declarations {
        declaration.property.index().hash(&mut hasher);
        hash_decl_value(&mut hasher, &declaration.value);
    }
    // Two rules identical except importance are DIFFERENT rules (different
    // cascade outcome): the parallel importance slice is part of the identity.
    for flag in importance {
        flag.hash(&mut hasher);
    }
    hasher.finish()
}

fn hash_decl_value(hasher: &mut std::collections::hash_map::DefaultHasher, value: &DeclValue) {
    match value {
        DeclValue::Length(length) => {
            0u8.hash(hasher);
            hash_decl_length(hasher, *length);
        }
        DeclValue::Color(channels) => {
            1u8.hash(hasher);
            for channel in channels {
                channel.to_bits().hash(hasher);
            }
        }
        DeclValue::Keyword(keyword) => {
            2u8.hash(hasher);
            keyword.hash(hasher);
        }
        DeclValue::Number(number) => {
            3u8.hash(hasher);
            number.to_bits().hash(hasher);
        }
        DeclValue::Integer(integer) => {
            4u8.hash(hasher);
            integer.hash(hasher);
        }
        DeclValue::Text(key) => {
            5u8.hash(hasher);
            key.0.hash(hasher);
        }
        DeclValue::Translate(channels) => {
            6u8.hash(hasher);
            for channel in channels {
                channel.to_bits().hash(hasher);
            }
        }
        DeclValue::TrackList(key) => {
            7u8.hash(hasher);
            key.hash(hasher);
        }
        DeclValue::GridAreas(key) => {
            11u8.hash(hasher);
            key.hash(hasher);
        }
        DeclValue::TrackSize(size) => {
            8u8.hash(hasher);
            hash_grid_track_size(hasher, *size);
        }
        DeclValue::GridAutoFlow(flow) => {
            9u8.hash(hasher);
            flow.hash(hasher);
        }
        DeclValue::GridLine(line) => {
            10u8.hash(hasher);
            hash_grid_line(hasher, *line);
        }
        DeclValue::FamilyList(key) => {
            12u8.hash(hasher);
            key.hash(hasher);
        }
    }
}

/// Hashes one `GridLine` by its variant tag plus the carried integer (the
/// `GridLine` derives `Hash`; hashing here keeps the tag consistent with the
/// `DeclValue` variant's own tag prefix).
fn hash_grid_line(hasher: &mut std::collections::hash_map::DefaultHasher, line: GridLine) {
    match line {
        GridLine::Auto => 0u8.hash(hasher),
        GridLine::Line(n) => {
            1u8.hash(hasher);
            n.hash(hasher);
        }
        GridLine::Span(n) => {
            2u8.hash(hasher);
            n.hash(hasher);
        }
        GridLine::Named(key) => {
            3u8.hash(hasher);
            key.0.hash(hasher);
        }
        GridLine::NamedSpan(key, n) => {
            4u8.hash(hasher);
            key.0.hash(hasher);
            n.hash(hasher);
        }
    }
}

/// Hashes one `GridTrackSize` by its min/max breadth pair plus the `to_bits`
/// of its numeric slots (`f32` does not implement `Hash` in this toolchain, so
/// the bits are hashed directly — consistent with the `PartialEq` bit equality
/// the group relies on). The percent-vs-px unit is part of the identity
/// (`GRID_STEP2_DESIGN.md` correction 3).
fn hash_grid_track_size(
    hasher: &mut std::collections::hash_map::DefaultHasher,
    size: GridTrackSize,
) {
    hash_track_breadth(hasher, size.min);
    hash_track_max_breadth(hasher, size.max);
}

fn hash_track_breadth(
    hasher: &mut std::collections::hash_map::DefaultHasher,
    breadth: TrackBreadth,
) {
    match breadth {
        TrackBreadth::Auto => 0u8.hash(hasher),
        TrackBreadth::Px(px) => {
            1u8.hash(hasher);
            px.to_bits().hash(hasher);
        }
        TrackBreadth::Percent(percent) => {
            2u8.hash(hasher);
            percent.to_bits().hash(hasher);
        }
        TrackBreadth::MinContent => 3u8.hash(hasher),
        TrackBreadth::MaxContent => 4u8.hash(hasher),
    }
}

fn hash_track_max_breadth(
    hasher: &mut std::collections::hash_map::DefaultHasher,
    breadth: TrackMaxBreadth,
) {
    match breadth {
        TrackMaxBreadth::Auto => 0u8.hash(hasher),
        TrackMaxBreadth::Px(px) => {
            1u8.hash(hasher);
            px.to_bits().hash(hasher);
        }
        TrackMaxBreadth::Percent(percent) => {
            2u8.hash(hasher);
            percent.to_bits().hash(hasher);
        }
        TrackMaxBreadth::MinContent => 3u8.hash(hasher),
        TrackMaxBreadth::MaxContent => 4u8.hash(hasher),
        TrackMaxBreadth::Fr(fr) => {
            5u8.hash(hasher);
            fr.to_bits().hash(hasher);
        }
    }
}

fn hash_decl_length(hasher: &mut std::collections::hash_map::DefaultHasher, length: DeclLength) {
    match length {
        DeclLength::Unset => 0u8.hash(hasher),
        DeclLength::Px(value) => {
            1u8.hash(hasher);
            value.to_bits().hash(hasher);
        }
        DeclLength::Em(value) => {
            2u8.hash(hasher);
            value.to_bits().hash(hasher);
        }
        DeclLength::Percent(value) => {
            3u8.hash(hasher);
            value.to_bits().hash(hasher);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolver_matches_descendant_right_to_left_and_rejects_non_matches() {
        let context = RootDomContext::new_cpu();
        let mut root = RootDom::new(context);
        let outer = root.create_element(DomTag::Div).expect("outer");
        let inner = root.create_element(DomTag::Div).expect("inner");
        let leaf = root.create_element(DomTag::Span).expect("leaf");
        root.append_child(root.root, outer).expect("append outer");
        root.append_child(outer, inner).expect("append inner");
        root.append_child(inner, leaf).expect("append leaf");
        root.set_attr_str(outer, "class", "panel").expect("class");
        root.set_attr_str(leaf, "class", "label focused")
            .expect("leaf class");

        let mut runtime = StyleRuntime::default();
        runtime
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 0),
                "div.panel span.label.focused { width: 10px; } div.miss span.label.focused { height: 10px; }",
            )
            .expect("load");
        let lookup = NodeLookup { nodes: &root.nodes };
        let candidates = runtime.candidate_keys_for_node(NodeStyleIndex {
            id: None,
            classes: &["label".to_string(), "focused".to_string()],
            tag: "span",
        });
        let mut matches = 0usize;
        for key in candidates {
            if runtime
                .active_rule_for_node(key, lookup, leaf)
                .expect("match")
                .is_some()
            {
                matches += 1;
            }
        }
        assert_eq!(matches, 1);
    }

    #[test]
    fn resolver_matches_hover_and_active_pseudo_state() {
        let context = RootDomContext::new_cpu();
        let mut root = RootDom::new(context);
        let button = root.create_element(DomTag::Button).expect("button");
        root.append_child(root.root, button).expect("append button");
        let button_node = root
            .nodes
            .get_mut(button)
            .expect("button should exist")
            .as_element_mut()
            .expect("button should be an element");
        button_node.set_pseudo_state(DomPseudoState::Hover);
        button_node.set_pseudo_state(DomPseudoState::Active);

        let mut runtime = StyleRuntime::default();
        runtime
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 0),
                "button:hover { width: 10px; } button:active { height: 12px; }",
            )
            .expect("stylesheet should load");
        let lookup = NodeLookup { nodes: &root.nodes };
        let candidates = runtime.candidate_keys_for_node(NodeStyleIndex {
            id: None,
            classes: &[],
            tag: "button",
        });
        let matches = candidates
            .into_iter()
            .filter(|key| {
                runtime
                    .active_rule_for_node(*key, lookup, button)
                    .expect("selector match should evaluate")
                    .is_some()
            })
            .count();
        assert_eq!(matches, 2);
    }

    #[test]
    fn resolver_matches_focus_and_focus_visible_pseudo_state() {
        let context = RootDomContext::new_cpu();
        let mut root = RootDom::new(context);
        let button = root.create_element(DomTag::Button).expect("button");
        root.append_child(root.root, button).expect("append button");
        let button_node = root
            .nodes
            .get_mut(button)
            .expect("button should exist")
            .as_element_mut()
            .expect("button should be an element");
        button_node.set_pseudo_state(DomPseudoState::Focus);
        button_node.set_pseudo_state(DomPseudoState::FocusVisible);

        let mut runtime = StyleRuntime::default();
        runtime
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 0),
                "button:focus { width: 10px; } button:focus-visible { height: 12px; }",
            )
            .expect("focus stylesheet should load");
        let lookup = NodeLookup { nodes: &root.nodes };
        let candidates = runtime.candidate_keys_for_node(NodeStyleIndex {
            id: None,
            classes: &[],
            tag: "button",
        });
        let matches = candidates
            .into_iter()
            .filter(|key| {
                runtime
                    .active_rule_for_node(*key, lookup, button)
                    .expect("selector match should evaluate")
                    .is_some()
            })
            .count();
        assert_eq!(matches, 2);
    }

    #[test]
    fn universal_hover_selector_still_checks_pseudo_state() {
        let context = RootDomContext::new_cpu();
        let mut root = RootDom::new(context);
        let button = root.create_element(DomTag::Button).expect("button");
        root.append_child(root.root, button).expect("append button");
        let button_node = root
            .nodes
            .get_mut(button)
            .expect("button should exist")
            .as_element_mut()
            .expect("button should be an element");
        button_node.set_pseudo_state(DomPseudoState::Hover);

        let mut runtime = StyleRuntime::default();
        runtime
            .load_stylesheet(StyleRuleOrigin::new("theme", 0), "*:hover { width: 10px; }")
            .expect("stylesheet should load");
        let lookup = NodeLookup { nodes: &root.nodes };
        let candidates = runtime.candidate_keys_for_node(NodeStyleIndex {
            id: None,
            classes: &[],
            tag: "button",
        });
        let matches = candidates
            .into_iter()
            .filter(|key| {
                runtime
                    .active_rule_for_node(*key, lookup, button)
                    .expect("selector match should evaluate")
                    .is_some()
            })
            .count();
        assert_eq!(matches, 1);
    }
}
