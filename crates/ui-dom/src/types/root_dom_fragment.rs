//! `RootDomFragment` is one detached staging owner for fragment-authored DOM.
//!
//! What this type represents:
//!
//! - one fragment-owned slot store for detached authored nodes
//! - one staging area whose automatic root node is a `DocumentFragment`
//! - one temporary authoring tree that can be consumed by `RootDom`
//!
//! Invariants and rules:
//!
//! - every `RootDomFragment` owns exactly one internal `DocumentFragment` root
//! - fragment node keys are only valid inside that fragment's slot store
//! - top-level staged children are the children of `document_fragment_key()`
//! - child order is explicit and stable through sibling links
//! - cycles are forbidden
//! - only nodes reachable from the `DocumentFragment` root are imported into
//!   `RootDom`
//! - unreachable fragment nodes are discarded when the fragment is dropped or
//!   consumed
//! - `RootDomFragment` owns fragment-local id indexing and subtree removal
//! - `RootDomFragment` is detached authoring only; attached document mutation
//!   belongs to `RootDom`
//!
//! Advice:
//!
//! - use `document_fragment_key()` when you want to append top-level staged
//!   children
//! - keep fragment building DOM-shaped: create node, then `append_child`
//! - consume the fragment through `RootDom::append_fragment` rather than
//!   copying fragment internals into callers
//! - use element ids plus normal DOM traversal after import instead of leaking
//!   fragment-local keys past the staging boundary
use std::collections::HashMap;

use crate::types::dom_attr_parse::normalize_attribute_value;
use crate::types::dom_event_listener::{
    DomEventListener, DomEventListenerOptions, add_event_listener_to_node,
    remove_event_listener_from_node,
};
use crate::types::dom_event_name::DomEventName;
use crate::types::dom_node::{
    DomIntrinsic, DomNode, DomNodeSlotKey, measure_text_intrinsic_from_content,
};
use crate::types::dom_ops::append_fragment_children_into_owner;
use crate::types::dom_query_selector::{
    DomQuerySelectorStore, query_selector as query_selector_in_store,
    query_selector_all as query_selector_all_in_store,
    query_selector_all_from as query_selector_all_from_in_store,
    query_selector_from as query_selector_from_in_store,
};
use crate::types::dom_tag::DomTag;
use crate::types::root_dom::{
    append_child_to_parent, collect_subtree_keys, detach_node_from_parent, next_id_from_attr,
};
use crate::types::{
    DomAttribute, DomAttributeValue, DomError, DomEventListenerOwner, parse_authored_value_input,
};
use slotmap::SlotMap;

#[derive(Debug)]
pub struct RootDomFragment {
    pub(crate) root: DomNodeSlotKey,
    pub(crate) nodes: SlotMap<DomNodeSlotKey, DomNode>,
    id_lookup: HashMap<String, DomNodeSlotKey>,
}

impl RootDomFragment {
    /// Creates one detached fragment owner with one `DocumentFragment` root node.
    ///
    /// This is the fragment entrypoint callers use to stage authored DOM
    /// content before importing it into a live `RootDom`. The returned fragment
    /// owns its own slot store and id index until consumption.
    #[must_use]
    pub fn new() -> Self {
        let mut nodes = SlotMap::with_key();
        let root = nodes.insert_with_key(DomNode::document_fragment);
        Self {
            root,
            nodes,
            id_lookup: HashMap::new(),
        }
    }

    /// Returns the stable key of this fragment's `DocumentFragment` root node.
    ///
    /// Callers use this key as the top-level append target while building a
    /// detached fragment tree. It exposes the fragment root explicitly instead
    /// of hiding top-level staging behavior behind special cases.
    #[must_use]
    pub fn document_fragment_key(&self) -> DomNodeSlotKey {
        self.root
    }

    /// Creates one staged element node in this fragment-owned slot store.
    ///
    /// This allocates one element inside the fragment without attaching it yet.
    /// Callers are expected to build the fragment tree explicitly through later
    /// `append_child` calls.
    pub fn create_element(&mut self, tag: DomTag) -> Result<DomNodeSlotKey, DomError> {
        self.insert_node(DomNode::element(self.root, tag))
    }

    /// Creates one staged text node in this fragment-owned slot store.
    ///
    /// This allocates one detached text node owned by the fragment's document
    /// root. It exists so fragment callers can build mixed element/text content
    /// before a later import into a live document.
    pub fn create_text_node(
        &mut self,
        data: impl Into<String>,
    ) -> Result<DomNodeSlotKey, DomError> {
        self.insert_node(DomNode::text(self.root, data.into().into()))
    }

    /// Returns one staged node by its stable key.
    ///
    /// This is the fragment-local lookup seam for callers that already hold a
    /// staged key. The key is meaningful only inside this fragment's slot store
    /// and should not leak past the staging boundary.
    #[must_use]
    pub fn node(&self, key: DomNodeSlotKey) -> Option<&DomNode> {
        self.nodes.get(key)
    }

    /// Returns one staged element by its authored `id`.
    ///
    /// This provides fragment-local id lookup so detached authoring can remain
    /// DOM-shaped before import. It does not walk the fragment tree because the
    /// fragment maintains its own retained id index.
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

    /// Returns the first staged node matching the provided selector in document order.
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

    /// Returns all staged nodes matching the provided selector in document order.
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

    /// Appends one staged child to one staged parent already stored in this fragment.
    ///
    /// This is the fragment-local tree mutation seam for callers building
    /// detached content incrementally. It preserves the same DOM-shaped parent
    /// and sibling rules used by live `RootDom` trees.
    pub fn append_child(
        &mut self,
        parent_key: DomNodeSlotKey,
        child_key: DomNodeSlotKey,
    ) -> Result<DomNodeSlotKey, DomError> {
        append_child_to_parent(&mut self.nodes, parent_key, child_key)
    }

