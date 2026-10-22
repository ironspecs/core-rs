//! Programmatic declaration write API for the `ui-dom` style system.
//!
//! This file owns `StyleMut`, the css-style-attr §4 declaration-store handle
//! (PLAN.md proposals A+D5): consumers write DECLARATIONS, not computed
//! values. The two contracts are deliberately different — `set` is the strict
//! single-declaration path (fail fast, typed `DomError`, no drops), while
//! `set_all` is the declaration-list path (drop-and-continue, CSS 2.1 §4.1.7,
//! with an explicit `DeclDropReport`) — per the AGENTS.md scoped exception for
//! boundary data. Writes mark style-dirty only: the style pass classifies the
//! layout/paint consequences (spine step 3); no write ever marks layout or
//! paint directly.
//!
//! Invariants and rules:
//!
//! - every write validates its target first: a missing node is
//!   `DomError::InvalidNodeKey`, a non-element target (`Text`, `Document`,
//!   `DocumentFragment`) is `DomError::InvalidAttributeTarget`; only
//!   `Element` is a valid target — the fold processes elements only, so
//!   writes on any other node kind would be silently inert
//! - `set`/`set_all`/`remove` are all-or-nothing per call: a failed parse or
//!   an unknown property name mutates nothing
//! - the two contracts differ by shape, never by accident: a single
//!   declaration has no "rest of the list" to continue to, so it fails fast;
//!   a list follows CSS error handling and reports
//! - dirty marking and the `StyleDeclarationUpsert` metric happen exactly once
//!   per mutating call, only when the block actually changed

use super::decl_drop_report::DeclDropReport;
use super::decl_parse::{longhands_for_name, parse_declaration_block, parse_declaration_strict};
use crate::types::dom_node::{DomNode, DomNodeSlotKey};
use crate::types::{DomError, RootDom};

#[cfg(test)]
use crate::domains::style::{DeclLength, DeclValue, PresentationProperty};
#[cfg(test)]
use crate::types::{DomTag, RootDomContext, RootDomMetricName};

/// The programmatic declaration write handle for one node's inline style
/// (css-style-attr §4 declaration-store semantics). Borrows the root so the
/// write and its invalidation are one atomic step; the handle is one-shot.
pub struct StyleMut<'a> {
    root_dom: &'a mut RootDom,
    node: DomNodeSlotKey,
}

impl<'a> StyleMut<'a> {
    pub(crate) fn new(root_dom: &'a mut RootDom, node: DomNodeSlotKey) -> Self {
        Self { root_dom, node }
    }

    /// STRICT single-declaration contract: parses and upserts one authored
    /// declaration (shorthands expand to their longhands). Malformed or
    /// unadmitted input is a typed `DomError` — fail fast, no drops.
    /// Never marks layout or paint directly: the pass classifies.
    pub fn set(self, property: &str, value: &str) -> Result<(), DomError> {
        self.validate_target()?;
        let declarations =
            parse_declaration_strict(property, value, self.root_dom.inline_decl_store_mut())?;
        self.root_dom
            .upsert_inline_declarations(self.node, &declarations);
        Ok(())
    }

    /// DECLARATION-LIST contract (G semantics): parses a declaration list
    /// drop-and-continue (CSS 2.1 §4.1.7), REPLACES the node's inline block
    /// wholesale (cssText semantics), and returns the explicit drop report.
    pub fn set_all(self, declaration_list: &str) -> Result<DeclDropReport, DomError> {
        self.validate_target()?;
        let (block, report) =
            parse_declaration_block(declaration_list, self.root_dom.inline_decl_store_mut());
        self.root_dom.replace_inline_decl_block(self.node, block);
        Ok(report)
    }

    /// Removes one property (a shorthand name removes all its longhands).
    /// Unknown names are a typed `DomError`; an absent property is `Ok(())`.
    pub fn remove(self, property: &str) -> Result<(), DomError> {
        self.validate_target()?;
        let longhands = longhands_for_name(property).ok_or(DomError::InvalidAttributeValue)?;
        self.root_dom
            .remove_inline_declarations(self.node, &longhands);
        Ok(())
    }

