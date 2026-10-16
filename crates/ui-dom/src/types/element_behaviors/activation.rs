//! Activation behavior definitions for DOM elements.
//!
//! This file owns the activation behavior *definitions*: the per-element
//! keyboard, pointer, focus, blur, and activation descriptors plus the
//! retained pressed-pointer-click and range-drag session state. The
//! default-behavior lifecycle methods that execute those definitions live
//! on `RootDom` in `types/root_dom.rs`.

use crate::types::DomNodeSlotKey;
use crate::types::primitives::PointPx;

/// Selects which behavior gets first chance at keyboard interaction.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DomElementOnKeyboard {
    #[default]
    None,
    Button,
    InputControl,
    /// Runs the built-in select keyboard behavior for opening and committing the picker.
    Select,
    TextEdit,
    PresentedOptions,
}

/// Selects which behavior gets first chance at pointer interaction.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DomElementOnPointer {
    #[default]
    None,
    InputControl,
    TextEdit,
}

/// Selects the post-click lifecycle for one element.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DomElementAfterClick {
    #[default]
    None,
    Activate,
}

/// Selects the committed activation action for one element.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DomElementOnActivation {
    #[default]
    None,
    /// Runs the built-in select activation behavior after an uncanceled click.
    Select,
    Toggle,
    PresentedOptions,
}

/// Selects the work that runs after one element becomes focused.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DomElementAfterFocus {
    #[default]
    None,
    PresentedOptions,
}

