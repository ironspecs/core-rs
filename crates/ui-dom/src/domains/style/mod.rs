mod style_rule_origin;
mod style_runtime;

use std::collections::{BTreeMap, HashSet};
use std::hash::{Hash, Hasher};

use slotmap::{SlotMap, new_key_type};

use crate::types::dom_node::{DomElementNode, DomNode, DomNodeSlotKey};
use crate::types::dom_tag::DomTag;
use crate::types::{DomAttribute, DomAttributeValue, DomError, DomPseudoState, RootDom};

#[cfg(test)]
use crate::types::RootDomContext;

pub use style_rule_origin::StyleRuleOrigin;
pub use style_runtime::StyleRuntime;

pub type ComputedStyleMap = BTreeMap<DomAttribute, DomAttributeValue>;

new_key_type! {
    pub struct StyleRuleKey;
}

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct SelectorSpecificity {
    pub id: u16,
    pub class: u16,
    pub tag: u16,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StyleRule {
    pub selector: String,
    pub attr_map: ComputedStyleMap,
    pub specificity: SelectorSpecificity,
    pub origin: StyleRuleOrigin,
    pub order: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CompiledRule {
    pub(crate) rule: StyleRule,
    selector_parts: Vec<SimpleSelector>,
    right_most: RightMostSelector,
    identity_hash: u64,
    active: bool,
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

pub(crate) fn matched_style_rules_for_element(
    root_dom: &RootDom,
    node_key: DomNodeSlotKey,
) -> Result<Vec<StyleRule>, DomError> {
    let node = root_dom.node(node_key).ok_or(DomError::InvalidNodeKey)?;
    let node_index = style_index_for_node(node).ok_or(DomError::InvalidAttributeTarget)?;
    let runtime = root_dom
        .context()
        .style_runtime
        .lock()
        .expect("style runtime mutex should not poison");
    let candidate_keys = runtime.candidate_keys_for_node(node_index);
    let mut matched_rules: Vec<StyleRule> = Vec::new();
    let lookup = NodeLookup {
        nodes: &root_dom.nodes,
    };
    for key in candidate_keys {
        let Some(rule) = runtime.active_rule_for_node(key, lookup, node_key)? else {
            continue;
        };
        matched_rules.push(rule.clone());
    }
    matched_rules.sort_by_key(precedence_tuple);
    Ok(matched_rules)
}

pub(crate) fn apply_pseudo_state_style_declarations(
    element: &DomElementNode,
    map: &mut ComputedStyleMap,
) {
    for pseudo_state in [
        DomPseudoState::Hover,
        DomPseudoState::Active,
        DomPseudoState::Focus,
        DomPseudoState::FocusVisible,
    ] {
        if !element.has_pseudo_state(pseudo_state) {
            continue;
        }
        for (attribute, value) in element.style().pseudo_state_declarations(pseudo_state) {
            map.insert(attribute.clone(), value.clone());
        }
    }
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

pub(crate) fn precedence_tuple(rule: &StyleRule) -> (i32, SelectorSpecificity, u64) {
    (rule.origin.priority, rule.specificity, rule.order)
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

fn compute_rule_identity_hash(rule: &StyleRule, selector_parts: &[SimpleSelector]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    rule.origin.key.hash(&mut hasher);
    rule.origin.priority.hash(&mut hasher);
    rule.specificity.id.hash(&mut hasher);
    rule.specificity.class.hash(&mut hasher);
    rule.specificity.tag.hash(&mut hasher);
    selector_parts.len().hash(&mut hasher);
    for selector in selector_parts {
        selector.id.hash(&mut hasher);
        selector.classes.hash(&mut hasher);
        selector.tag.hash(&mut hasher);
        selector.universal.hash(&mut hasher);
        selector.pseudo_state.hash(&mut hasher);
    }
    for (attribute, value) in &rule.attr_map {
        hash_dom_attribute(&mut hasher, attribute);
        hash_dom_attribute_value(&mut hasher, value);
    }
    hasher.finish()
}

fn hash_dom_attribute(hasher: &mut impl Hasher, attribute: &DomAttribute) {
    match attribute {
        DomAttribute::Id => 0u8.hash(hasher),
        DomAttribute::Style => 1u8.hash(hasher),
        DomAttribute::Class => 2u8.hash(hasher),
        DomAttribute::Transition => 3u8.hash(hasher),
        DomAttribute::TabIndex => 4u8.hash(hasher),
        DomAttribute::Display => 5u8.hash(hasher),
        DomAttribute::Overflow => 6u8.hash(hasher),
        DomAttribute::Transform => 7u8.hash(hasher),
        DomAttribute::Isolate => 8u8.hash(hasher),
        DomAttribute::Position => 9u8.hash(hasher),
        DomAttribute::FlexDirection => 10u8.hash(hasher),
        DomAttribute::FlexWrap => 11u8.hash(hasher),
        DomAttribute::FlexFlow => 12u8.hash(hasher),
        DomAttribute::FlexGrow => 13u8.hash(hasher),
        DomAttribute::FlexShrink => 14u8.hash(hasher),
        DomAttribute::FlexBasis => 15u8.hash(hasher),
        DomAttribute::Flex => 16u8.hash(hasher),
        DomAttribute::Order => 17u8.hash(hasher),
        DomAttribute::JustifyContent => 18u8.hash(hasher),
        DomAttribute::AlignItems => 19u8.hash(hasher),
        DomAttribute::AlignSelf => 20u8.hash(hasher),
        DomAttribute::AlignContent => 21u8.hash(hasher),
        DomAttribute::Gap => 22u8.hash(hasher),
        DomAttribute::RowGap => 23u8.hash(hasher),
        DomAttribute::ColumnGap => 24u8.hash(hasher),
        DomAttribute::PlaceContent => 25u8.hash(hasher),
        DomAttribute::PlaceItems => 26u8.hash(hasher),
        DomAttribute::PlaceSelf => 27u8.hash(hasher),
        DomAttribute::Width => 28u8.hash(hasher),
        DomAttribute::Height => 29u8.hash(hasher),
        DomAttribute::MinWidth => 30u8.hash(hasher),
        DomAttribute::MinHeight => 31u8.hash(hasher),
        DomAttribute::MaxWidth => 32u8.hash(hasher),
        DomAttribute::MaxHeight => 33u8.hash(hasher),
        DomAttribute::Top => 34u8.hash(hasher),
        DomAttribute::Left => 35u8.hash(hasher),
        DomAttribute::Right => 36u8.hash(hasher),
        DomAttribute::Bottom => 37u8.hash(hasher),
        DomAttribute::MarginTop => 38u8.hash(hasher),
        DomAttribute::MarginLeft => 39u8.hash(hasher),
        DomAttribute::MarginRight => 40u8.hash(hasher),
        DomAttribute::MarginBottom => 41u8.hash(hasher),
        DomAttribute::PaddingTop => 42u8.hash(hasher),
        DomAttribute::PaddingLeft => 43u8.hash(hasher),
        DomAttribute::PaddingRight => 44u8.hash(hasher),
        DomAttribute::PaddingBottom => 45u8.hash(hasher),
        DomAttribute::BorderTop => 46u8.hash(hasher),
        DomAttribute::BorderLeft => 47u8.hash(hasher),
        DomAttribute::BorderRight => 48u8.hash(hasher),
        DomAttribute::BorderBottom => 49u8.hash(hasher),
        DomAttribute::Src => 50u8.hash(hasher),
        DomAttribute::Type => 51u8.hash(hasher),
        DomAttribute::Value => 52u8.hash(hasher),
        DomAttribute::Custom(name) => {
            53u8.hash(hasher);
            name.hash(hasher);
        }
        DomAttribute::ReadOnly => 54u8.hash(hasher),
        DomAttribute::Disabled => 55u8.hash(hasher),
        DomAttribute::Required => 56u8.hash(hasher),
        DomAttribute::Checked => 57u8.hash(hasher),
        DomAttribute::Selected => 58u8.hash(hasher),
        DomAttribute::Multiple => 59u8.hash(hasher),
        DomAttribute::Max => 60u8.hash(hasher),
        DomAttribute::MaxLength => 61u8.hash(hasher),
        DomAttribute::Min => 62u8.hash(hasher),
        DomAttribute::MinLength => 63u8.hash(hasher),
        DomAttribute::Placeholder => 64u8.hash(hasher),
        DomAttribute::Step => 65u8.hash(hasher),
        DomAttribute::FontFamily => 66u8.hash(hasher),
        DomAttribute::FontWeight => 67u8.hash(hasher),
        DomAttribute::FontStyle => 68u8.hash(hasher),
        DomAttribute::LineHeight => 69u8.hash(hasher),
        DomAttribute::TextAlign => 70u8.hash(hasher),
        DomAttribute::WhiteSpace => 71u8.hash(hasher),
        DomAttribute::OverflowWrap => 72u8.hash(hasher),
        DomAttribute::WordBreak => 73u8.hash(hasher),
        DomAttribute::Lang => 74u8.hash(hasher),
        DomAttribute::Dir => 75u8.hash(hasher),
        DomAttribute::List => 76u8.hash(hasher),
        DomAttribute::Label => 77u8.hash(hasher),
        DomAttribute::Size => 78u8.hash(hasher),
        DomAttribute::PresentedOptionsRole => 79u8.hash(hasher),
        DomAttribute::PresentedOptionsState => 80u8.hash(hasher),
        DomAttribute::PresentedOptionsHighlighted => 81u8.hash(hasher),
        DomAttribute::PresentedOptionsSourceIndex => 82u8.hash(hasher),
        DomAttribute::TransitionProperty => 83u8.hash(hasher),
        DomAttribute::TransitionDuration => 84u8.hash(hasher),
        DomAttribute::TransitionTimingFunction => 85u8.hash(hasher),
        DomAttribute::TransitionDelay => 86u8.hash(hasher),
        DomAttribute::Direction => 87u8.hash(hasher),
        DomAttribute::FontSize => 255u8.hash(hasher),
    }
}

fn hash_dom_attribute_value(hasher: &mut impl Hasher, value: &DomAttributeValue) {
    match value {
        DomAttributeValue::String(v) => {
            0u8.hash(hasher);
            v.hash(hasher);
        }
        DomAttributeValue::Bool(v) => {
            1u8.hash(hasher);
            v.hash(hasher);
        }
        DomAttributeValue::Integer(v) => {
            2u8.hash(hasher);
            v.hash(hasher);
        }
        DomAttributeValue::Float(v) => {
            3u8.hash(hasher);
            v.to_bits().hash(hasher);
        }
        DomAttributeValue::Color(v) => {
            4u8.hash(hasher);
            for channel in v {
                channel.to_bits().hash(hasher);
            }
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