    fn validate_target(&self) -> Result<(), DomError> {
        let node = self
            .root_dom
            .node(self.node)
            .ok_or(DomError::InvalidNodeKey)?;
        // Element-only target. Keep this check in sync with the `style`
        // attribute route in `RootDom::route_style_attribute`
        // (src/types/root_dom.rs): the fold processes elements only, so a
        // write on any other node kind would be silently inert.
        if !matches!(node, DomNode::Element(_)) {
            return Err(DomError::InvalidAttributeTarget);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style_dirty(root: &RootDom, key: DomNodeSlotKey) -> bool {
        root.node(key)
            .expect("node should exist")
            .as_element()
            .expect("node should be an element")
            .style()
            .dirty
    }

    fn block_value(
        root: &RootDom,
        key: DomNodeSlotKey,
        property: PresentationProperty,
    ) -> DeclValue {
        let block = root
            .inline_decl_block(key)
            .unwrap_or_else(|| panic!("inline block should exist"));
        *block
            .get(property)
            .unwrap_or_else(|| panic!("{property:?} should be present"))
    }

    #[test]
    fn set_rejects_missing_text_and_document_targets() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let missing = root.create_element(DomTag::Div).expect("div");
        root.remove_node(missing).expect("remove");

        assert_eq!(
            root.style_mut(missing).set("width", "10px"),
            Err(DomError::InvalidNodeKey)
        );

        let text = root.create_text_node("label").expect("text");
        assert_eq!(
            root.style_mut(text).set("width", "10px"),
            Err(DomError::InvalidAttributeTarget)
        );

        // The document node is a valid DOM node but not an element: the fold
        // processes elements only, so a document write would be inert and is
        // rejected rather than silently dropped.
        let document = root.document_node().0;
        assert_eq!(
            root.style_mut(document).set("width", "10px"),
            Err(DomError::InvalidAttributeTarget)
        );
    }

    #[test]
    fn set_is_strict_and_leaves_no_block_or_metric_on_failure() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let node = root.create_element(DomTag::Div).expect("div");

        for (property, value) in [("width", "lots"), ("frobnicate", "1"), ("", "1px")] {
            assert_eq!(
                root.style_mut(node).set(property, value),
                Err(DomError::InvalidAttributeValue),
                "set({property:?}, {value:?}) should fail strict"
            );
        }

        assert_eq!(root.inline_decl_block(node), None);
        assert_eq!(root.count(RootDomMetricName::StyleDeclarationUpsert), 0);
    }

    #[test]
    fn set_upserts_longhands_marks_dirty_and_counts_once_per_mutation() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let node = root.create_element(DomTag::Div).expect("div");

        root.style_mut(node).set("width", "10px").expect("width");
        assert_eq!(root.count(RootDomMetricName::StyleDeclarationUpsert), 1);
        assert!(style_dirty(&root, node));
        assert_eq!(
            block_value(&root, node, PresentationProperty::Width),
            DeclValue::Length(DeclLength::Px(10.0))
        );