    /// Appends one detached fragment root's children under one staged parent
    /// in this fragment-owned tree.
    ///
    /// The source fragment root itself is not imported. Only children
    /// reachable from that root are appended, in order. Duplicate element ids
    /// fail before partial import.
    pub fn append_fragment(
        &mut self,
        parent_key: DomNodeSlotKey,
        fragment: &RootDomFragment,
    ) -> Result<(), DomError> {
        append_fragment_children_into_owner(
            self.root,
            &mut self.nodes,
            &mut self.id_lookup,
            parent_key,
            fragment,
            &mut |owner_root, text| DomNode::text(owner_root, text.into()),
        )?;
        Ok(())
    }

    /// Attaches one event listener to the targeted staged node.
    ///
    /// This keeps detached listener authoring aligned with live `RootDom`
    /// authoring while the subtree is still fragment-owned.
    pub fn add_event_listener(
        &mut self,
        key: DomNodeSlotKey,
        event_name: DomEventName,
        listener: impl Into<DomEventListener>,
        options: Option<DomEventListenerOptions>,
    ) -> Result<(), DomError> {
        <Self as crate::types::DomEventListenerOwner>::add_event_listener(
            self, key, event_name, listener, options,
        )
    }

    /// Removes one matching listener from the targeted staged node.
    pub fn remove_event_listener(
        &mut self,
        key: DomNodeSlotKey,
        event_name: DomEventName,
        listener: impl Into<DomEventListener>,
        options: Option<DomEventListenerOptions>,
    ) -> Result<bool, DomError> {
        <Self as crate::types::DomEventListenerOwner>::remove_event_listener(
            self, key, event_name, listener, options,
        )
    }

    /// Removes one non-root staged node together with its full subtree.
    ///
    /// This deletes staged content strictly inside the fragment-owned slot
    /// store while keeping fragment-local id bookkeeping in sync. It does not
    /// affect any live document because fragments are detached authoring only.
    pub fn remove_node(&mut self, key: DomNodeSlotKey) -> Result<(), DomError> {
        if key == self.root {
            return Err(DomError::CannotRemoveRoot);
        }
        if !self.nodes.contains_key(key) {
            return Err(DomError::InvalidNodeKey);
        }

        let removed_keys = collect_subtree_keys(&self.nodes, key)?;
        let _ = detach_node_from_parent(&mut self.nodes, key)?;

        for &removed_key in &removed_keys {
            self.remove_indexed_id_for_node(removed_key);
        }
        for removed_key in removed_keys {
            let removed = self.nodes.remove(removed_key);
            assert!(
                removed.is_some(),
                "validated fragment subtree key should remove"
            );
        }
        Ok(())
    }

    /// Sets one node attribute while keeping the fragment-local id lookup synchronized.
    ///
    /// This is the fragment-owned mutation path for authored attrs before
    /// import. It exists so detached authoring can obey the same validation and
    /// id-integrity rules as live documents.
    pub fn set_attr(
        &mut self,
        key: DomNodeSlotKey,
        attribute: DomAttribute,
        value: Option<DomAttributeValue>,
    ) -> Result<(), DomError> {
        let value = normalize_attribute_value(&attribute, value)?;
        let is_id_attribute = attribute == DomAttribute::Id;
        let previous_id = self.current_element_id(key);
        let next_id = next_id_from_attr(&attribute, value.as_ref())?;
        self.assert_id_is_available_for(key, next_id.as_deref())?;
        let attribute_for_refresh = attribute.clone();
        let current_node = self.nodes.get(key).ok_or(DomError::InvalidNodeKey)?;
        if current_node.attr(&attribute_for_refresh) == value.as_ref() {
            return Ok(());
        }
        let prepared_intrinsic_after_attr_write = current_node
            .before_attr_write_intrinsic_update(&attribute_for_refresh, value.as_ref())?;
        let node = self.nodes.get_mut(key).ok_or(DomError::InvalidNodeKey)?;
        node.set_attr(attribute, value)?;
        let _intrinsic_changed =
            node.after_attr_write_intrinsic_update(prepared_intrinsic_after_attr_write);

        if is_id_attribute {
            self.replace_indexed_id(key, previous_id, next_id);
        }
        Ok(())
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
        let mut next_intrinsic = measure_text_intrinsic_from_content(text);
        if let DomIntrinsic::Text(current_intrinsic) = &text_node.base.intrinsic {
            next_intrinsic.generation = current_intrinsic.generation.wrapping_add(1);
        }
        let next_text = std::sync::Arc::<str>::from(text);
        let node = self.nodes.get_mut(key).ok_or(DomError::InvalidNodeKey)?;
        let DomNode::Text(text_node) = node else {
            return Err(DomError::InvalidAttributeTarget);
        };
        text_node.set_data(next_text);
        text_node.set_text_run_id_hint(None);
        text_node.base.intrinsic = DomIntrinsic::Text(next_intrinsic);
        Ok(())
    }

    pub(crate) fn top_level_child_keys(&self) -> Vec<DomNodeSlotKey> {
        let mut child_keys = Vec::new();
        let Some(root) = self.nodes.get(self.root) else {
            return child_keys;
        };
        let mut child_key = root
            .first_child(|key| self.nodes.get(key))
            .map(|(key, _)| key);
        while let Some(current_key) = child_key {
            child_keys.push(current_key);
            child_key = self
                .nodes
                .get(current_key)
                .and_then(|node| node.next_sibling(|key| self.nodes.get(key)))
                .map(|(key, _)| key);
        }
        child_keys
    }

    fn insert_node(&mut self, node: DomNode) -> Result<DomNodeSlotKey, DomError> {
        self.assert_id_is_available(
            node.as_element()
                .and_then(|element| element.attrs.id.as_deref()),
        )?;
        let mut node = node;
        node.after_create_intrinsic_update()?;
        let key = self.nodes.insert(node);
        self.index_node_id_if_present(key);
        Ok(key)
    }

