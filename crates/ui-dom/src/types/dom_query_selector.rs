use std::collections::HashMap;

use slotmap::SlotMap;

use crate::types::dom_node::{DomNode, DomNodeSlotKey};
use crate::types::{DomAttributeNode, DomError};

#[derive(Clone, Copy)]
pub(crate) struct DomQuerySelectorStore<'a> {
    pub(crate) root: DomNodeSlotKey,
    pub(crate) nodes: &'a SlotMap<DomNodeSlotKey, DomNode>,
    pub(crate) id_lookup: &'a HashMap<String, DomNodeSlotKey>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SimpleSelector {
    id: Option<String>,
    classes: Vec<String>,
    tag: Option<String>,
}

impl SimpleSelector {
    fn is_pure_id_selector(&self) -> bool {
        self.id.is_some() && self.tag.is_none() && self.classes.is_empty()
    }
}

pub(crate) fn query_selector<'a>(
    store: DomQuerySelectorStore<'a>,
    selector: &str,
) -> Result<Option<(DomNodeSlotKey, &'a DomNode)>, DomError> {
    let selector = parse_selector(selector)?;
    if let Some(found) = find_single_id_match(store, store.root, &selector)? {
        return Ok(Some(found));
    }
    find_first_match(store, store.root, &selector)
}

pub(crate) fn query_selector_all<'a>(
    store: DomQuerySelectorStore<'a>,
    selector: &str,
) -> Result<Vec<(DomNodeSlotKey, &'a DomNode)>, DomError> {
    let selector = parse_selector(selector)?;
    if let Some(found) = find_single_id_match(store, store.root, &selector)? {
        return Ok(vec![found]);
    }
    collect_matches(store, store.root, &selector)
}

pub(crate) fn query_selector_from<'a>(
    store: DomQuerySelectorStore<'a>,
    from: DomNodeSlotKey,
    selector: &str,
) -> Result<Option<(DomNodeSlotKey, &'a DomNode)>, DomError> {
    validate_from_key(store, from)?;
    let selector = parse_selector(selector)?;
    if let Some(found) = find_single_id_match(store, from, &selector)? {
        return Ok(Some(found));
    }
    find_first_match(store, from, &selector)
}

pub(crate) fn query_selector_all_from<'a>(
    store: DomQuerySelectorStore<'a>,
    from: DomNodeSlotKey,
    selector: &str,
) -> Result<Vec<(DomNodeSlotKey, &'a DomNode)>, DomError> {
    validate_from_key(store, from)?;
    let selector = parse_selector(selector)?;
    if let Some(found) = find_single_id_match(store, from, &selector)? {
        return Ok(vec![found]);
    }
    collect_matches(store, from, &selector)
}

fn validate_from_key(
    store: DomQuerySelectorStore<'_>,
    from: DomNodeSlotKey,
) -> Result<(), DomError> {
    if store.nodes.contains_key(from) {
        return Ok(());
    }
    Err(DomError::InvalidNodeKey)
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
    if selector.is_empty() || contains_unsupported_selector_syntax(selector) {
        return Err(DomError::InvalidSelector);
    }

    if let Some(id) = selector.strip_prefix('#') {
        return parse_id_selector(id);
    }
    if selector.contains('#') {
        return Err(DomError::InvalidSelector);
    }

    parse_tag_and_classes(selector)
}

fn contains_unsupported_selector_syntax(selector: &str) -> bool {
    selector.chars().any(|ch| {
        matches!(
            ch,
            '[' | ']' | ':' | '>' | '+' | '~' | ',' | '(' | ')' | '{' | '}' | '='
        )
    })
}

fn is_valid_selector_ident(selector: &str) -> bool {
    selector
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
}

fn parse_id_selector(id: &str) -> Result<SimpleSelector, DomError> {
    if id.is_empty() || id.contains('.') || id.contains('#') || !is_valid_selector_ident(id) {
        return Err(DomError::InvalidSelector);
    }
    Ok(SimpleSelector {
        id: Some(id.to_owned()),
        classes: Vec::new(),
        tag: None,
    })
}

