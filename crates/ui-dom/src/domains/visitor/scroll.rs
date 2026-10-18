//! Scroll visitor for point-targeted wheel dispatch.
//!
//! Why this visitor exists:
//!
//! - wheel scroll is not DOM event dispatch
//! - wheel scroll must run before pointer event dispatch
//! - scroll ownership belongs to retained scroll state in `RootDom`, not to
//!   hover/active reconciliation and not to DOM event propagation
//!
//! What this visitor does:
//!
//! - reuse the shared clipped-rect layer-hit traversal to collect matching
//!   layer roots in topmost-first order
//! - offer the wheel delta to each matching layer root in that order
//! - stop at the first layer root whose DOM attributes and retained scroll
//!   state accept the delta
//!
//! Constraints this file is protecting:
//!
//! - no hover/active behavior mixed into scroll resolution
//! - no DOM event propagation mixed into scroll resolution
//! - keep layer selection aligned with the shared hit algorithm so fixed and
//!   absolute child layers obey the same clipping rules as hover and layer hit
use bevy::prelude::{Entity, World};
use smallvec::SmallVec;

use crate::domains::layer_hit::visit_attached_layer_hits_from_root_point;
use crate::types::primitives::PointPx;
use crate::types::{DomError, DomNodeSlotKey, RootDom};

#[cfg(test)]
use crate::types::{
    DomNode, DomPointerState, DomPointerUpdate, DomPseudoState, DomTag, RootDomContext,
};
#[cfg(test)]
use crate::{LayerConfig, LayerPool, LayerPoolRenderJobQueue, StyleRuleOrigin, UiDomRenderSupport};

