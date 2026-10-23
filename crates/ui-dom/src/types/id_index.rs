//! The element-id index for the DOM slot stores.
//!
//! What this file represents:
//!
//! - the id mirror: a `HashMap<String, DomNodeSlotKey>` whose entries exactly
//!   reflect the elements that currently carry an `id` attribute
//! - the maintained-index operations over that mirror: assert-free (guard the
//!   insert), rename (replace one indexed id with another), and the authored
//!   `id`-attribute value extraction
//!
//! Invariants and rules:
//!
//! - the mirror is exact: an id maps to a key iff that key's element carries
//!   that id; clearing or renaming the `id` attribute must de-index or
//!   re-index within the same mutation
//! - an id maps to at most one key; a second owner is a `DuplicateElementId`
//!   error
//!
//! Two assert variants (triage (b), meaningful divergence — not accidental):
//!
//! - `assert_id_is_available` guards NEW-node insertion (batch import): the id
//!   must be wholly absent, because a fresh node can own nothing yet.
//! - `assert_id_is_available_for` guards a single `set_attr` write on an
//!   EXISTING key: the id may already be owned by `key` itself (a no-op id
//!   re-set), so only a DIFFERENT owner is rejected.
//!
//! Extraction note (PR 5, E2): `index_node_id_if_present` (the mirror's
//! insert-from-node operation) STAYED in `root_dom.rs`. Its node-store read
//! goes through the metric-instrumented `RootDomNodes::get`; re-homing it as a
//! free function over `&SlotMap` would drop that `Get` increment and move the
//! `append+update` perf pin. It re-homes once the insert primitive takes a
//! pre-resolved `Option<String>` id (a signature change, deferred).
use std::collections::HashMap;

use crate::types::dom_node::DomNodeSlotKey;
use crate::types::{DomAttribute, DomAttributeValue, DomError};

#[cfg(test)]
use crate::types::dom_node::DomNode;
#[cfg(test)]
use crate::types::dom_tag::DomTag;
#[cfg(test)]
use slotmap::SlotMap;

pub(crate) fn assert_id_is_available(
    id_lookup: &HashMap<String, DomNodeSlotKey>,
    id: Option<&str>,
) -> Result<(), DomError> {
    let Some(id) = id else {
        return Ok(());
    };
    if id_lookup.contains_key(id) {
        return Err(DomError::DuplicateElementId);
    }
    Ok(())
}

pub(crate) fn assert_id_is_available_for(
    id_lookup: &HashMap<String, DomNodeSlotKey>,
    key: DomNodeSlotKey,
    id: Option<&str>,
) -> Result<(), DomError> {
    let Some(id) = id else {
        return Ok(());
    };
    match id_lookup.get(id) {
        Some(existing_key) if *existing_key != key => Err(DomError::DuplicateElementId),
        _ => Ok(()),
    }
}

pub(crate) fn replace_indexed_id(
    id_lookup: &mut HashMap<String, DomNodeSlotKey>,
    key: DomNodeSlotKey,
    previous_id: Option<String>,
    next_id: Option<String>,
) {
    if let Some(previous_id) = previous_id {
        id_lookup.remove(&previous_id);
    }
    if let Some(next_id) = next_id {
        id_lookup.insert(next_id, key);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn key_pair() -> (DomNodeSlotKey, DomNodeSlotKey) {
        let mut nodes = SlotMap::with_key();
        let first = nodes.insert_with_key(DomNode::document);
        let second = nodes.insert(DomNode::element(first, DomTag::Div));
        (first, second)
    }

    #[test]
    fn mirror_inserts_looks_up_and_removes_by_id() {
        let mut index = HashMap::new();
        let (key, _) = key_pair();

        replace_indexed_id(&mut index, key, None, Some("subject".to_string()));
        assert_eq!(index.get("subject"), Some(&key));

        replace_indexed_id(&mut index, key, Some("subject".to_string()), None);
        assert_eq!(index.get("subject"), None);
        assert!(index.is_empty());
    }

    #[test]
    fn rename_replaces_the_indexed_id() {
        let mut index = HashMap::new();
        let (key, _) = key_pair();

        replace_indexed_id(&mut index, key, None, Some("old".to_string()));
        replace_indexed_id(
            &mut index,
            key,
            Some("old".to_string()),
            Some("new".to_string()),
        );

        assert_eq!(index.get("old"), None);
        assert_eq!(index.get("new"), Some(&key));
        assert_eq!(index.len(), 1);
    }

    #[test]
    fn assert_id_is_available_fires_on_duplicate_id() {
        let mut index = HashMap::new();
        let (key, _) = key_pair();
        replace_indexed_id(&mut index, key, None, Some("dup".to_string()));

        assert_eq!(
            assert_id_is_available(&index, Some("dup")),
            Err(DomError::DuplicateElementId)
        );
        assert_eq!(assert_id_is_available(&index, Some("fresh")), Ok(()));
        assert_eq!(assert_id_is_available(&index, None), Ok(()));
    }

    #[test]
    fn clearing_id_deindexes_the_node() {
        let mut index = HashMap::new();
        let (key, _) = key_pair();
        replace_indexed_id(&mut index, key, None, Some("subject".to_string()));

        replace_indexed_id(&mut index, key, Some("subject".to_string()), None);
        assert_eq!(index.get("subject"), None);
    }

    #[test]
    fn assert_id_is_available_for_rejects_only_other_keys() {
        let mut index = HashMap::new();
        let (key, other) = key_pair();
        replace_indexed_id(&mut index, key, None, Some("owned".to_string()));

        // the owning key may re-affirm its own id
        assert_eq!(
            assert_id_is_available_for(&index, key, Some("owned")),
            Ok(())
        );
        // a different key may not
        assert_eq!(
            assert_id_is_available_for(&index, other, Some("owned")),
            Err(DomError::DuplicateElementId)
        );
        assert_eq!(assert_id_is_available_for(&index, key, None), Ok(()));
    }

    #[test]
    fn next_id_from_attr_returns_only_string_id_values() {
        let id = DomAttribute::Id;
        let class = DomAttribute::Class;

        assert_eq!(
            next_id_from_attr(&id, Some(&DomAttributeValue::String("x".to_string()))),
            Ok(Some("x".to_string()))
        );
        assert_eq!(next_id_from_attr(&id, None), Ok(None));
        assert_eq!(
            next_id_from_attr(&class, Some(&DomAttributeValue::String("x".to_string()))),
            Ok(None)
        );
        assert_eq!(
            next_id_from_attr(&id, Some(&DomAttributeValue::Bool(true))),
            Err(DomError::InvalidAttributeValue)
        );
    }
}