        root.style_mut(node).set("gap", "4px").expect("gap");
        assert_eq!(root.count(RootDomMetricName::StyleDeclarationUpsert), 2);
        assert_eq!(
            block_value(&root, node, PresentationProperty::RowGap),
            DeclValue::Length(DeclLength::Px(4.0))
        );
        assert_eq!(
            block_value(&root, node, PresentationProperty::ColumnGap),
            DeclValue::Length(DeclLength::Px(4.0))
        );
    }

    #[test]
    fn set_same_value_twice_does_not_recount() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let node = root.create_element(DomTag::Div).expect("div");

        root.style_mut(node).set("width", "10px").expect("first");
        root.style_mut(node).set("width", "10px").expect("second");

        assert_eq!(root.count(RootDomMetricName::StyleDeclarationUpsert), 1);
    }

    #[test]
    fn set_all_replaces_wholesale_reports_and_expands_shorthands() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let node = root.create_element(DomTag::Div).expect("div");
        root.style_mut(node).set("width", "10px").expect("width");

        let report = root
            .style_mut(node)
            .set_all("height: 5px; mystery: 1px; gap: 2px")
            .expect("set_all");
        assert_eq!(
            report,
            DeclDropReport {
                applied: 2,
                dropped: 1
            }
        );

        let block = root.inline_decl_block(node).expect("block");
        assert_eq!(block.get(PresentationProperty::Width), None);
        assert_eq!(
            block.get(PresentationProperty::Height),
            Some(&DeclValue::Length(DeclLength::Px(5.0)))
        );
        assert_eq!(
            block.get(PresentationProperty::RowGap),
            Some(&DeclValue::Length(DeclLength::Px(2.0)))
        );
        assert_eq!(
            block.get(PresentationProperty::ColumnGap),
            Some(&DeclValue::Length(DeclLength::Px(2.0)))
        );
    }

    #[test]
    fn set_all_empty_list_frees_the_block() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let node = root.create_element(DomTag::Div).expect("div");
        root.style_mut(node).set("width", "10px").expect("width");

        let report = root.style_mut(node).set_all("").expect("set_all empty");
        assert_eq!(report, DeclDropReport::default());
        assert_eq!(root.inline_decl_block(node), None);
    }

    #[test]
    fn remove_removes_longhands_and_shorthands_and_frees_empty_blocks() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let node = root.create_element(DomTag::Div).expect("div");
        root.style_mut(node).set("width", "10px").expect("width");
        root.style_mut(node)
            .set("border-radius", "5px")
            .expect("radius");
        assert_eq!(root.count(RootDomMetricName::StyleDeclarationUpsert), 2);

        // Removing an absent property is a no-op: Ok, metric unchanged.
        root.style_mut(node)
            .remove("opacity")
            .expect("absent remove");
        assert_eq!(root.count(RootDomMetricName::StyleDeclarationUpsert), 2);

        // Removing a longhand removes just that property.
        root.style_mut(node).remove("width").expect("remove width");
        assert_eq!(root.count(RootDomMetricName::StyleDeclarationUpsert), 3);
        let block = root.inline_decl_block(node).expect("radius still present");
        assert_eq!(block.get(PresentationProperty::Width), None);

        // Removing a shorthand removes all its longhands, freeing the block.
        root.style_mut(node)
            .remove("border-radius")
            .expect("remove radius");
        assert_eq!(root.inline_decl_block(node), None);
        assert_eq!(root.count(RootDomMetricName::StyleDeclarationUpsert), 4);

        // Unknown names are a typed error.
        assert_eq!(
            root.style_mut(node).remove("frobnicate"),
            Err(DomError::InvalidAttributeValue)
        );
    }

    #[test]
    fn set_interns_family_list_resolvable_through_the_inline_store() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let node = root.create_element(DomTag::Div).expect("div");

        root.style_mut(node)
            .set("font-family", "ui-sans, sans-serif")
            .expect("font-family");

        let block = root.inline_decl_block(node).expect("block");
        let value = *block
            .get(PresentationProperty::FontFamily)
            .expect("font-family");
        let DeclValue::FamilyList(key) = value else {
            panic!("font-family should intern to a family list");
        };
        assert_eq!(
            root.resolve_inline_family_list(key)
                .map(crate::domains::style::computed::TextFamilyList::to_css_string),
            Some("ui-sans, sans-serif".to_string())
        );
    }

    #[test]
    fn removing_a_subtree_frees_its_inline_blocks() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let parent = root.create_element(DomTag::Div).expect("parent");
        let child = root.create_element(DomTag::Div).expect("child");
        root.append_child(root.document_node().0, parent)
            .expect("append parent");
        root.append_child(parent, child).expect("append child");
        root.style_mut(child)
            .set("width", "10px")
            .expect("child width");
        assert!(root.inline_decl_block(child).is_some());
        assert_eq!(root.inline_decl_live_block_count(), 1);

        root.remove_node(parent).expect("remove parent subtree");

        assert_eq!(root.inline_decl_block(child), None);
        assert_eq!(root.inline_decl_live_block_count(), 0);
    }
}
