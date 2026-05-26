use std::collections::HashMap;

use crate::types::DomError;
use crate::types::dom_node::{DomNode, DomNodeSlotKey};
use crate::types::root_dom::append_child_to_parent;
use crate::types::root_dom_fragment::RootDomFragment;
use slotmap::SlotMap;

pub(crate) fn append_fragment_children_into_owner<F>(
    owner_root: DomNodeSlotKey,
    owner_nodes: &mut SlotMap<DomNodeSlotKey, DomNode>,
    owner_id_lookup: &mut HashMap<String, DomNodeSlotKey>,
    parent_key: DomNodeSlotKey,
    fragment: &RootDomFragment,
    import_text_node: &mut F,
) -> Result<Vec<DomNodeSlotKey>, DomError>
where
    F: FnMut(DomNodeSlotKey, &str) -> DomNode,
{
    assert_fragment_parent_is_valid(owner_nodes, parent_key)?;
    let top_level_child_keys = fragment.top_level_child_keys();
    assert_fragment_ids_are_available_in_owner(owner_id_lookup, fragment, &top_level_child_keys)?;
    let mut imported_keys = Vec::new();
    for child_key in top_level_child_keys {
        let imported_key = import_fragment_subtree_into_owner(
            owner_root,
            owner_nodes,
            owner_id_lookup,
            parent_key,
            fragment,
            child_key,
            import_text_node,
        )?;
        imported_keys.push(imported_key);
    }
    Ok(imported_keys)
}

fn assert_fragment_parent_is_valid(
    owner_nodes: &SlotMap<DomNodeSlotKey, DomNode>,
    parent_key: DomNodeSlotKey,
) -> Result<(), DomError> {
    let parent = owner_nodes
        .get(parent_key)
        .ok_or(DomError::InvalidNodeKey)?;
    if !parent.is_parent_node() {
        return Err(DomError::InvalidParentTarget);
    }
    Ok(())
}

fn assert_fragment_ids_are_available_in_owner(
    owner_id_lookup: &HashMap<String, DomNodeSlotKey>,
    fragment: &RootDomFragment,
    top_level_child_keys: &[DomNodeSlotKey],
) -> Result<(), DomError> {
    let mut stack = top_level_child_keys.to_vec();
    while let Some(current_key) = stack.pop() {
        let node = fragment.node(current_key).ok_or(DomError::InvalidNodeKey)?;
        if let Some(id) = node
            .as_element()
            .and_then(|element| element.attrs.id.as_deref())
        {
            assert_owner_id_is_available(owner_id_lookup, Some(id))?;
        }

        let mut child_key = node
            .first_child(|key| fragment.node(key))
            .map(|(key, _)| key);
        while let Some(current_child_key) = child_key {
            let child = fragment
                .node(current_child_key)
                .ok_or(DomError::InvalidNodeKey)?;
            child_key = child
                .next_sibling(|key| fragment.node(key))
                .map(|(key, _)| key);
            stack.push(current_child_key);
        }
    }
    Ok(())
}

fn import_fragment_subtree_into_owner<F>(
    owner_root: DomNodeSlotKey,
    owner_nodes: &mut SlotMap<DomNodeSlotKey, DomNode>,
    owner_id_lookup: &mut HashMap<String, DomNodeSlotKey>,
    parent_key: DomNodeSlotKey,
    fragment: &RootDomFragment,
    fragment_key: DomNodeSlotKey,
    import_text_node: &mut F,
) -> Result<DomNodeSlotKey, DomError>
where
    F: FnMut(DomNodeSlotKey, &str) -> DomNode,
{
    let fragment_node = fragment
        .node(fragment_key)
        .ok_or(DomError::InvalidNodeKey)?;
    let imported_key = match fragment_node {
        DomNode::Document(_) | DomNode::DocumentFragment(_) => {
            return Err(DomError::InvalidParentTarget);
        }
        DomNode::Element(_) => insert_node_into_owner(
            owner_nodes,
            owner_id_lookup,
            fragment_node.clone_for_owner_document(owner_root),
        )?,
        DomNode::Text(text_node) => insert_node_into_owner(
            owner_nodes,
            owner_id_lookup,
            import_text_node(owner_root, text_node.data()),
        )?,
    };

    append_child_to_parent(owner_nodes, parent_key, imported_key)?;

    let mut child_key = fragment_node
        .first_child(|key| fragment.node(key))
        .map(|(key, _)| key);
    while let Some(current_child_key) = child_key {
        let child = fragment
            .node(current_child_key)
            .ok_or(DomError::InvalidNodeKey)?;
        child_key = child
            .next_sibling(|key| fragment.node(key))
            .map(|(key, _)| key);
        import_fragment_subtree_into_owner(
            owner_root,
            owner_nodes,
            owner_id_lookup,
            imported_key,
            fragment,
            current_child_key,
            import_text_node,
        )?;
    }
    Ok(imported_key)
}

fn insert_node_into_owner(
    owner_nodes: &mut SlotMap<DomNodeSlotKey, DomNode>,
    owner_id_lookup: &mut HashMap<String, DomNodeSlotKey>,
    mut node: DomNode,
) -> Result<DomNodeSlotKey, DomError> {
    assert_owner_id_is_available(
        owner_id_lookup,
        node.as_element()
            .and_then(|element| element.attrs.id.as_deref()),
    )?;
    node.after_create_intrinsic_update()?;
    let key = owner_nodes.insert(node);
    index_owner_node_id_if_present(owner_nodes, owner_id_lookup, key);
    Ok(key)
}

fn assert_owner_id_is_available(
    owner_id_lookup: &HashMap<String, DomNodeSlotKey>,
    id: Option<&str>,
) -> Result<(), DomError> {
    let Some(id) = id else {
        return Ok(());
    };
    if owner_id_lookup.contains_key(id) {
        return Err(DomError::DuplicateElementId);
    }
    Ok(())
}

fn index_owner_node_id_if_present(
    owner_nodes: &SlotMap<DomNodeSlotKey, DomNode>,
    owner_id_lookup: &mut HashMap<String, DomNodeSlotKey>,
    key: DomNodeSlotKey,
) {
    let Some(id) = owner_nodes
        .get(key)
        .and_then(DomNode::as_element)
        .and_then(|element| element.attrs.id.clone())
    else {
        return;
    };
    owner_id_lookup.insert(id, key);
}