    fn query_selector_store(&self) -> DomQuerySelectorStore<'_> {
        DomQuerySelectorStore {
            root: self.root,
            nodes: &self.nodes,
            id_lookup: &self.id_lookup,
        }
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

    fn remove_indexed_id_for_node(&mut self, key: DomNodeSlotKey) {
        let Some(id) = self
            .nodes
            .get(key)
            .and_then(DomNode::as_element)
            .and_then(|element| element.attrs.id.as_deref())
        else {
            return;
        };
        self.id_lookup.remove(id);
    }
}

impl DomEventListenerOwner for RootDomFragment {
    fn add_event_listener(
        &mut self,
        key: DomNodeSlotKey,
        event_name: DomEventName,
        listener: impl Into<DomEventListener>,
        options: Option<DomEventListenerOptions>,
    ) -> Result<(), DomError> {
        let node = self.nodes.get_mut(key).ok_or(DomError::InvalidNodeKey)?;
        let _added = add_event_listener_to_node(node, event_name, listener, options);
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

impl Default for RootDomFragment {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use crate::types::dom_node::DomNode;
    use crate::types::dom_tag::DomTag;
    use crate::types::root_dom::RootDom;
    use crate::types::root_dom_context::RootDomContext;
    use crate::types::{
        DomAttribute, DomAttributeNode, DomAttributeValue, DomError, DomIntrinsic,
        parse_css_color_rgba,
    };

    use super::RootDomFragment;

    fn query_match_ids(
        matches: Vec<(crate::types::dom_node::DomNodeSlotKey, &DomNode)>,
    ) -> Vec<String> {
        matches
            .into_iter()
            .map(|(_, node)| {
                node.as_element()
                    .and_then(|element| element.attrs.id.clone())
                    .expect("query result should be element with id")
            })
            .collect()
    }

    fn color(value: &str) -> DomAttributeValue {
        DomAttributeValue::Color(parse_css_color_rgba(value).expect("test color should parse"))
    }

    fn build_query_test_fragment(
        fragment: &mut RootDomFragment,
    ) -> (
        crate::types::dom_node::DomNodeSlotKey,
        crate::types::dom_node::DomNodeSlotKey,
        crate::types::dom_node::DomNodeSlotKey,
        crate::types::dom_node::DomNodeSlotKey,
        crate::types::dom_node::DomNodeSlotKey,
    ) {
        let container = fragment
            .create_element(DomTag::Custom("Panel".to_string()))
            .expect("container");
        let branch = fragment.create_element(DomTag::Div).expect("branch");
        let launch = fragment.create_element(DomTag::Button).expect("launch");
        let label = fragment.create_element(DomTag::Span).expect("label");
        let item = fragment.create_element(DomTag::Div).expect("item");
        let sibling = fragment.create_element(DomTag::Span).expect("sibling");
        let outside = fragment.create_element(DomTag::Button).expect("outside");
        let text = fragment.create_text_node("ignored").expect("text");

        fragment
            .append_child(fragment.document_fragment_key(), container)
            .expect("append container");
        fragment
            .append_child(container, branch)
            .expect("append branch");
        fragment
            .append_child(branch, launch)
            .expect("append launch");
        fragment.append_child(launch, label).expect("append label");
        fragment.append_child(branch, item).expect("append item");
        fragment
            .append_child(container, sibling)
            .expect("append sibling");
        fragment
            .append_child(fragment.document_fragment_key(), text)
            .expect("append text");
        fragment
            .append_child(fragment.document_fragment_key(), outside)
            .expect("append outside");

        fragment
            .set_attr_str(container, "id", "container")
            .expect("container id");
        fragment
            .set_attr_str(container, "class", "panel")
            .expect("container class");
        fragment
            .set_attr_str(branch, "id", "branch")
            .expect("branch id");
        fragment
            .set_attr_str(branch, "class", "panel")
            .expect("branch class");
        fragment
            .set_attr_str(launch, "id", "launch")
            .expect("launch id");
        fragment
            .set_attr_str(launch, "class", "primary action")
            .expect("launch class");
        fragment
            .set_attr_str(label, "id", "label")
            .expect("label id");
        fragment
            .set_attr_str(label, "class", "label")
            .expect("label class");
        fragment.set_attr_str(item, "id", "item").expect("item id");
        fragment
            .set_attr_str(item, "class", "item")
            .expect("item class");
        fragment
            .set_attr_str(sibling, "id", "sibling")
            .expect("sibling id");
        fragment
            .set_attr_str(sibling, "class", "primary")
            .expect("sibling class");
        fragment
            .set_attr_str(outside, "id", "outside")
            .expect("outside id");
        fragment
            .set_attr_str(outside, "class", "primary")
            .expect("outside class");

        (container, branch, launch, label, outside)
    }

    #[test]
    fn new_fragment_starts_with_document_fragment_root() {
        let fragment = RootDomFragment::new();

        assert!(matches!(
            fragment.node(fragment.document_fragment_key()),
            Some(crate::types::DomNode::DocumentFragment(_))
        ));
    }

    #[test]
    fn append_child_supports_multiple_top_level_children() {
        let mut fragment = RootDomFragment::new();
        let fragment_root = fragment.document_fragment_key();
        let a = fragment.create_element(DomTag::Div).expect("a");
        let b = fragment.create_element(DomTag::Span).expect("b");

        fragment
            .append_child(fragment_root, a)
            .expect("first append");
        fragment
            .append_child(fragment_root, b)
            .expect("second append");

        assert_eq!(fragment.top_level_child_keys(), vec![a, b]);
    }

    #[test]
    fn set_text_content_remeasures_text_intrinsic_in_fragment() {
        let mut fragment = RootDomFragment::new();
        let text_key = fragment
            .create_text_node("tiny")
            .expect("text should insert");

        let before = match fragment
            .node(text_key)
            .expect("text should exist")
            .intrinsic()
        {
            DomIntrinsic::Text(metrics) => metrics.clone(),
            other => panic!("expected text intrinsic, got {other:?}"),
        };

        fragment
            .set_text_content(text_key, "much wider detached text")
            .expect("text content should update");

        let after = match fragment
            .node(text_key)
            .expect("text should exist")
            .intrinsic()
        {
            DomIntrinsic::Text(metrics) => metrics,
            other => panic!("expected text intrinsic, got {other:?}"),
        };
        assert!(after.max_content_width_px > before.max_content_width_px);
    }

    #[test]
    fn set_text_content_is_noop_when_fragment_text_is_unchanged() {
        let mut fragment = RootDomFragment::new();
        let text_key = fragment
            .create_text_node("stable")
            .expect("text should insert");
        let before = fragment
            .node(text_key)
            .expect("text should exist")
            .intrinsic()
            .clone();

        fragment
            .set_text_content(text_key, "stable")
            .expect("unchanged text should succeed");

        assert_eq!(
            fragment
                .node(text_key)
                .expect("text should still exist")
                .intrinsic(),
            &before
        );
    }

    #[test]
    fn img_intrinsic_materializes_placeholder_and_one_sided_sizes_in_fragment() {
        let mut fragment = RootDomFragment::new();
        let img_key = fragment
            .create_element(DomTag::Img)
            .expect("img should insert");

        fragment
            .set_attr(
                img_key,
                DomAttribute::Src,
                Some(DomAttributeValue::String("memory://a.png".to_string())),
            )
            .expect("src should set");
        match fragment
            .node(img_key)
            .expect("img should exist")
            .intrinsic()
        {
            DomIntrinsic::Img(metrics) => {
                assert!((metrics.width_px - 160.0).abs() < f32::EPSILON);
                assert!((metrics.height_px - 90.0).abs() < f32::EPSILON);
            }
            other => panic!("expected placeholder img intrinsic, got {other:?}"),
        }

        fragment
            .set_attr(
                img_key,
                DomAttribute::Width,
                Some(DomAttributeValue::String("96px".to_string())),
            )
            .expect("width should set");
        match fragment
            .node(img_key)
            .expect("img should exist")
            .intrinsic()
        {
            DomIntrinsic::Img(metrics) => {
                assert!((metrics.width_px - 96.0).abs() < f32::EPSILON);
                assert!((metrics.height_px - 54.0).abs() < f32::EPSILON);
            }
            other => panic!("expected one-sided img intrinsic, got {other:?}"),
        }
        fragment
            .set_attr(
                img_key,
                DomAttribute::Height,
                Some(DomAttributeValue::String("48px".to_string())),
            )
            .expect("height should set");

        match fragment
            .node(img_key)
            .expect("img should exist")
            .intrinsic()
        {
            DomIntrinsic::Img(metrics) => {
                assert!((metrics.width_px - 96.0).abs() < f32::EPSILON);
                assert!((metrics.height_px - 48.0).abs() < f32::EPSILON);
            }
            other => panic!("expected img intrinsic, got {other:?}"),
        }
    }

    #[test]
    fn failed_img_attr_update_does_not_partially_mutate_fragment_node() {
        let mut fragment = RootDomFragment::new();
        let img_key = fragment
            .create_element(DomTag::Img)
            .expect("img should insert");

        let error = fragment
            .set_attr(
                img_key,
                DomAttribute::Src,
                Some(DomAttributeValue::String(String::new())),
            )
            .expect_err("empty src should fail");
        assert_eq!(error, DomError::InvalidAttributeValue);
        assert_eq!(
            fragment
                .node(img_key)
                .expect("img should exist")
                .attr(&DomAttribute::Src),
            None
        );
    }

    #[test]
    fn fragment_set_attr_indexes_element_ids() {
        let mut fragment = RootDomFragment::new();
        let element = fragment.create_element(DomTag::Div).expect("element");

        fragment
            .set_attr(
                element,
                DomAttribute::Id,
                Some(DomAttributeValue::String("hero".to_string())),
            )
            .expect("id should apply");

        assert_eq!(
            fragment
                .get_element_by_id("hero")
                .expect("id should resolve")
                .0,
            element
        );
    }

    #[test]
    fn fragment_query_selector_matches_supported_shapes() {
        let mut fragment = RootDomFragment::new();
        let (_, branch, launch, label, _) = build_query_test_fragment(&mut fragment);

        assert_eq!(
            fragment
                .query_selector("#launch")
                .expect("id selector")
                .map(|(key, _)| key),
            Some(launch)
        );
        assert_eq!(
            fragment
                .query_selector(".primary")
                .expect("class selector")
                .map(|(key, _)| key),
            Some(launch)
        );
        assert_eq!(
            fragment
                .query_selector("button")
                .expect("tag selector")
                .map(|(key, _)| key),
            Some(launch)
        );
        assert_eq!(
            fragment
                .query_selector("button.primary")
                .expect("tag.class selector")
                .map(|(key, _)| key),
            Some(launch)
        );
        assert_eq!(
            fragment
                .query_selector(".primary.action")
                .expect("multi-class selector")
                .map(|(key, _)| key),
            Some(launch)
        );
        assert_eq!(
            fragment
                .query_selector("button.primary.action")
                .expect("tag multi-class selector")
                .map(|(key, _)| key),
            Some(launch)
        );
        assert_eq!(
            fragment
                .query_selector("panel")
                .expect("custom tag selector")
                .map(|(key, _)| key),
            Some(
                fragment
                    .get_element_by_id("container")
                    .expect("container")
                    .0
            )
        );
        assert_eq!(
            fragment
                .query_selector("div .item")
                .expect("descendant selector")
                .and_then(|(_, node)| node.as_element())
                .and_then(|element| element.attrs.id.as_deref())
                .map(str::to_string),
            Some("item".to_string())
        );
        assert_eq!(
            fragment
                .query_selector("panel button.primary")
                .expect("custom tag descendant selector")
                .map(|(key, _)| key),
            Some(launch)
        );
        assert_eq!(
            fragment
                .query_selector(".panel .primary.action")
                .expect("descendant multi-class selector")
                .map(|(key, _)| key),
            Some(launch)
        );
        assert_eq!(
            fragment
                .query_selector(".panel .label")
                .expect("descendant should match non-immediate ancestor")
                .map(|(key, _)| key),
            Some(label)
        );
        assert_eq!(
            fragment
                .query_selector_from(branch, "#launch")
                .expect("from-scoped id selector should work")
                .map(|(key, _)| key),
            Some(launch)
        );
    }

    #[test]
    fn fragment_query_selector_all_returns_document_order_matches() {
        let mut fragment = RootDomFragment::new();
        build_query_test_fragment(&mut fragment);

        let matches = fragment
            .query_selector_all(".primary")
            .expect("class query should work");

        assert_eq!(
            query_match_ids(matches),
            vec!["launch", "sibling", "outside"]
        );
        assert_eq!(
            query_match_ids(
                fragment
                    .query_selector_all(".primary.action")
                    .expect("multi-class query should work")
            ),
            vec!["launch"]
        );
    }

    #[test]
    fn fragment_query_selector_from_searches_descendants_only() {
        let mut fragment = RootDomFragment::new();
        let (_, branch, launch, _, _) = build_query_test_fragment(&mut fragment);

        assert_eq!(
            fragment
                .query_selector_from(branch, ".primary")
                .expect("branch search should work")
                .map(|(key, _)| key),
            Some(launch)
        );
        assert_eq!(
            fragment
                .query_selector_from(launch, "button")
                .expect("self should be excluded"),
            None
        );
        assert_eq!(
            query_match_ids(
                fragment
                    .query_selector_all_from(branch, ".primary")
                    .expect("branch all should work")
            ),
            vec!["launch"]
        );
        assert_eq!(
            query_match_ids(
                fragment
                    .query_selector_all_from(branch, ".primary.action")
                    .expect("branch all multi-class should work")
            ),
            vec!["launch"]
        );
        assert_eq!(
            query_match_ids(
                fragment
                    .query_selector_all_from(branch, "#launch")
                    .expect("branch all id should use scoped id lookup")
            ),
            vec!["launch"]
        );
    }

    #[test]
    fn fragment_query_selector_rejects_invalid_syntax_and_keys() {
        let mut fragment = RootDomFragment::new();
        build_query_test_fragment(&mut fragment);
        let removed_key = fragment.create_element(DomTag::Div).expect("removed key");
        fragment.remove_node(removed_key).expect("remove key");

        let selector_error = fragment
            .query_selector("button > .primary")
            .expect_err("unsupported selector should fail");
        let multi_id_error = fragment
            .query_selector("#launch.primary")
            .expect_err("id with classes should stay unsupported");
        let key_error = fragment
            .query_selector_from(removed_key, ".primary")
            .expect_err("outside key should fail");

        assert_eq!(selector_error, DomError::InvalidSelector);
        assert_eq!(multi_id_error, DomError::InvalidSelector);
        assert_eq!(key_error, DomError::InvalidNodeKey);
    }

    #[test]
    fn fragment_query_selector_id_matches_get_element_by_id() {
        let mut fragment = RootDomFragment::new();
        build_query_test_fragment(&mut fragment);

        let query = fragment
            .query_selector("#launch")
            .expect("id query should work")
            .map(|(key, _)| key);
        let indexed = fragment.get_element_by_id("launch").map(|(key, _)| key);

        assert_eq!(query, indexed);
    }

    #[test]
    fn fragment_query_selector_skips_text_nodes_and_does_not_match_fragment_root() {
        let mut fragment = RootDomFragment::new();
        let text = fragment.create_text_node("only text").expect("text");
        fragment
            .append_child(fragment.document_fragment_key(), text)
            .expect("append text");
        fragment
            .set_attr_str(fragment.document_fragment_key(), "class", "fragment-root")
            .expect("fragment class");

        assert_eq!(
            fragment.query_selector("documentfragment").expect("query"),
            None
        );
        assert_eq!(
            fragment.query_selector(".fragment-root").expect("query"),
            None
        );
        assert_eq!(fragment.query_selector("#missing").expect("query"), None);
    }

    #[test]
    fn fragment_set_attr_str_accepts_known_and_custom_attrs() {
        let mut fragment = RootDomFragment::new();
        let element = fragment.create_element(DomTag::Div).expect("element");

        fragment
            .set_attr_str(element, "id", "hero")
            .expect("id should parse");
        fragment
            .set_attr_str(element, "font-size", "14px")
            .expect("font-size should parse");
        fragment
            .set_attr_str(element, "slot", "panel")
            .expect("custom attr should parse");

        let element_node = fragment
            .node(element)
            .and_then(crate::types::DomNode::as_element)
            .expect("element node");
        assert_eq!(element_node.attrs.id.as_deref(), Some("hero"));
        assert_eq!(
            element_node.attrs.attributes.get(&DomAttribute::FontSize),
            Some(&DomAttributeValue::String("14px".to_owned()))
        );
        assert_eq!(
            element_node
                .attrs
                .attributes
                .get(&DomAttribute::Custom("slot".to_owned())),
            Some(&DomAttributeValue::String("panel".to_owned()))
        );
    }

    #[test]
    fn fragment_set_attr_str_accepts_em_font_size() {
        let mut fragment = RootDomFragment::new();
        let element = fragment.create_element(DomTag::Div).expect("element");

        fragment
            .set_attr_str(element, "font-size", "1em")
            .expect("em font-size should succeed in step 3");
        let element_node = fragment
            .node(element)
            .and_then(crate::types::DomNode::as_element)
            .expect("element node");
        assert_eq!(
            element_node.attrs.attributes.get(&DomAttribute::FontSize),
            Some(&DomAttributeValue::String("1em".to_owned()))
        );
    }

    #[test]
    fn fragment_set_attr_str_accepts_em_inside_style_attr_in_step_4() {
        let mut fragment = RootDomFragment::new();
        let element = fragment.create_element(DomTag::Div).expect("element");

        fragment
            .set_attr_str(element, "style", "font-size: 1.5em; width: 2em")
            .expect("em style should parse in step 4");
        let element_node = fragment
            .node(element)
            .and_then(crate::types::DomNode::as_element)
            .expect("element node");
        assert_eq!(
            element_node
                .attribute_style_map()
                .get(&DomAttribute::FontSize),
            Some(&DomAttributeValue::String("1.5em".to_string()))
        );
        assert_eq!(
            element_node.attribute_style_map().get(&DomAttribute::Width),
            Some(&DomAttributeValue::String("2em".to_string()))
        );
    }

    #[test]
    fn fragment_set_attr_str_uses_unset_to_remove_attr() {
        let mut fragment = RootDomFragment::new();
        let element = fragment.create_element(DomTag::Div).expect("element");

        fragment
            .set_attr_str(element, "slot", "panel")
            .expect("custom attr should set");
        fragment
            .set_attr_str(element, "slot", "unset")
            .expect("unset should remove custom attr");

        let element_node = fragment
            .node(element)
            .and_then(crate::types::DomNode::as_element)
            .expect("element node");
        assert_eq!(
            element_node
                .attrs
                .attributes
                .get(&DomAttribute::Custom("slot".to_owned())),
            None
        );
    }

    #[test]
    fn fragment_set_attr_str_updates_style_raw_attr_and_parsed_map() {
        let mut fragment = RootDomFragment::new();
        let element = fragment.create_element(DomTag::Div).expect("element");

        fragment
            .set_attr_str(
                element,
                "style",
                "width: 12px; background-color: #112233; width: 14px",
            )
            .expect("style should parse");

        let element_node = fragment
            .node(element)
            .and_then(crate::types::DomNode::as_element)
            .expect("element node");
        assert_eq!(
            element_node.attrs.attributes.get(&DomAttribute::Style),
            Some(&DomAttributeValue::String(
                "width: 12px; background-color: #112233; width: 14px".to_string()
            ))
        );
        assert_eq!(
            element_node.attribute_style_map().get(&DomAttribute::Width),
            Some(&DomAttributeValue::String("14px".to_string()))
        );
        assert_eq!(
            element_node
                .attribute_style_map()
                .get(&DomAttribute::Custom("background-color".to_owned())),
            Some(&color("#112233"))
        );
    }

    #[test]
    fn fragment_set_attr_str_updates_class_raw_attr_and_parsed_list() {
        let mut fragment = RootDomFragment::new();
        let element = fragment.create_element(DomTag::Div).expect("element");

        fragment
            .set_attr_str(element, "class", "hero primary hero")
            .expect("class should parse");

        let element_node = fragment
            .node(element)
            .and_then(crate::types::DomNode::as_element)
            .expect("element node");
        assert_eq!(
            element_node.attrs.attributes.get(&DomAttribute::Class),
            Some(&DomAttributeValue::String("hero primary hero".to_string()))
        );
        assert_eq!(
            element_node.class_list(),
            &[
                "hero".to_string(),
                "primary".to_string(),
                "hero".to_string()
            ]
        );
    }

    #[test]
    fn fragment_set_attr_str_unset_clears_style_raw_attr_and_parsed_map() {
        let mut fragment = RootDomFragment::new();
        let element = fragment.create_element(DomTag::Div).expect("element");

        fragment
            .set_attr_str(element, "style", "width: 12px; color: #ffffff")
            .expect("style should parse");
        fragment
            .set_attr_str(element, "style", "unset")
            .expect("unset should clear style");

        let element_node = fragment
            .node(element)
            .and_then(crate::types::DomNode::as_element)
            .expect("element node");
        assert_eq!(
            element_node.attrs.attributes.get(&DomAttribute::Style),
            None
        );
        assert!(element_node.attribute_style_map().is_empty());
    }

    #[test]
    fn fragment_set_attr_str_unset_clears_class_raw_attr_and_parsed_list() {
        let mut fragment = RootDomFragment::new();
        let element = fragment.create_element(DomTag::Div).expect("element");

        fragment
            .set_attr_str(element, "class", "hero primary")
            .expect("class should parse");
        fragment
            .set_attr_str(element, "class", "unset")
            .expect("unset should clear class");

        let element_node = fragment
            .node(element)
            .and_then(crate::types::DomNode::as_element)
            .expect("element node");
        assert_eq!(
            element_node.attrs.attributes.get(&DomAttribute::Class),
            None
        );
        assert!(element_node.class_list().is_empty());
    }

    #[test]
    fn fragment_set_attr_str_invalid_style_keeps_previous_raw_attr_and_parsed_map() {
        let mut fragment = RootDomFragment::new();
        let element = fragment.create_element(DomTag::Div).expect("element");

        fragment
            .set_attr_str(element, "style", "width: 12px; color: #ffffff")
            .expect("style should parse");

        let error = fragment
            .set_attr_str(element, "style", "slot: panel")
            .expect_err("invalid style should fail");

        assert_eq!(error, DomError::InvalidAttributeValue);
        let element_node = fragment
            .node(element)
            .and_then(crate::types::DomNode::as_element)
            .expect("element node");
        assert_eq!(
            element_node.attrs.attributes.get(&DomAttribute::Style),
            Some(&DomAttributeValue::String(
                "width: 12px; color: #ffffff".to_string()
            ))
        );
        assert_eq!(
            element_node.attribute_style_map().get(&DomAttribute::Width),
            Some(&DomAttributeValue::String("12px".to_string()))
        );
        assert_eq!(
            element_node
                .attribute_style_map()
                .get(&DomAttribute::Custom("color".to_owned())),
            Some(&color("#ffffff"))
        );
    }

    #[test]
    fn remove_node_removes_full_subtree_and_retains_unreachable_nodes() {
        let mut fragment = RootDomFragment::new();
        let fragment_root = fragment.document_fragment_key();
        let branch = fragment.create_element(DomTag::Div).expect("branch");
        let leaf = fragment.create_text_node("leaf").expect("leaf");
        let detached = fragment.create_element(DomTag::Span).expect("detached");

        fragment
            .append_child(fragment_root, branch)
            .expect("attach branch");
        fragment.append_child(branch, leaf).expect("attach leaf");

        fragment.remove_node(branch).expect("remove subtree");

        assert!(fragment.node(branch).is_none());
        assert!(fragment.node(leaf).is_none());
        assert!(fragment.node(detached).is_some());
    }

    #[test]
    fn append_fragment_imports_children_without_importing_fragment_root() {
        let mut destination = RootDomFragment::new();
        let destination_root = destination.document_fragment_key();
        let target = destination.create_element(DomTag::Div).expect("target");
        destination
            .append_child(destination_root, target)
            .expect("target attach");

        let mut source = RootDomFragment::new();
        let source_root = source.document_fragment_key();
        let first = source.create_element(DomTag::Span).expect("first");
        let second = source.create_element(DomTag::Button).expect("second");
        source
            .append_child(source_root, first)
            .expect("first attach");
        source
            .append_child(source_root, second)
            .expect("second attach");

        destination
            .append_fragment(target, &source)
            .expect("fragment append should succeed");

        let target_node = destination.node(target).expect("target node should exist");
        let (first_key, first_node) = target_node
            .first_child(|key| destination.node(key))
            .expect("imported first child should exist");
        assert!(matches!(
            first_node.as_element().map(|node| &node.tag),
            Some(DomTag::Span)
        ));
        let (second_key, second_node) = first_node
            .next_sibling(|key| destination.node(key))
            .expect("imported second child should exist");
        assert!(matches!(
            second_node.as_element().map(|node| &node.tag),
            Some(DomTag::Button)
        ));
        assert_eq!(
            second_node
                .next_sibling(|key| destination.node(key))
                .map(|(key, _)| key),
            None
        );
        assert_ne!(first_key, target);
        assert_ne!(second_key, target);
    }

    #[test]
    fn append_fragment_can_target_fragment_root_directly() {
        let mut destination = RootDomFragment::new();
        let destination_root = destination.document_fragment_key();

        let mut source = RootDomFragment::new();
        let source_root = source.document_fragment_key();
        let first = source.create_element(DomTag::Div).expect("first");
        let second = source.create_text_node("label").expect("second");
        source
            .append_child(source_root, first)
            .expect("first attach");
        source
            .append_child(source_root, second)
            .expect("second attach");

        destination
            .append_fragment(destination_root, &source)
            .expect("fragment should append to fragment root");

        let destination_root_node = destination
            .node(destination_root)
            .expect("destination root should exist");
        let (first_key, _) = destination_root_node
            .first_child(|key| destination.node(key))
            .expect("first imported child should exist");
        let (second_key, _) = destination
            .node(first_key)
            .and_then(|node| node.next_sibling(|key| destination.node(key)))
            .expect("second imported child should exist");
        assert!(
            matches!(destination.node(first_key), Some(DomNode::Element(_))),
            "first imported child should be an element"
        );
        assert!(
            matches!(destination.node(second_key), Some(DomNode::Text(_))),
            "second imported child should be a text node"
        );
    }

    #[test]
    fn append_fragment_refreshes_imported_text_intrinsic() {
        let mut destination = RootDomFragment::new();
        let destination_root = destination.document_fragment_key();
        let target = destination.create_element(DomTag::Div).expect("target");
        destination
            .append_child(destination_root, target)
            .expect("target attach");

        let mut source = RootDomFragment::new();
        let source_root = source.document_fragment_key();
        let text = source
            .create_text_node("imported fragment text")
            .expect("text");
        source.append_child(source_root, text).expect("text attach");

        destination
            .append_fragment(target, &source)
            .expect("fragment append should succeed");

        let imported_text = destination
            .node(target)
            .and_then(|node| node.first_child(|key| destination.node(key)))
            .map(|(_, node)| node)
            .expect("imported text node should exist");
        match imported_text.intrinsic() {
            DomIntrinsic::Text(metrics) => {
                assert!(metrics.max_content_width_px > 0.0);
                assert!(metrics.line_height_px > 0.0);
            }
            other => panic!("expected refreshed text intrinsic, got {other:?}"),
        }
    }

    #[test]
    fn append_fragment_ignores_unreachable_fragment_nodes() {
        let mut destination = RootDomFragment::new();
        let destination_root = destination.document_fragment_key();
        let target = destination.create_element(DomTag::Div).expect("target");
        destination
            .append_child(destination_root, target)
            .expect("target attach");

        let mut source = RootDomFragment::new();
        let source_root = source.document_fragment_key();
        let imported = source.create_element(DomTag::Span).expect("imported");
        let unreachable = source.create_element(DomTag::Button).expect("unreachable");
        source
            .set_attr(
                imported,
                DomAttribute::Id,
                Some(DomAttributeValue::String("reachable".to_string())),
            )
            .expect("reachable id");
        source
            .set_attr(
                unreachable,
                DomAttribute::Id,
                Some(DomAttributeValue::String("unreachable".to_string())),
            )
            .expect("unreachable id");
        source
            .append_child(source_root, imported)
            .expect("attach reachable");

        destination
            .append_fragment(target, &source)
            .expect("fragment append should succeed");

        assert!(destination.get_element_by_id("reachable").is_some());
        assert!(destination.get_element_by_id("unreachable").is_none());
    }

    #[test]
    fn append_fragment_rejects_duplicate_ids_without_partial_import() {
        let mut destination = RootDomFragment::new();
        let destination_root = destination.document_fragment_key();
        let target = destination.create_element(DomTag::Div).expect("target");
        let existing = destination.create_element(DomTag::Span).expect("existing");
        destination
            .append_child(destination_root, target)
            .expect("target attach");
        destination
            .append_child(target, existing)
            .expect("existing attach");
        destination
            .set_attr(
                existing,
                DomAttribute::Id,
                Some(DomAttributeValue::String("duplicate".to_string())),
            )
            .expect("existing id");

        let mut source = RootDomFragment::new();
        let source_root = source.document_fragment_key();
        let imported = source.create_element(DomTag::Button).expect("imported");
        source
            .set_attr(
                imported,
                DomAttribute::Id,
                Some(DomAttributeValue::String("duplicate".to_string())),
            )
            .expect("fragment id");
        source
            .append_child(source_root, imported)
            .expect("attach fragment child");

        let error = destination
            .append_fragment(target, &source)
            .expect_err("duplicate id should fail");

        assert_eq!(error, DomError::DuplicateElementId);
        let target_node = destination.node(target).expect("target node should exist");
        let imported_child = target_node
            .first_child(|key| destination.node(key))
            .map(|(key, _)| key);
        assert_eq!(imported_child, Some(existing));
        assert_eq!(
            destination
                .node(existing)
                .and_then(|node| node.next_sibling(|key| destination.node(key)))
                .map(|(key, _)| key),
            None
        );
    }

    #[test]
    fn append_fragment_rejects_non_parent_destination_nodes() {
        let mut destination = RootDomFragment::new();
        let fragment_root = destination.document_fragment_key();
        let parent = destination.create_element(DomTag::Div).expect("parent");
        let child = destination.create_text_node("child").expect("child");
        destination
            .append_child(fragment_root, parent)
            .expect("parent attach");
        destination
            .append_child(parent, child)
            .expect("child attach");

        let mut source = RootDomFragment::new();
        let source_root = source.document_fragment_key();
        let imported = source.create_element(DomTag::Button).expect("imported");
        source
            .append_child(source_root, imported)
            .expect("imported attach");

        let error = destination
            .append_fragment(child, &source)
            .expect_err("non-parent destination should fail");

        assert_eq!(error, DomError::InvalidParentTarget);
    }

    #[test]
    fn append_fragment_matches_root_dom_structure_for_same_source_tree() {
        let mut root_dom = RootDom::new(RootDomContext::new_cpu());
        let root_target = root_dom.create_element(DomTag::Div).expect("root target");
        root_dom
            .append_child(root_dom.document_node().0, root_target)
            .expect("root target attach");

        let mut root_source = RootDomFragment::new();
        let root_source_key = root_source.document_fragment_key();
        let root_parent = root_source
            .create_element(DomTag::Div)
            .expect("root parent");
        let root_child = root_source.create_text_node("hello").expect("root child");
        root_source
            .append_child(root_source_key, root_parent)
            .expect("root parent attach");
        root_source
            .append_child(root_parent, root_child)
            .expect("root child attach");

        root_dom
            .append_fragment(root_target, root_source)
            .expect("root dom append");

        let mut destination = RootDomFragment::new();
        let fragment_root = destination.document_fragment_key();
        let fragment_target = destination
            .create_element(DomTag::Div)
            .expect("fragment target");
        destination
            .append_child(fragment_root, fragment_target)
            .expect("fragment target attach");

        let mut fragment_source = RootDomFragment::new();
        let fragment_source_root = fragment_source.document_fragment_key();
        let fragment_parent = fragment_source
            .create_element(DomTag::Div)
            .expect("fragment parent");
        let fragment_child = fragment_source
            .create_text_node("hello")
            .expect("fragment child");
        fragment_source
            .append_child(fragment_source_root, fragment_parent)
            .expect("fragment parent attach");
        fragment_source
            .append_child(fragment_parent, fragment_child)
            .expect("fragment child attach");

        destination
            .append_fragment(fragment_target, &fragment_source)
            .expect("fragment append");

        let root_parent_key = root_dom
            .node(root_target)
            .and_then(|node| node.first_child(|key| root_dom.node(key)))
            .map(|(key, _)| key)
            .expect("root dom imported parent");
        let fragment_parent_key = destination
            .node(fragment_target)
            .and_then(|node| node.first_child(|key| destination.node(key)))
            .map(|(key, _)| key)
            .expect("fragment imported parent");

        assert!(matches!(
            root_dom.node(root_parent_key).and_then(DomNode::as_element),
            Some(node) if node.tag == DomTag::Div
        ));
        assert!(matches!(
            destination.node(fragment_parent_key).and_then(DomNode::as_element),
            Some(node) if node.tag == DomTag::Div
        ));
        assert!(matches!(
            root_dom
                .node(root_parent_key)
                .and_then(|node| node.first_child(|key| root_dom.node(key)))
                .map(|(_, node)| node),
            Some(DomNode::Text(_))
        ));
        assert!(matches!(
            destination
                .node(fragment_parent_key)
                .and_then(|node| node.first_child(|key| destination.node(key)))
                .map(|(_, node)| node),
            Some(DomNode::Text(_))
        ));
    }
}