/// Selects which behavior runs when one element loses focus.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DomElementOnBlur {
    #[default]
    None,
    /// Runs the built-in select blur behavior for closing the picker.
    Select,
    PresentedOptions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PressedPointerClickTarget {
    pub(crate) target_key: DomNodeSlotKey,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ActiveRangeDragSession {
    pub(crate) target_key: DomNodeSlotKey,
    pub(crate) root_to_subtree_point_offset: PointPx,
    pub(crate) focus_entry_value: String,
    pub(crate) dirty: bool,
}

#[cfg(test)]
use crate::domains::hit_test::hit_test_from_dom_subtree_cluster_local;
#[cfg(test)]
use crate::domains::layer_hit::topmost_attached_layer_hit_from_root_point;
#[cfg(test)]
use crate::domains::layout::node_box_from_ref;
#[cfg(test)]
use crate::domains::paint::build_layer_paint_plan;
#[cfg(test)]
use crate::domains::visitor::inherited_style::InheritedStyleContext;
#[cfg(test)]
use crate::types::dom_event::{DomEvent, DomEventName};
#[cfg(test)]
use crate::types::{
    DomAttribute, DomAttributeValue, DomNode, DomPointerState, DomPointerUpdate, DomRect, DomTag,
    RootDom, RootDomContext,
};
#[cfg(test)]
use crate::{LayerConfig, LayerPool, LayerPoolRenderJobQueue, UiDomRenderSupport};

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

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= 0.001,
            "expected {expected}, got {actual}"
        );
    }

    fn append_select(root: &mut RootDom) -> DomNodeSlotKey {
        let select = root.create_element(DomTag::Select).expect("select");
        let first_option = root.create_element(DomTag::Option).expect("first option");
        let second_option = root.create_element(DomTag::Option).expect("second option");
        let first_text = root.create_text_node("alpha").expect("first text");
        let second_text = root.create_text_node("beta").expect("second text");
        root.append_child(root.root, select).expect("append select");
        root.append_child(select, first_option)
            .expect("append first option");
        root.append_child(select, second_option)
            .expect("append second option");
        root.append_child(first_option, first_text)
            .expect("append first text");
        root.append_child(second_option, second_text)
            .expect("append second text");
        root.set_attr_str(first_option, "value", "alpha")
            .expect("set first option value");
        root.set_attr_str(second_option, "value", "beta")
            .expect("set second option value");
        root.set_attr_str(select, "width", "120px").expect("width");
        root.set_attr_str(select, "height", "28px").expect("height");
        select
    }

    fn append_select_to_parent(root: &mut RootDom, parent: DomNodeSlotKey) -> DomNodeSlotKey {
        let select = root.create_element(DomTag::Select).expect("select");
        let first_option = root.create_element(DomTag::Option).expect("first option");
        let second_option = root.create_element(DomTag::Option).expect("second option");
        let first_text = root.create_text_node("alpha").expect("first text");
        let second_text = root.create_text_node("beta").expect("second text");
        root.append_child(parent, select).expect("append select");
        root.append_child(select, first_option)
            .expect("append first option");
        root.append_child(select, second_option)
            .expect("append second option");
        root.append_child(first_option, first_text)
            .expect("append first text");
        root.append_child(second_option, second_text)
            .expect("append second text");
        root.set_attr_str(first_option, "value", "alpha")
            .expect("set first option value");
        root.set_attr_str(second_option, "value", "beta")
            .expect("set second option value");
        root.set_attr_str(select, "width", "120px").expect("width");
        root.set_attr_str(select, "height", "28px").expect("height");
        select
    }

    fn attach_root_with_size(
        root: &mut RootDom,
        world: &mut World,
        width_px: f32,
        height_px: f32,
    ) -> Entity {
        install_direct_gpu_support(world);
        let parent = world
            .spawn((Sprite {
                custom_size: Some(Vec2::new(width_px, height_px)),
                ..Default::default()
            },))
            .id();
        root.attach_to_bevy_parent_entity(world, parent, 0.0)
            .expect("attach root");
        root.update(world, 0.0, DomPointerUpdate::Unchanged)
            .expect("present once");
        parent
    }

    fn pointer_update(x_px: f32, y_px: f32, primary_button_down: bool) -> DomPointerUpdate {
        DomPointerUpdate::Set(DomPointerState {
            x_px,
            y_px,
            primary_button_down,
            scroll_delta_y_px: 0.0,
        })
    }

    fn event_names(events: &[DomEvent]) -> Vec<DomEventName> {
        events.iter().map(|event| event.name).collect()
    }

    fn input_value(root: &RootDom, input: DomNodeSlotKey) -> String {
        let element = root
            .node(input)
            .and_then(DomNode::as_element)
            .expect("input element should exist");
        match element.attrs.attributes.get(&DomAttribute::Value) {
            Some(DomAttributeValue::String(value)) => value.clone(),
            other => panic!("expected input string value, got {other:?}"),
        }
    }

    struct SelectPopupProof {
        shell_entity: Entity,
        shell_box: DomRect,
        row_keys: Vec<DomNodeSlotKey>,
        popup_scroll_key: DomNodeSlotKey,
        popup_scroll_entity: Entity,
    }

    fn select_popup_proof(root: &RootDom, select: DomNodeSlotKey) -> SelectPopupProof {
        let shell_key = root
            .presented_options_popup_shell_key(select)
            .expect("select popup shell should exist");
        let shell_entity = root
            .layer_entity(shell_key)
            .expect("select popup shell entity");
        let shell_box = node_box_from_ref(root.node(shell_key).expect("shell")).border_box;
        let row_keys = root.presented_options_popup_row_keys(select);
        let first_row_key = row_keys
            .first()
            .copied()
            .expect("popup should contain at least one row");
        let popup_scroll_key = root
            .node(first_row_key)
            .and_then(DomNode::parent_key)
            .expect("popup row should have a scroll parent");
        let popup_scroll_entity = root
            .layer_entity(popup_scroll_key)
            .expect("popup scroll layer should have a Bevy entity");
        SelectPopupProof {
            shell_entity,
            shell_box,
            row_keys,
            popup_scroll_key,
            popup_scroll_entity,
        }
    }

    fn assert_popup_surface_order_and_text(
        root: &mut RootDom,
        world: &World,
        parent: Entity,
        proof: &SelectPopupProof,
    ) {
        assert!(
            proof.shell_box.height_px >= 56.0,
            "select popup shell should keep enough height for two rows, got {}",
            proof.shell_box.height_px
        );
        assert_eq!(proof.row_keys.len(), 2);
        let ordered_surface_entities = world
            .resource::<LayerPoolRenderJobQueue>()
            .ordered_surface_entities_for_test(parent.into())
            .expect("root update should publish retained surface order");
        assert_eq!(
            ordered_surface_entities
                .iter()
                .position(|entity| *entity == proof.shell_entity.into()),
            Some(1),
            "popup shell should be raised above the document layer"
        );
        assert_eq!(
            ordered_surface_entities
                .iter()
                .position(|entity| *entity == proof.popup_scroll_entity.into()),
            Some(2),
            "popup scroll child should render after its raised shell"
        );
        assert_popup_text_runs_inside_scroll_surface(root, proof.popup_scroll_key);
    }

    fn assert_popup_text_runs_inside_scroll_surface(
        root: &mut RootDom,
        popup_scroll_key: DomNodeSlotKey,
    ) {
        let mut layer_pool = LayerPool::new(LayerConfig::default());
        let popup_scroll_plan = build_layer_paint_plan(
            root,
            &mut layer_pool,
            popup_scroll_key,
            (120.0, 56.0),
            &InheritedStyleContext::default(),
        )
        .expect("popup scroll paint plan");
        assert_eq!(popup_scroll_plan.text_runs.len(), 2);
        assert_close(popup_scroll_plan.text_runs[0].position_px[0], 8.0);
        assert_close(popup_scroll_plan.text_runs[0].position_px[1], 5.0);
        assert_close(popup_scroll_plan.text_runs[0].size_px[1], 18.0);
        assert_close(popup_scroll_plan.text_runs[0].line_height_px, 18.0);
        assert_close(popup_scroll_plan.text_runs[1].position_px[0], 8.0);
        assert_close(popup_scroll_plan.text_runs[1].position_px[1], 33.0);
        assert_close(popup_scroll_plan.text_runs[1].size_px[1], 18.0);
        assert_close(popup_scroll_plan.text_runs[1].line_height_px, 18.0);
        for text_run in &popup_scroll_plan.text_runs {
            assert!(
                text_run.size_px[0] > 0.0 && text_run.size_px[1] > 0.0,
                "popup option text run should paint with non-zero size: {text_run:?}"
            );
            assert!(
                text_run.position_px[0] >= 0.0
                    && text_run.position_px[0] < 120.0
                    && text_run.position_px[1] >= 0.0
                    && text_run.position_px[1] < 56.0,
                "popup option text run should paint inside the scroll surface: {text_run:?}"
            );
        }
    }

    fn assert_second_popup_row_mouse_click_commits(
        root: &mut RootDom,
        world: &mut World,
        parent: Entity,
        select: DomNodeSlotKey,
        proof: &SelectPopupProof,
    ) {
        let second_row_point = PointPx {
            x_px: 48.0,
            y_px: 130.0,
        };
        let second_row_layer_hit =
            topmost_attached_layer_hit_from_root_point(root, world, second_row_point)
                .expect("popup row layer hit should resolve");
        let second_row_subtree_hit = second_row_layer_hit.and_then(|layer_hit| {
            hit_test_from_dom_subtree_cluster_local(
                root,
                layer_hit.node_key,
                layer_hit.point,
                layer_hit.effective_rect,
            )
            .expect("popup row subtree hit should resolve")
        });
        let root_box = node_box_from_ref(root.node(root.root).expect("root")).border_box;
        assert_eq!(
            root.point_event_target_key_from_attached_point(world, parent, second_row_point)
                .expect("popup row hit target should resolve"),
            Some(proof.row_keys[1]),
            "pointer hit target should be the popup row under the cursor; root box: {root_box:?}; shell box: {:?}; layer hit: {second_row_layer_hit:?}; subtree hit: {second_row_subtree_hit:?}",
            proof.shell_box,
        );
        let down_events = root
            .update(
                world,
                0.0,
                pointer_update(second_row_point.x_px, second_row_point.y_px, true),
            )
            .expect("second row down");
        let target_after_down = root
            .point_event_target_key_from_attached_point(world, parent, second_row_point)
            .expect("target after row down should resolve");
        assert_eq!(
            target_after_down,
            Some(proof.row_keys[1]),
            "popup row should remain the hit target after pointer down; down events: {down_events:?}",
        );
        let up_events = root
            .update(
                world,
                0.0,
                pointer_update(second_row_point.x_px, second_row_point.y_px, false),
            )
            .expect("second row up");
        assert_eq!(
            input_value(root, select),
            "beta",
            "mouse click on a raised popup row should commit the row value; down events: {down_events:?}; up events: {up_events:?}",
        );
    }

    #[test]
    fn pointer_click_opens_select_presented_options_after_click_activation() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let select = append_select(&mut root);
        root.set_attr_str(select, "margin-left", "12px")
            .expect("select margin left");
        root.set_attr_str(select, "margin-top", "40px")
            .expect("select margin top");
        root.set_border_box_for_test(root.root, 0.0, 0.0, 180.0, 180.0);
        let mut world = World::new();
        attach_root_with_size(&mut root, &mut world, 180.0, 180.0);
        root.reset_metrics();
        let point = PointPx {
            x_px: 17.0,
            y_px: 45.0,
        };

        let _ = root
            .update(
                &mut world,
                0.0,
                pointer_update(point.x_px, point.y_px, true),
            )
            .expect("down");
        let up_events = root
            .update(
                &mut world,
                0.0,
                pointer_update(point.x_px, point.y_px, false),
            )
            .expect("up");

        assert_eq!(
            event_names(&up_events),
            vec![DomEventName::PointerUp, DomEventName::Click]
        );
        let shell_key = root
            .presented_options_popup_shell_key(select)
            .expect("select popup shell should exist");
        assert!(
            root.layer_entity(shell_key).is_some(),
            "select popup shell should materialize as a layer"
        );
        assert!(
            root.raised_layer_roots().contains(&shell_key),
            "select popup shell should use raised layer ordering"
        );
        let shell_entity = root
            .layer_entity(shell_key)
            .expect("select popup shell entity");
        let shell_transform = world
            .get::<Transform>(shell_entity)
            .expect("select popup shell transform");
        assert_close(shell_transform.translation.x, 12.0);
        assert_close(shell_transform.translation.y, -68.0);
    }

    #[test]
    fn pointer_click_anchors_select_popup_to_nested_owner_box() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let wrapper = root.create_element(DomTag::Div).expect("wrapper");
        root.append_child(root.root, wrapper)
            .expect("append wrapper");
        root.set_attr_str(wrapper, "display", "block")
            .expect("wrapper display");
        root.set_attr_str(wrapper, "margin-left", "31px")
            .expect("wrapper margin left");
        root.set_attr_str(wrapper, "margin-top", "53px")
            .expect("wrapper margin top");
        root.set_attr_str(wrapper, "padding-left", "9px")
            .expect("wrapper padding left");
        root.set_attr_str(wrapper, "padding-top", "7px")
            .expect("wrapper padding top");
        root.set_attr_str(root.root, "width", "240px")
            .expect("root width");
        root.set_attr_str(root.root, "height", "240px")
            .expect("root height");
        let select = append_select_to_parent(&mut root, wrapper);
        let mut world = World::new();
        let parent = attach_root_with_size(&mut root, &mut world, 240.0, 240.0);
        root.reset_metrics();
        let point = PointPx {
            x_px: 45.0,
            y_px: 65.0,
        };

        let _ = root
            .update(
                &mut world,
                0.0,
                pointer_update(point.x_px, point.y_px, true),
            )
            .expect("down");
        let _ = root
            .update(
                &mut world,
                0.0,
                pointer_update(point.x_px, point.y_px, false),
            )
            .expect("up");

        let proof = select_popup_proof(&root, select);
        let shell_transform = world
            .get::<Transform>(proof.shell_entity)
            .expect("select popup shell transform");
        assert_popup_surface_order_and_text(&mut root, &world, parent, &proof);
        assert_close(shell_transform.translation.x, 40.0);
        assert_close(shell_transform.translation.y, -88.0);
        assert_second_popup_row_mouse_click_commits(&mut root, &mut world, parent, select, &proof);
    }
}