fn parse_tag_and_classes(selector: &str) -> Result<SimpleSelector, DomError> {
    let mut selector_parts = selector.split('.');
    let tag_part = selector_parts
        .next()
        .expect("split should always yield at least one selector part");
    let tag = if tag_part.is_empty() {
        None
    } else {
        if !is_valid_selector_ident(tag_part) {
            return Err(DomError::InvalidSelector);
        }
        Some(tag_part.to_owned())
    };

    let mut classes = Vec::with_capacity(selector.bytes().filter(|byte| *byte == b'.').count());
    for class_part in selector_parts {
        if class_part.is_empty() || !is_valid_selector_ident(class_part) {
            return Err(DomError::InvalidSelector);
        }
        classes.push(class_part.to_owned());
    }

    if tag.is_none() && classes.is_empty() {
        return Err(DomError::InvalidSelector);
    }

    Ok(SimpleSelector {
        id: None,
        classes,
        tag,
    })
}

fn find_single_id_match<'a>(
    store: DomQuerySelectorStore<'a>,
    from: DomNodeSlotKey,
    selector: &[SimpleSelector],
) -> Result<Option<(DomNodeSlotKey, &'a DomNode)>, DomError> {
    let Some(step) = selector.first() else {
        return Err(DomError::InvalidSelector);
    };
    if selector.len() != 1 || !step.is_pure_id_selector() {
        return Ok(None);
    }

    let id = step.id.as_deref().expect("validated id selector");
    let Some(&key) = store.id_lookup.get(id) else {
        return Ok(None);
    };
    if !is_descendant_of(store, from, key)? {
        return Ok(None);
    }
    let node = store.nodes.get(key).ok_or(DomError::InvalidNodeKey)?;
    Ok(Some((key, node)))
}

fn is_descendant_of(
    store: DomQuerySelectorStore<'_>,
    from: DomNodeSlotKey,
    candidate: DomNodeSlotKey,
) -> Result<bool, DomError> {
    if candidate == from {
        return Ok(false);
    }

    let mut current_key = store
        .nodes
        .get(candidate)
        .ok_or(DomError::InvalidNodeKey)?
        .parent_key();
    while let Some(parent_key) = current_key {
        if parent_key == from {
            return Ok(true);
        }
        current_key = store
            .nodes
            .get(parent_key)
            .ok_or(DomError::InvalidNodeKey)?
            .parent_key();
    }
    Ok(false)
}

fn find_first_match<'a>(
    store: DomQuerySelectorStore<'a>,
    from: DomNodeSlotKey,
    selector: &[SimpleSelector],
) -> Result<Option<(DomNodeSlotKey, &'a DomNode)>, DomError> {
    let mut found = None;
    visit_descendant_elements(store, from, |key, node| {
        if found.is_some() {
            return Ok(false);
        }
        if matches_selector_path(store, from, node, selector)? {
            found = Some((key, node));
            return Ok(false);
        }
        Ok(true)
    })?;
    Ok(found)
}

fn collect_matches<'a>(
    store: DomQuerySelectorStore<'a>,
    from: DomNodeSlotKey,
    selector: &[SimpleSelector],
) -> Result<Vec<(DomNodeSlotKey, &'a DomNode)>, DomError> {
    let mut matches = Vec::new();
    visit_descendant_elements(store, from, |key, node| {
        if matches_selector_path(store, from, node, selector)? {
            matches.push((key, node));
        }
        Ok(true)
    })?;
    Ok(matches)
}