pub(crate) fn apply_vertical_scroll_from_point(
    root_dom: &mut RootDom,
    world: &World,
    _parent: Entity,
    point: PointPx,
    delta_y_px: f32,
) -> Result<bool, DomError> {
    if delta_y_px == 0.0 {
        return Ok(false);
    }
    let mut layer_root_keys = SmallVec::<[DomNodeSlotKey; 8]>::new();
    visit_attached_layer_hits_from_root_point(root_dom, world, point, &mut |layer_hit| {
        layer_root_keys.push(layer_hit.node_key);
        Ok(false)
    })?;
    for layer_root_key in layer_root_keys {
        if root_dom.apply_vertical_scroll_to_layer_root(layer_root_key, delta_y_px)? {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{image::Image, prelude::*};

    fn install_direct_gpu_support(world: &mut World) {
        world.init_resource::<Assets<Image>>();
        world.insert_resource(LayerPool::new(LayerConfig::default()));
        world.init_resource::<LayerPoolRenderJobQueue>();
        world.insert_resource(UiDomRenderSupport::direct_gpu());
    }

    #[test]
    fn wheel_scroll_targets_scrollable_layer_while_descending_and_applies_on_ascent() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let scroll_root = root.create_element(DomTag::Div).expect("scroll root");
        let top_overlay = root.create_element(DomTag::Div).expect("top overlay");
        let first_item = root.create_element(DomTag::Div).expect("first item");
        let second_item = root.create_element(DomTag::Div).expect("second item");

        root.append_child(root.root, scroll_root)
            .expect("append scroll root");
        root.append_child(scroll_root, first_item)
            .expect("append first item");
        root.append_child(scroll_root, second_item)
            .expect("append second item");
        root.append_child(root.root, top_overlay)
            .expect("append top overlay");

        root.style_mut(scroll_root)
            .set("overflow", "scroll")
            .expect("scroll overflow");
        root.style_mut(scroll_root)
            .set("width", "80px")
            .expect("scroll width");
        root.style_mut(scroll_root)
            .set("height", "40px")
            .expect("scroll height");
        root.style_mut(first_item)
            .set("width", "80px")
            .expect("first width");
        root.style_mut(first_item)
            .set("height", "40px")
            .expect("first height");
        root.style_mut(second_item)
            .set("width", "80px")
            .expect("second width");
        root.style_mut(second_item)
            .set("height", "80px")
            .expect("second height");
        root.style_mut(top_overlay)
            .set("position", "absolute")
            .expect("overlay layer");
        root.style_mut(top_overlay)
            .set("left", "0px")
            .expect("overlay left");
        root.style_mut(top_overlay)
            .set("top", "0px")
            .expect("overlay top");
        root.style_mut(top_overlay)
            .set("width", "80px")
            .expect("overlay width");
        root.style_mut(top_overlay)
            .set("height", "80px")
            .expect("overlay height");

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        let parent = world
            .spawn((Sprite {
                custom_size: Some(Vec2::new(160.0, 160.0)),
                ..Default::default()
            },))
            .id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("wheel proof should attach");
        root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("attached root should present once before wheel proof");

        root.update(
            &mut world,
            0.0,
            DomPointerUpdate::Set(DomPointerState {
                x_px: 10.0,
                y_px: 10.0,
                primary_button_down: false,
                scroll_delta_y_px: -60.0,
            }),
        )
        .expect("wheel update should succeed");

        assert!(
            root.node(top_overlay)
                .and_then(DomNode::as_element)
                .is_some_and(|element| element.has_pseudo_state(DomPseudoState::Hover)),
            "wheel update should still resolve hover from the top overlay after scroll ownership falls through",
        );
        assert!(
            (root.vertical_scroll_offset_y_px(scroll_root) - 60.0).abs() <= 0.001,
            "wheel scroll should reach the expected retained offset behind the overlay",
        );
    }

    #[test]
    // CSS Overflow 3 plus Blink/Gecko scroll-container policy:
    // `overflow:auto` owns scroll state once content exceeds the viewport.
    fn wheel_scroll_targets_auto_overflow_layer_when_content_exceeds_viewport() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let scroll_root = root.create_element(DomTag::Div).expect("scroll root");
        let first_item = root.create_element(DomTag::Div).expect("first item");
        let second_item = root.create_element(DomTag::Div).expect("second item");

        root.append_child(root.root, scroll_root)
            .expect("append scroll root");
        root.append_child(scroll_root, first_item)
            .expect("append first item");
        root.append_child(scroll_root, second_item)
            .expect("append second item");

        root.style_mut(scroll_root)
            .set("overflow", "auto")
            .expect("auto overflow");
        root.style_mut(scroll_root)
            .set("width", "80px")
            .expect("scroll width");
        root.style_mut(scroll_root)
            .set("height", "40px")
            .expect("scroll height");
        root.style_mut(first_item)
            .set("width", "80px")
            .expect("first width");
        root.style_mut(first_item)
            .set("height", "40px")
            .expect("first height");
        root.style_mut(second_item)
            .set("width", "80px")
            .expect("second width");
        root.style_mut(second_item)
            .set("height", "80px")
            .expect("second height");

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        let parent = world
            .spawn((Sprite {
                custom_size: Some(Vec2::new(160.0, 160.0)),
                ..Default::default()
            },))
            .id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("auto overflow proof should attach");
        root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("auto overflow proof should present once before wheel update");

        root.update(
            &mut world,
            0.0,
            DomPointerUpdate::Set(DomPointerState {
                x_px: 10.0,
                y_px: 10.0,
                primary_button_down: false,
                scroll_delta_y_px: -60.0,
            }),
        )
        .expect("wheel update should succeed");

        assert!(
            (root.vertical_scroll_offset_y_px(scroll_root) - 60.0).abs() <= 0.001,
            "overflow:auto should own retained scroll state once its content exceeds the viewport",
        );
    }

    #[test]
    // CSS Overflow 3 plus Gecko/Blink scroll-container policy:
    // `overflow:hidden` still owns scroll state even though the UI is clipped.
    fn wheel_scroll_targets_hidden_overflow_layer_when_content_exceeds_viewport() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let scroll_root = root.create_element(DomTag::Div).expect("scroll root");
        let first_item = root.create_element(DomTag::Div).expect("first item");
        let second_item = root.create_element(DomTag::Div).expect("second item");

        root.append_child(root.root, scroll_root)
            .expect("append scroll root");
        root.append_child(scroll_root, first_item)
            .expect("append first item");
        root.append_child(scroll_root, second_item)
            .expect("append second item");

        root.style_mut(scroll_root)
            .set("overflow", "hidden")
            .expect("hidden overflow");
        root.style_mut(scroll_root)
            .set("width", "80px")
            .expect("scroll width");
        root.style_mut(scroll_root)
            .set("height", "40px")
            .expect("scroll height");
        root.style_mut(first_item)
            .set("width", "80px")
            .expect("first width");
        root.style_mut(first_item)
            .set("height", "40px")
            .expect("first height");
        root.style_mut(second_item)
            .set("width", "80px")
            .expect("second width");
        root.style_mut(second_item)
            .set("height", "80px")
            .expect("second height");

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        let parent = world
            .spawn((Sprite {
                custom_size: Some(Vec2::new(160.0, 160.0)),
                ..Default::default()
            },))
            .id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("hidden overflow proof should attach");
        root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("hidden overflow proof should present once before wheel update");

        root.update(
            &mut world,
            0.0,
            DomPointerUpdate::Set(DomPointerState {
                x_px: 10.0,
                y_px: 10.0,
                primary_button_down: false,
                scroll_delta_y_px: -60.0,
            }),
        )
        .expect("wheel update should succeed");

        assert!(
            (root.vertical_scroll_offset_y_px(scroll_root) - 60.0).abs() <= 0.001,
            "overflow:hidden should own retained scroll state once content exceeds the viewport",
        );
    }

    #[test]
    // The overflow scroll-container split must stay reachable through
    // stylesheets, not only direct attrs. A class-authored `overflow:auto`
    // root should own retained scroll after a later style recompute.
    fn stylesheet_auto_overflow_layer_owns_scroll_after_recompute() {
        let (mut world, mut root, scroll_root) =
            stylesheet_scroll_fixture("scroll-auto", "overflow: auto;");

        root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("auto overflow proof should present once before wheel update");

        root.style_mut(scroll_root)
            .set("color", "#ffffff")
            .expect("style recompute attr");
        root.update(&mut world, 1.0, DomPointerUpdate::Unchanged)
            .expect("style recompute update should succeed");

        root.update(
            &mut world,
            1.0,
            DomPointerUpdate::Set(DomPointerState {
                x_px: 10.0,
                y_px: 10.0,
                primary_button_down: false,
                scroll_delta_y_px: -60.0,
            }),
        )
        .expect("wheel update should succeed");

        assert!(
            (root.vertical_scroll_offset_y_px(scroll_root) - 60.0).abs() <= 0.001,
            "stylesheet overflow:auto should own retained scroll state",
        );
    }

    #[test]
    // `overflow:hidden` remains in the scroll-container family for Gecko/Blink,
    // and the stylesheet path should keep that behavior after recompute too.
    fn stylesheet_hidden_overflow_layer_owns_scroll_after_recompute() {
        let (mut world, mut root, scroll_root) =
            stylesheet_scroll_fixture("scroll-hidden", "overflow: hidden;");

        root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("hidden overflow proof should present once before wheel update");

        root.style_mut(scroll_root)
            .set("color", "#ffffff")
            .expect("style recompute attr");
        root.update(&mut world, 1.0, DomPointerUpdate::Unchanged)
            .expect("style recompute update should succeed");

        root.update(
            &mut world,
            1.0,
            DomPointerUpdate::Set(DomPointerState {
                x_px: 10.0,
                y_px: 10.0,
                primary_button_down: false,
                scroll_delta_y_px: -60.0,
            }),
        )
        .expect("wheel update should succeed");

        assert!(
            (root.vertical_scroll_offset_y_px(scroll_root) - 60.0).abs() <= 0.001,
            "stylesheet overflow:hidden should own retained scroll state",
        );
    }

    #[test]
    fn switching_stylesheet_overflow_class_from_hidden_to_clip_removes_scroll_ownership() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        root.context()
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 0),
                "
                    .scroll-hidden { overflow: hidden; width: 80px; height: 40px; }
                    .scroll-clip { overflow: clip; width: 80px; height: 40px; }
                ",
            )
            .expect("stylesheet");
        let scroll_root = root.create_element(DomTag::Div).expect("scroll root");
        let first_item = root.create_element(DomTag::Div).expect("first item");
        let second_item = root.create_element(DomTag::Div).expect("second item");

        root.append_child(root.root, scroll_root)
            .expect("append scroll root");
        root.append_child(scroll_root, first_item)
            .expect("append first item");
        root.append_child(scroll_root, second_item)
            .expect("append second item");
        root.set_attr_str(scroll_root, "class", "scroll-hidden")
            .expect("hidden class");
        root.style_mut(first_item)
            .set("width", "80px")
            .expect("first width");
        root.style_mut(first_item)
            .set("height", "40px")
            .expect("first height");
        root.style_mut(second_item)
            .set("width", "80px")
            .expect("second width");
        root.style_mut(second_item)
            .set("height", "80px")
            .expect("second height");

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        let parent = world
            .spawn((Sprite {
                custom_size: Some(Vec2::new(160.0, 160.0)),
                ..Default::default()
            },))
            .id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("attach");

        root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("initial update");
        root.update(
            &mut world,
            0.0,
            DomPointerUpdate::Set(DomPointerState {
                x_px: 10.0,
                y_px: 10.0,
                primary_button_down: false,
                scroll_delta_y_px: -60.0,
            }),
        )
        .expect("first wheel update");
        assert_close(
            root.vertical_scroll_offset_y_px(scroll_root),
            60.0,
            "hidden class should own scroll before the class switch",
        );

        root.set_attr_str(scroll_root, "class", "scroll-clip")
            .expect("clip class");
        root.update(&mut world, 1.0, DomPointerUpdate::Unchanged)
            .expect("class switch update");
        root.update(
            &mut world,
            1.0,
            DomPointerUpdate::Set(DomPointerState {
                x_px: 10.0,
                y_px: 10.0,
                primary_button_down: false,
                scroll_delta_y_px: -60.0,
            }),
        )
        .expect("second wheel update");

        assert_close(
            root.vertical_scroll_offset_y_px(scroll_root),
            0.0,
            "switching to overflow:clip should clear retained scroll state back to the non-scroll-container baseline",
        );
    }

    #[test]
    // CSS Overflow 3 plus Blink scroll-container policy:
    // `overflow:clip` clips but forbids scrolling, so wheel deltas must not
    // mutate retained scroll state.
    fn wheel_scroll_does_not_apply_to_clip_overflow_layer() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let clipped_root = root.create_element(DomTag::Div).expect("clipped root");
        let first_item = root.create_element(DomTag::Div).expect("first item");
        let second_item = root.create_element(DomTag::Div).expect("second item");

        root.append_child(root.root, clipped_root)
            .expect("append clipped root");
        root.append_child(clipped_root, first_item)
            .expect("append first item");
        root.append_child(clipped_root, second_item)
            .expect("append second item");

        root.style_mut(clipped_root)
            .set("overflow", "clip")
            .expect("clip overflow");
        root.style_mut(clipped_root)
            .set("width", "80px")
            .expect("scroll width");
        root.style_mut(clipped_root)
            .set("height", "40px")
            .expect("scroll height");
        root.style_mut(first_item)
            .set("width", "80px")
            .expect("first width");
        root.style_mut(first_item)
            .set("height", "40px")
            .expect("first height");
        root.style_mut(second_item)
            .set("width", "80px")
            .expect("second width");
        root.style_mut(second_item)
            .set("height", "80px")
            .expect("second height");

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        let parent = world
            .spawn((Sprite {
                custom_size: Some(Vec2::new(160.0, 160.0)),
                ..Default::default()
            },))
            .id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("clip overflow proof should attach");
        root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("clip overflow proof should present once before wheel update");

        root.update(
            &mut world,
            0.0,
            DomPointerUpdate::Set(DomPointerState {
                x_px: 10.0,
                y_px: 10.0,
                primary_button_down: false,
                scroll_delta_y_px: -60.0,
            }),
        )
        .expect("wheel update should succeed");

        assert!(
            root.vertical_scroll_offset_y_px(clipped_root).abs() <= 0.001,
            "overflow:clip should reject retained scroll state updates",
        );
    }

    #[test]
    fn wheel_scroll_reuses_layer_hit_for_absolute_child_outside_parent_box() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let parent_layer = root.create_element(DomTag::Div).expect("parent layer");
        let scroll_root = root.create_element(DomTag::Div).expect("scroll root");
        let content = root.create_element(DomTag::Div).expect("content");
        let rows: Vec<_> = (0..20)
            .map(|_| root.create_element(DomTag::Div).expect("row"))
            .collect();

        root.append_child(root.root, parent_layer)
            .expect("append parent layer");
        root.append_child(parent_layer, scroll_root)
            .expect("append scroll root");
        root.append_child(scroll_root, content)
            .expect("append content");
        for row in &rows {
            root.append_child(content, *row).expect("append row");
        }

        root.style_mut(parent_layer)
            .set("position", "relative")
            .expect("parent position");
        root.style_mut(parent_layer)
            .set("width", "20px")
            .expect("parent width");
        root.style_mut(parent_layer)
            .set("height", "20px")
            .expect("parent height");
        root.style_mut(scroll_root)
            .set("position", "absolute")
            .expect("scroll position");
        root.style_mut(scroll_root)
            .set("left", "100px")
            .expect("scroll left");
        root.style_mut(scroll_root)
            .set("top", "100px")
            .expect("scroll top");
        root.style_mut(scroll_root)
            .set("width", "160px")
            .expect("scroll width");
        root.style_mut(scroll_root)
            .set("height", "120px")
            .expect("scroll height");
        root.style_mut(scroll_root)
            .set("overflow", "scroll")
            .expect("scroll overflow");
        root.style_mut(content)
            .set("width", "160px")
            .expect("content width");
        for row in &rows {
            root.style_mut(*row)
                .set("width", "160px")
                .expect("row width");
            root.style_mut(*row)
                .set("height", "40px")
                .expect("row height");
        }

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        let parent = world
            .spawn((Sprite {
                custom_size: Some(Vec2::new(320.0, 320.0)),
                ..Default::default()
            },))
            .id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("absolute child proof should attach");
        root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("absolute child proof should present once");
        let root_key = root.root;
        set_box(&mut root, root_key, 0.0, 0.0, 320.0, 320.0);
        set_box(&mut root, parent_layer, 0.0, 0.0, 20.0, 20.0);
        set_box(&mut root, scroll_root, 100.0, 100.0, 160.0, 120.0);
        set_content_height(&mut root, scroll_root, 800.0);

        root.update(
            &mut world,
            0.0,
            DomPointerUpdate::Set(DomPointerState {
                x_px: 140.0,
                y_px: 160.0,
                primary_button_down: false,
                scroll_delta_y_px: -120.0,
            }),
        )
        .expect("wheel update should succeed through absolute child");

        assert!(
            (root.vertical_scroll_offset_y_px(scroll_root) - 120.0).abs() <= 0.001,
            "scroll should reach the absolute child layer even when it sits outside the parent layer box",
        );
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

    fn set_content_height(root: &mut RootDom, key: DomNodeSlotKey, height_px: f32) {
        let node = root.nodes.get_mut(key).expect("node should exist");
        match node {
            DomNode::Document(document) => document.base.layout.content_height_px = height_px,
            DomNode::DocumentFragment(fragment) => {
                fragment.base.layout.content_height_px = height_px;
            }
            DomNode::Element(element) => element.base.layout.content_height_px = height_px,
            DomNode::Text(text) => text.base.layout.content_height_px = height_px,
        }
    }

    #[test]
    // `overflow:clip` should stay non-scrollable through selector-driven style
    // too, even after a later style recompute.
    fn stylesheet_clip_overflow_layer_does_not_own_scroll_after_recompute() {
        let (mut world, mut root, scroll_root) =
            stylesheet_scroll_fixture("scroll-clip", "overflow: clip;");

        root.update(&mut world, 0.0, DomPointerUpdate::Unchanged)
            .expect("clip overflow proof should present once before wheel update");

        root.style_mut(scroll_root)
            .set("color", "#ffffff")
            .expect("style recompute attr");
        root.update(&mut world, 1.0, DomPointerUpdate::Unchanged)
            .expect("style recompute update should succeed");

        root.update(
            &mut world,
            1.0,
            DomPointerUpdate::Set(DomPointerState {
                x_px: 10.0,
                y_px: 10.0,
                primary_button_down: false,
                scroll_delta_y_px: -60.0,
            }),
        )
        .expect("wheel update should succeed");

        assert_close(
            root.vertical_scroll_offset_y_px(scroll_root),
            0.0,
            "stylesheet overflow:clip should not own retained scroll state",
        );
    }

    fn stylesheet_scroll_fixture(
        class_name: &str,
        overflow_rule: &str,
    ) -> (World, RootDom, DomNodeSlotKey) {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        root.context()
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 0),
                &format!(".{class_name} {{ {overflow_rule} width: 80px; height: 40px; }}"),
            )
            .expect("stylesheet");
        let scroll_root = root.create_element(DomTag::Div).expect("scroll root");
        let first_item = root.create_element(DomTag::Div).expect("first item");
        let second_item = root.create_element(DomTag::Div).expect("second item");

        root.append_child(root.root, scroll_root)
            .expect("append scroll root");
        root.append_child(scroll_root, first_item)
            .expect("append first item");
        root.append_child(scroll_root, second_item)
            .expect("append second item");

        root.set_attr_str(scroll_root, "class", class_name)
            .expect("scroll class");
        root.style_mut(first_item)
            .set("width", "80px")
            .expect("first width");
        root.style_mut(first_item)
            .set("height", "40px")
            .expect("first height");
        root.style_mut(second_item)
            .set("width", "80px")
            .expect("second width");
        root.style_mut(second_item)
            .set("height", "80px")
            .expect("second height");

        let mut world = World::new();
        install_direct_gpu_support(&mut world);
        let parent = world
            .spawn((Sprite {
                custom_size: Some(Vec2::new(160.0, 160.0)),
                ..Default::default()
            },))
            .id();
        root.attach_to_bevy_parent_entity(&mut world, parent, 0.0)
            .expect("stylesheet overflow proof should attach");

        (world, root, scroll_root)
    }

    fn assert_close(actual: f32, expected: f32, label: &str) {
        assert!(
            (actual - expected).abs() <= 0.001,
            "{label} should be {expected}, got {actual}",
        );
    }
}
