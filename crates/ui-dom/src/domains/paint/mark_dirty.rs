use crate::domains::layout::{layer_kind_for_node, node_box_from_ref, relative_offset_for_node};
use crate::types::{DomLayerKind, DomNodeSlotKey, DomPaintState, DomRect, RootDom};

#[cfg(test)]
use crate::domains::visitor::style_recompute::recompute_dirty_style_subtree_for_layer_root;
#[cfg(test)]
use crate::types::{DomNode, DomTag, RootDomContext};

pub(crate) fn mark_node_dirty_up_to_layout_root(root_dom: &mut RootDom, key: DomNodeSlotKey) {
    let Some(damaged_rect) = node_damage_rect(root_dom, key) else {
        mark_full_repaint_up_to_layout_root(root_dom, key);
        return;
    };
    mark_rect_dirty_up_to_layout_root(root_dom, key, damaged_rect);
}

pub(crate) fn mark_rect_dirty_up_to_layout_root(
    root_dom: &mut RootDom,
    key: DomNodeSlotKey,
    damaged_rect: DomRect,
) {
    let mut current_key = Some(key);
    while let Some(current) = current_key {
        let is_layout_root = is_layout_root_key(root_dom, current);
        let Some(node) = root_dom.nodes.get_mut(current) else {
            return;
        };
        let was_full_repaint =
            matches!(node.paint_repaint_state(), DomPaintState::FullLayerRepaint);
        node.mark_damaged_rect_repaint(damaged_rect);
        if was_full_repaint || is_layout_root {
            return;
        }
        current_key = node.parent_key();
    }
}

pub(crate) fn mark_full_repaint_up_to_layout_root(root_dom: &mut RootDom, key: DomNodeSlotKey) {
    let mut current_key = Some(key);
    while let Some(current) = current_key {
        let is_layout_root = is_layout_root_key(root_dom, current);
        let Some(node) = root_dom.nodes.get_mut(current) else {
            return;
        };
        let was_full_repaint =
            matches!(node.paint_repaint_state(), DomPaintState::FullLayerRepaint);
        node.mark_full_layer_repaint();
        if was_full_repaint || is_layout_root {
            return;
        }
        current_key = node.parent_key();
    }
}

fn node_damage_rect(root_dom: &RootDom, key: DomNodeSlotKey) -> Option<DomRect> {
    let node = root_dom.node(key)?;
    let border_box = node_box_from_ref(node).border_box;
    let offset = if is_layout_root_key(root_dom, key) {
        crate::types::primitives::PointPx::default()
    } else {
        relative_offset_for_node(root_dom, key)
    };
    Some(DomRect {
        left_px: offset.x_px,
        top_px: offset.y_px,
        width_px: border_box.width_px.max(0.0),
        height_px: border_box.height_px.max(0.0),
    })
}