fn visit_descendant_elements<'a, F>(
    store: DomQuerySelectorStore<'a>,
    from: DomNodeSlotKey,
    mut visit: F,
) -> Result<(), DomError>
where
    F: FnMut(DomNodeSlotKey, &'a DomNode) -> Result<bool, DomError>,
{
    let mut stack = Vec::new();
    push_start_child(store, from, &mut stack)?;

    while let Some(key) = stack.pop() {
        let node = store.nodes.get(key).ok_or(DomError::InvalidNodeKey)?;
        if let Some(next_sibling_key) = node.next_sibling_key_for_store() {
            stack.push(next_sibling_key);
        }
        if let Some(first_child_key) = node.first_child_key_for_store() {
            stack.push(first_child_key);
        }
        if node.as_element().is_some() && !visit(key, node)? {
            return Ok(());
        }
    }
    Ok(())
}

fn push_start_child(
    store: DomQuerySelectorStore<'_>,
    from: DomNodeSlotKey,
    stack: &mut Vec<DomNodeSlotKey>,
) -> Result<(), DomError> {
    let first_child_key = store
        .nodes
        .get(from)
        .ok_or(DomError::InvalidNodeKey)?
        .first_child_key_for_store();
    if let Some(first_child_key) = first_child_key {
        stack.push(first_child_key);
    }
    Ok(())
}

fn matches_selector_path(
    store: DomQuerySelectorStore<'_>,
    from: DomNodeSlotKey,
    candidate: &DomNode,
    selector: &[SimpleSelector],
) -> Result<bool, DomError> {
    let Some(last_step) = selector.last() else {
        return Err(DomError::InvalidSelector);
    };
    if !matches_simple_selector(candidate, last_step) {
        return Ok(false);
    }

    let mut ancestor_key = candidate.parent_key();
    for step in selector.iter().rev().skip(1) {
        ancestor_key = find_matching_ancestor(store, from, ancestor_key, step)?;
        if let Some(matched_key) = ancestor_key {
            ancestor_key = store
                .nodes
                .get(matched_key)
                .ok_or(DomError::InvalidNodeKey)?
                .parent_key();
            continue;
        }
        return Ok(false);
    }
    Ok(true)
}

fn find_matching_ancestor(
    store: DomQuerySelectorStore<'_>,
    from: DomNodeSlotKey,
    mut ancestor_key: Option<DomNodeSlotKey>,
    step: &SimpleSelector,
) -> Result<Option<DomNodeSlotKey>, DomError> {
    while let Some(current_key) = ancestor_key {
        if current_key == from {
            return Ok(None);
        }
        let current = store
            .nodes
            .get(current_key)
            .ok_or(DomError::InvalidNodeKey)?;
        if matches_simple_selector(current, step) {
            return Ok(Some(current_key));
        }
        ancestor_key = current.parent_key();
    }
    Ok(None)
}

fn matches_simple_selector(node: &DomNode, selector: &SimpleSelector) -> bool {
    let Some(element) = node.as_element() else {
        return false;
    };

    if let Some(expected_id) = selector.id.as_deref()
        && element.attrs.id.as_deref() != Some(expected_id)
    {
        return false;
    }

    if let Some(expected_tag) = selector.tag.as_deref()
        && !tag_name_matches(&element.tag, expected_tag)
    {
        return false;
    }

    if !selector.classes.is_empty() {
        let class_list = DomAttributeNode::class_list(element);
        if class_list.len() < selector.classes.len() {
            return false;
        }
        for expected_class in &selector.classes {
            if !class_list.iter().any(|class| class == expected_class) {
                return false;
            }
        }
    }

    true
}

fn tag_name_matches(tag: &crate::types::dom_tag::DomTag, expected: &str) -> bool {
    tag.selector_name().eq_ignore_ascii_case(expected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_selector_accepts_supported_shapes() {
        assert!(parse_selector("#hero").is_ok());
        assert!(parse_selector(".primary").is_ok());
        assert!(parse_selector(".primary.action").is_ok());
        assert!(parse_selector("div").is_ok());
        assert!(parse_selector("button.primary").is_ok());
        assert!(parse_selector("button.primary.action").is_ok());
        assert!(parse_selector("panel button.primary").is_ok());
        assert!(parse_selector(".panel .primary").is_ok());
        assert!(parse_selector(".panel .primary.action").is_ok());
    }

    #[test]
    fn parse_selector_rejects_unsupported_shapes() {
        assert_eq!(parse_selector("").unwrap_err(), DomError::InvalidSelector);
        assert_eq!(
            parse_selector(".primary..action").unwrap_err(),
            DomError::InvalidSelector
        );
        assert_eq!(
            parse_selector("button > .primary").unwrap_err(),
            DomError::InvalidSelector
        );
        assert_eq!(
            parse_selector("button:hover").unwrap_err(),
            DomError::InvalidSelector
        );
        assert_eq!(
            parse_selector("button, .primary").unwrap_err(),
            DomError::InvalidSelector
        );
        assert_eq!(parse_selector("*").unwrap_err(), DomError::InvalidSelector);
        assert_eq!(
            parse_selector("#hero!").unwrap_err(),
            DomError::InvalidSelector
        );
        assert_eq!(
            parse_selector("#hero.primary").unwrap_err(),
            DomError::InvalidSelector
        );
    }
}
