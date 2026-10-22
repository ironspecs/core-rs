//! The read-only computed-style view for one node (PLAN.md S1.2, spine 3.3a).
//!
//! This file owns `ComputedStyleRef`: the typed, group-based view returned by
//! `RootDom::computed_style`. It is the public read surface of the redesign's
//! "presentation half": each accessor delegates to the `DomNode::computed_*`
//! accessor (which merges the active-transition overlay) and projects one
//! `Copy` group (or the transition-longhand slot) back out of the node record.
//! The view holds the key and root, never a cached copy.
//!
//! Invariants and rules:
//!
//! - a ref is a borrowed view (`root_dom` + `node` key); it never clones the
//!   groups into a separate store
//! - `RootDom::computed_style` returns `Option<ComputedStyleRef<'_>>`: a
//!   missing key is boundary data and must not fabricate a group, so the ref
//!   only ever holds a key that was validated live at construction
//! - group accessors return the `Copy` group by value — this *is* the view;
//!   they delegate to `DomNode::computed_*`, which merges the node's
//!   active-transition overlay (spine 3.3 fix)
//! - `font_family` serializes the resolved family list (through the document's
//!   inline family-list arena); `font_family_list` exposes the structured list

use crate::domains::style::computed::{
    ComputedBoxGroup, ComputedColorGroup, ComputedFlexGroup, ComputedFontGroup, ComputedTextGroup,
    ComputedTransformGroup, ComputedVisualGroup, NodePresentation, ResolvedTransitionLonghands,
    TextFamilyList,
};
use crate::types::{DomNode, DomNodeSlotKey, RootDom};

#[cfg(test)]
use crate::domains::style::computed::BoxLength;
#[cfg(test)]
use crate::domains::style::computed::DisplayKw;
#[cfg(test)]
use crate::types::{DomTag, RootDomContext};

/// The typed computed-style view for one live node key.
#[derive(Clone, Copy, Debug)]
pub struct ComputedStyleRef<'a> {
    root_dom: &'a RootDom,
    node: DomNodeSlotKey,
}

impl<'a> ComputedStyleRef<'a> {
    pub(crate) fn new(root_dom: &'a RootDom, node: DomNodeSlotKey) -> Self {
        Self { root_dom, node }
    }

    fn node(&self) -> &DomNode {
        self.root_dom
            .node(self.node)
            .expect("computed_style refs only ever hold keys validated at construction")
    }

    fn presentation(&self) -> &NodePresentation {
        match self.node() {
            DomNode::Document(document) => &document.base.presentation,
            DomNode::DocumentFragment(fragment) => &fragment.base.presentation,
            DomNode::Element(element) => &element.base.presentation,
            DomNode::Text(text) => &text.base.presentation,
        }
    }

    #[must_use]
    pub fn as_font(&self) -> ComputedFontGroup {
        self.node().computed_font()
    }

    #[must_use]
    pub fn as_text(&self) -> ComputedTextGroup {
        self.node().computed_text()
    }

    #[must_use]
    pub fn as_color(&self) -> ComputedColorGroup {
        self.node().computed_color()
    }

    #[must_use]
    pub fn as_box(&self) -> ComputedBoxGroup {
        self.node().computed_box()
    }

    #[must_use]
    pub fn as_flex(&self) -> ComputedFlexGroup {
        self.node().computed_flex()
    }

    #[must_use]
    pub fn as_visual(&self) -> ComputedVisualGroup {
        self.node().computed_visual()
    }

    #[must_use]
    pub fn as_transform(&self) -> ComputedTransformGroup {
        self.node().computed_transform()
    }

    /// The resolved `font-family` list, serialized to its canonical CSS string
    /// (quoted names re-quoted, generics bare); `None` when the family is
    /// absent (engine default). The structured list is `font_family_list`.
    #[must_use]
    pub fn font_family(&self) -> Option<String> {
        self.font_family_list().map(TextFamilyList::to_css_string)
    }

    /// The resolved `font-family` list (text seam S2), interned in the
    /// document's inline family-list arena; `None` when the family is absent
    /// (engine default).
    #[must_use]
    pub(crate) fn font_family_list(&self) -> Option<&TextFamilyList> {
        let key = self.node().computed_font().family?;
        self.root_dom.resolve_inline_family_list(key)
    }

    #[must_use]
    pub fn transition_longhands(&self) -> &ResolvedTransitionLonghands {
        &self.presentation().transition_longhands
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // initial-value assertions compare exact literals
mod tests {
    use super::*;

    #[test]
    fn missing_node_key_returns_none() {
        let root = RootDom::new(RootDomContext::new_cpu());
        // A key from a separate keyed slotmap at index 1 cannot collide with
        // this root's document node (index 0).
        let absent_key = {
            let mut keys = slotmap::SlotMap::with_key();
            keys.insert(());
            keys.insert(())
        };
        assert!(root.computed_style(absent_key).is_none());
    }

    #[test]
    fn element_reads_back_default_presentation_until_the_pass_writes_it() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let element = root.create_element(DomTag::Div).expect("element");
        root.append_child(root.root, element).expect("append");

        let view = root.computed_style(element).expect("live element");
        assert_eq!(view.as_box().display, DisplayKw::Block);
        assert_eq!(view.as_font().size_px, 14.0);
        assert!(view.font_family().is_none());
        assert!(view.transition_longhands().is_empty());
    }

    #[test]
    fn element_reads_back_what_was_written_to_the_record() {
        let mut root = RootDom::new(RootDomContext::new_cpu());
        let element = root.create_element(DomTag::Div).expect("element");
        root.append_child(root.root, element).expect("append");
        {
            let node = root.nodes.get_mut(element).expect("live element");
            match node {
                DomNode::Element(node) => {
                    node.base.presentation.computed.box_.width = BoxLength::Px(42.0);
                }
                _ => panic!("created element should stay an element"),
            }
        }

        assert_eq!(
            root.computed_style(element)
                .expect("live element")
                .as_box()
                .width,
            BoxLength::Px(42.0)
        );
    }
}