fn is_layout_root_key(root_dom: &RootDom, key: DomNodeSlotKey) -> bool {
    if key == root_dom.root {
        return true;
    }
    root_dom
        .node(key)
        .is_some_and(|node| layer_kind_for_node(node) != DomLayerKind::None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn append_element(
        root: &mut RootDom,
        parent_key: DomNodeSlotKey,
        tag: DomTag,
    ) -> DomNodeSlotKey {
        let key = root.create_element(tag).expect("element");
        root.append_child(parent_key, key).expect("append child");
        key
    }

    fn set_position_fixed(root: &mut RootDom, key: DomNodeSlotKey) {
        root.style_mut(key)
            .set("position", "fixed")
            .expect("position");
        recompute_dirty_style_subtree_for_layer_root(root, root.root).expect("style pass");
    }

    fn set_box(
        root: &mut RootDom,
        key: DomNodeSlotKey,
        left_px: f32,
        top_px: f32,
        width_px: f32,
        height_px: f32,
    ) {
        let node = root.nodes.get_mut(key).expect("node should exist");
        match node {
            DomNode::Document(document) => {
                document.base.box_model.border_box.left_px = left_px;
                document.base.box_model.border_box.top_px = top_px;
                document.base.box_model.border_box.width_px = width_px;
                document.base.box_model.border_box.height_px = height_px;
            }
            DomNode::DocumentFragment(fragment) => {
                fragment.base.box_model.border_box.left_px = left_px;
                fragment.base.box_model.border_box.top_px = top_px;
                fragment.base.box_model.border_box.width_px = width_px;
                fragment.base.box_model.border_box.height_px = height_px;
            }
            DomNode::Element(element) => {
                element.base.box_model.border_box.left_px = left_px;
                element.base.box_model.border_box.top_px = top_px;
                element.base.box_model.border_box.width_px = width_px;
                element.base.box_model.border_box.height_px = height_px;
            }
            DomNode::Text(text) => {
                text.base.box_model.border_box.left_px = left_px;
                text.base.box_model.border_box.top_px = top_px;
                text.base.box_model.border_box.width_px = width_px;
                text.base.box_model.border_box.height_px = height_px;
            }
        }
    }

    fn clear_paint_state(root: &mut RootDom, key: DomNodeSlotKey) {
        let node = root.nodes.get_mut(key).expect("node should exist");
        node.clear_paint_repaint_state();
    }

    fn assert_damaged_rect_state(root: &RootDom, key: DomNodeSlotKey, expected_rects: &[DomRect]) {
        let node = root.node(key).expect("node should exist");
        match node.paint_repaint_state() {
            DomPaintState::DamagedRectsRepaint(rects) => {
                let actual = rects.iter().copied().collect::<Vec<_>>();
                assert_eq!(actual.as_slice(), expected_rects);
            }
            other => panic!("expected damaged rect repaint, got {other:?}"),
        }
    }

    fn assert_full_repaint(root: &RootDom, key: DomNodeSlotKey) {
        let node = root.node(key).expect("node should exist");
        assert!(matches!(
            node.paint_repaint_state(),
            DomPaintState::FullLayerRepaint
        ));
    }

    fn assert_clean(root: &RootDom, key: DomNodeSlotKey) {
        let node = root.node(key).expect("node should exist");
        assert!(matches!(node.paint_repaint_state(), DomPaintState::None));
    }

    #[test]
    fn marks_rect_dirty_up_to_document_root_when_no_explicit_layout_root_exists() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let parent = append_element(&mut root, document_key, DomTag::Div);
        let text = root.create_text_node("label").expect("text");
        root.append_child(parent, text).expect("append text");

        set_box(&mut root, document_key, 0.0, 0.0, 100.0, 80.0);
        set_box(&mut root, parent, 0.0, 0.0, 100.0, 80.0);
        set_box(&mut root, text, 7.0, 9.0, 20.0, 10.0);
        clear_paint_state(&mut root, document_key);
        clear_paint_state(&mut root, parent);
        clear_paint_state(&mut root, text);

        mark_node_dirty_up_to_layout_root(&mut root, text);

        let expected = [DomRect {
            left_px: 7.0,
            top_px: 9.0,
            width_px: 20.0,
            height_px: 10.0,
        }];
        assert_damaged_rect_state(&root, text, &expected);
        assert_damaged_rect_state(&root, parent, &expected);
        assert_damaged_rect_state(&root, document_key, &expected);
    }

    #[test]
    fn marks_rect_dirty_only_up_to_nearest_explicit_layout_root() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let outer = append_element(&mut root, document_key, DomTag::Div);
        let layout_root = append_element(&mut root, outer, DomTag::Div);
        let child = append_element(&mut root, layout_root, DomTag::Div);
        let text = root.create_text_node("label").expect("text");
        root.append_child(child, text).expect("append text");

        set_position_fixed(&mut root, layout_root);
        set_box(&mut root, document_key, 0.0, 0.0, 160.0, 120.0);
        set_box(&mut root, outer, 0.0, 0.0, 160.0, 120.0);
        set_box(&mut root, layout_root, 0.0, 0.0, 80.0, 60.0);
        set_box(&mut root, child, 0.0, 0.0, 80.0, 60.0);
        set_box(&mut root, text, 3.0, 4.0, 12.0, 8.0);
        for key in [document_key, outer, layout_root, child, text] {
            clear_paint_state(&mut root, key);
        }

        mark_node_dirty_up_to_layout_root(&mut root, text);

        let expected = [DomRect {
            left_px: 3.0,
            top_px: 4.0,
            width_px: 12.0,
            height_px: 8.0,
        }];
        assert_clean(&root, document_key);
        assert_clean(&root, outer);
        assert_damaged_rect_state(&root, layout_root, &expected);
        assert_damaged_rect_state(&root, child, &expected);
        assert_damaged_rect_state(&root, text, &expected);
    }

    #[test]
    fn promotes_to_full_repaint_when_rect_limit_is_exceeded() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let parent = append_element(&mut root, document_key, DomTag::Div);
        let text = root.create_text_node("label").expect("text");
        root.append_child(parent, text).expect("append text");
        for key in [document_key, parent, text] {
            clear_paint_state(&mut root, key);
        }

        for index in 0..9 {
            let index = f32::from(u16::try_from(index).expect("test index should fit u16"));
            mark_rect_dirty_up_to_layout_root(
                &mut root,
                text,
                DomRect {
                    left_px: index * 20.0,
                    top_px: 0.0,
                    width_px: 5.0,
                    height_px: 5.0,
                },
            );
        }

        assert_full_repaint(&root, text);
        assert_full_repaint(&root, parent);
        assert_full_repaint(&root, document_key);
    }

    #[test]
    fn stops_marking_rect_dirty_when_an_ancestor_is_already_full_repaint() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let parent = append_element(&mut root, document_key, DomTag::Div);
        let child = append_element(&mut root, parent, DomTag::Div);
        let text = root.create_text_node("label").expect("text");
        root.append_child(child, text).expect("append text");
        for key in [document_key, parent, child, text] {
            clear_paint_state(&mut root, key);
        }
        root.nodes
            .get_mut(parent)
            .expect("parent should exist")
            .mark_full_layer_repaint();

        mark_rect_dirty_up_to_layout_root(
            &mut root,
            text,
            DomRect {
                left_px: 12.0,
                top_px: 6.0,
                width_px: 8.0,
                height_px: 4.0,
            },
        );

        assert_clean(&root, document_key);
        assert_full_repaint(&root, parent);
        assert_damaged_rect_state(
            &root,
            child,
            &[DomRect {
                left_px: 12.0,
                top_px: 6.0,
                width_px: 8.0,
                height_px: 4.0,
            }],
        );
        assert_damaged_rect_state(
            &root,
            text,
            &[DomRect {
                left_px: 12.0,
                top_px: 6.0,
                width_px: 8.0,
                height_px: 4.0,
            }],
        );
    }

    #[test]
    fn marks_full_repaint_up_to_document_root() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let parent = append_element(&mut root, document_key, DomTag::Div);
        let child = append_element(&mut root, parent, DomTag::Div);
        for key in [document_key, parent, child] {
            clear_paint_state(&mut root, key);
        }

        mark_full_repaint_up_to_layout_root(&mut root, child);

        assert_full_repaint(&root, child);
        assert_full_repaint(&root, parent);
        assert_full_repaint(&root, document_key);
    }

    #[test]
    fn marks_full_repaint_only_up_to_nearest_explicit_layout_root() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let outer = append_element(&mut root, document_key, DomTag::Div);
        let layout_root = append_element(&mut root, outer, DomTag::Div);
        let child = append_element(&mut root, layout_root, DomTag::Div);
        set_position_fixed(&mut root, layout_root);
        for key in [document_key, outer, layout_root, child] {
            clear_paint_state(&mut root, key);
        }

        mark_full_repaint_up_to_layout_root(&mut root, child);

        assert_clean(&root, document_key);
        assert_clean(&root, outer);
        assert_full_repaint(&root, layout_root);
        assert_full_repaint(&root, child);
    }
}
