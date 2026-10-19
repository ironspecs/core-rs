//! GPU-retained paint boundary for `ui-dom`.
//!
//! Paint ownership lives here: dirty detection, retained target reuse, GPU
//! paint preparation, revision bumps, and paint-state clearing after a
//! successful retained commit.
//!
//! Presentation reads (background/border/outline/caret colors, radius,
//! opacity) come from the node's typed computed groups
//! (`computed_visual()`/`computed_color()`/`computed_box()`, spine step 3.3c),
//! not from stringly presented attrs. Text-node paint color is the one
//! exception: text nodes are never projected, so their color comes from the
//! carried inherited context (`inherited_text_color_rgba`).

mod mark_dirty;
mod order_modified_children;

use bevy::prelude::World;

use crate::domains::layer_pool::{LayerPool, LayerPoolMetricName};
use crate::domains::layout::{node_box_from_ref, relative_offset_for_node};
use crate::domains::paint::order_modified_children::visit_order_modified_children;
use crate::domains::render_ops::{StageOutcome, StagedLayerApply};
use crate::domains::style::computed::DisplayKw;
use crate::domains::text_layout::{
    DEFAULT_TEXT_FONT_SIZE_PX, TextIntrinsicMetrics, TextLanguageId, TextLayoutRequest,
    text_align_for_css_value, text_layout_input_for_wrap_policy, text_wrap_mode_for_css_intrinsic,
    text_wrap_mode_for_css_layout, with_repo_text_layout_context,
};
use crate::domains::visitor::inherited_style::{
    InheritedStyleContext, inherited_context_from_node,
};
use crate::types::element_behaviors::{ElementPaintContext, append_specialized_element_paint_ops};
use crate::types::primitives::PointPx;
use crate::types::{
    DomBoxModel, DomElementNode, DomError, DomIntrinsic, DomLayerKind, DomNode, DomNodeSlotKey,
    DomTag, PaintBackend, RootDom, RootDomMetricName, TextIntrinsic,
};

#[cfg(test)]
use crate::types::DomAttribute;
#[cfg(test)]
use crate::types::DomAttributeValue;

#[cfg(test)]
use crate::StyleRuleOrigin;
#[cfg(test)]
use crate::domains::layer_ops::{ImagePaintId, TextAlign, TextWrapMode};
#[cfg(test)]
use crate::domains::layer_pool::LayerConfig;
#[cfg(test)]
use crate::domains::layout::{IdentityDomLayoutResolver, relayout_tree};
#[cfg(test)]
use crate::domains::style::name_is_presentation;
#[cfg(test)]
use crate::domains::visitor::style_recompute::recompute_dirty_style_subtree_for_layer_root;
#[cfg(test)]
use crate::types::dom_node::DomTextNode;
#[cfg(test)]
use crate::types::{PixelDensity, RootDomContext};

#[cfg(test)]
pub(crate) use crate::domains::layer_ops::{IconPaintOp, ImagePaintOp};
pub(crate) use crate::domains::layer_ops::{LayerPaintPlan, RectPaintOp, TextRunPaintOp};
pub(crate) use mark_dirty::{
    mark_full_repaint_up_to_layout_root, mark_node_dirty_up_to_layout_root,
};

#[tracing::instrument(
    name = "paint_updated",
    level = "debug",
    skip(root_dom, world),
    fields(
        dom_parent = ?root_dom.attached_bevy_parent_entity(),
        layer_root_key = ?layer_root_key,
        logical_width_px = logical_size.0,
        logical_height_px = logical_size.1
    )
)]
pub(crate) fn paint_layer(
    root_dom: &mut RootDom,
    world: &mut World,
    layer_root_key: DomNodeSlotKey,
    lease: crate::domains::LayerLease,
    logical_size: (f32, f32),
    inherited_style: &InheritedStyleContext,
) -> Result<(), DomError> {
    if root_dom.context().paint_config().backend != PaintBackend::Gpu {
        return Ok(());
    }
    root_dom.increment_metric(RootDomMetricName::PaintUpdated);
    root_dom.increment_metric(RootDomMetricName::PaintPlanBuilt);
    if root_dom.layer_entity(layer_root_key).is_none() {
        return Ok(());
    }
    let plan = world.resource_scope(|_, mut layer_pool: bevy::prelude::Mut<LayerPool>| {
        build_layer_paint_plan(
            root_dom,
            &mut layer_pool,
            layer_root_key,
            logical_size,
            inherited_style,
        )
    })?;
    if plan.is_empty() {
        clear_paint_repaint_state_for_painted_layer(root_dom, layer_root_key)?;
        return Ok(());
    }
    let target = match LayerPool::acquire_paint_target_in_world(world, lease) {
        Ok(Some(target)) => target,
        Ok(None) => {
            clear_paint_repaint_state_for_painted_layer(root_dom, layer_root_key)?;
            root_dom.increment_metric(RootDomMetricName::LayerPainted);
            return Ok(());
        }
        Err(_) => return Err(DomError::RetainedGpuApplyFailed),
    };
    let outcome = root_dom.context().render_ops().stage(
        StagedLayerApply {
            lease,
            image_handle: target.image_handle,
            layer_zero_in_image_px: target.layer_zero_in_image_px,
            plan,
        },
        target.policy,
    );
    if outcome == StageOutcome::Deferred {
        world
            .resource_mut::<LayerPool>()
            .increment_metric(LayerPoolMetricName::UploadDeferred);
    }
    clear_paint_repaint_state_for_painted_layer(root_dom, layer_root_key)?;
    root_dom.increment_metric(RootDomMetricName::LayerPainted);
    Ok(())
}

pub(crate) fn build_layer_paint_plan(
    root_dom: &mut RootDom,
    layer_pool: &mut LayerPool,
    layer_root_key: DomNodeSlotKey,
    _logical_size: (f32, f32),
    inherited_style: &InheritedStyleContext,
) -> Result<LayerPaintPlan, DomError> {
    root_dom
        .node(layer_root_key)
        .map(|_| ())
        .ok_or(DomError::InvalidNodeKey)?;
    let mut plan = LayerPaintPlan::default();
    derive_paint_ops_into_plan(
        root_dom,
        layer_pool,
        layer_root_key,
        layer_root_key,
        inherited_style,
        &mut plan,
    )?;
    Ok(plan)
}

pub(crate) fn layer_tree_needs_paint(
    root_dom: &RootDom,
    node_key: DomNodeSlotKey,
) -> Result<bool, DomError> {
    node_paint_dirty(root_dom, node_key)
}

fn node_paint_dirty(root_dom: &RootDom, node_key: DomNodeSlotKey) -> Result<bool, DomError> {
    Ok(root_dom
        .node(node_key)
        .ok_or(DomError::InvalidNodeKey)?
        .paint_repaint_state()
        .is_dirty())
}

/// Computes one node's four corner radii (`[TL, TR, BR, BL]` px) from the
/// computed visual group. The projection already expands the `border-radius`
/// shorthand and per-corner longhands into the group's `radius` field
/// (spine step 3.3), so paint reads the typed group instead of re-parsing
/// stringly presented attrs.
pub(crate) fn border_radii_px_for_node(node: &DomNode) -> [f32; 4] {
    node.computed_visual().radius
}

fn derive_paint_ops_into_plan(
    root_dom: &mut RootDom,
    layer_pool: &mut LayerPool,
    layer_root_key: DomNodeSlotKey,
    current_key: DomNodeSlotKey,
    inherited_style: &InheritedStyleContext,
    plan: &mut LayerPaintPlan,
) -> Result<(), DomError> {
    let absolute_offset = offset_within_painted_layer(root_dom, layer_root_key, current_key);
    let (child_style, paint_result) = {
        let node = root_dom.node(current_key).ok_or(DomError::InvalidNodeKey)?;
        if current_key != layer_root_key && node.layout().layer_kind != DomLayerKind::None {
            return Ok(());
        }
        let box_model = paint_box_from_node(node_box_from_ref(node), absolute_offset);
        let child_style = inherited_context_from_node(node, inherited_style);
        let paint_result = add_node_paint_ops(
            current_key,
            root_dom,
            node,
            layer_pool,
            box_model,
            inherited_style,
            plan,
        )?;
        (child_style, paint_result)
    };
    if let Some(text_run_id_hint) = paint_result.text_run_id_hint {
        let node = root_dom
            .nodes
            .get_mut(current_key)
            .ok_or(DomError::InvalidNodeKey)?;
        let DomNode::Text(text_node) = node else {
            panic!("text hint updates should only target text nodes");
        };
        text_node.set_text_run_id_hint(Some(text_run_id_hint));
    }
    derive_child_paint_ops_into_plan(
        root_dom,
        layer_pool,
        layer_root_key,
        current_key,
        &child_style,
        plan,
    )?;
    Ok(())
}

struct AddNodePaintOpsResult {
    text_run_id_hint: Option<crate::domains::layer_ops::TextRunId>,
}

fn offset_within_painted_layer(
    root_dom: &RootDom,
    layer_root_key: DomNodeSlotKey,
    current_key: DomNodeSlotKey,
) -> PointPx {
    if current_key == layer_root_key {
        return PointPx::default();
    }
    relative_offset_for_node(root_dom, current_key)
}

fn derive_child_paint_ops_into_plan(
    root_dom: &mut RootDom,
    layer_pool: &mut LayerPool,
    layer_root_key: DomNodeSlotKey,
    parent_key: DomNodeSlotKey,
    child_style: &InheritedStyleContext,
    plan: &mut LayerPaintPlan,
) -> Result<(), DomError> {
    let (parent_is_select, select_popup_shell_key, parent_is_flex) = {
        let parent_node = root_dom.node(parent_key).ok_or(DomError::InvalidNodeKey)?;
        let parent_is_select = node_is_select_element(Some(parent_node));
        let select_popup_shell_key = if parent_is_select {
            root_dom.presented_options_popup_shell_key(parent_key)
        } else {
            None
        };
        let parent_is_flex = node_is_flex_container(Some(parent_node));
        (parent_is_select, select_popup_shell_key, parent_is_flex)
    };
    if parent_is_flex {
        // Flex containers paint their children in order-modified document
        // order (CSS Flexbox §5.4.1); non-flex parents keep the DOM walk.
        visit_order_modified_children(root_dom, parent_key, |root, current_key| {
            if parent_is_select && Some(current_key) != select_popup_shell_key {
                return Ok(());
            }
            derive_paint_ops_into_plan(
                root,
                layer_pool,
                layer_root_key,
                current_key,
                child_style,
                plan,
            )
        })?;
        return Ok(());
    }
    let mut child_key = root_dom
        .node(parent_key)
        .ok_or(DomError::InvalidNodeKey)?
        .first_child_key_for_store();
    while let Some(current_key) = child_key {
        let child_node = root_dom.node(current_key).ok_or(DomError::InvalidNodeKey)?;
        child_key = child_node.next_sibling_key_for_store();
        if parent_is_select && Some(current_key) != select_popup_shell_key {
            continue;
        }
        derive_paint_ops_into_plan(
            root_dom,
            layer_pool,
            layer_root_key,
            current_key,
            child_style,
            plan,
        )?;
    }
    Ok(())
}

fn node_is_select_element(node: Option<&DomNode>) -> bool {
    node.and_then(DomNode::as_element)
        .is_some_and(|element| element.tag == DomTag::Select)
}

/// Returns `true` when `node` is a flex container (`display: flex` or
/// `display: inline-flex`). Reads the computed box group's `display` keyword
/// (`DomNode::computed_box().display`, `types/dom_node.rs`), the same computed
/// projection the layout solver's `presented_display` maps to `LayoutDisplay`
/// (`domains/layout/style.rs`), but through the public computed surface the
/// paint walk already holds — no extra store read beyond the parent fetch the
/// walk performs anyway.
fn node_is_flex_container(node: Option<&DomNode>) -> bool {
    node.is_some_and(|node| {
        matches!(
            node.computed_box().display,
            DisplayKw::Flex | DisplayKw::InlineFlex
        )
    })
}

fn clear_subtree_paint_repaint_state(
    root_dom: &mut RootDom,
    subtree_root_key: DomNodeSlotKey,
) -> Result<(), DomError> {
    let mut child_key = root_dom
        .node(subtree_root_key)
        .ok_or(DomError::InvalidNodeKey)?
        .first_child_key_for_store();
    while let Some(current_child_key) = child_key {
        let child = root_dom
            .node(current_child_key)
            .ok_or(DomError::InvalidNodeKey)?;
        child_key = child.next_sibling_key_for_store();
        clear_subtree_paint_repaint_state(root_dom, current_child_key)?;
    }
    root_dom
        .nodes
        .get_mut(subtree_root_key)
        .ok_or(DomError::InvalidNodeKey)?
        .clear_paint_repaint_state();
    Ok(())
}

/// Clears paint dirt for exactly the node set today's paint walk clears, in one
/// act at a terminal paint decision (C3): the painted subtree plus the
/// select-popup skipped subtrees, and nothing else.
fn clear_paint_repaint_state_for_painted_layer(
    root_dom: &mut RootDom,
    layer_root_key: DomNodeSlotKey,
) -> Result<(), DomError> {
    clear_paint_repaint_state_from_node(root_dom, layer_root_key, layer_root_key)
}

fn clear_paint_repaint_state_from_node(
    root_dom: &mut RootDom,
    layer_root_key: DomNodeSlotKey,
    current_key: DomNodeSlotKey,
) -> Result<(), DomError> {
    {
        let node = root_dom.node(current_key).ok_or(DomError::InvalidNodeKey)?;
        if current_key != layer_root_key && node.layout().layer_kind != DomLayerKind::None {
            return Ok(());
        }
    }
    root_dom
        .nodes
        .get_mut(current_key)
        .ok_or(DomError::InvalidNodeKey)?
        .clear_paint_repaint_state();
    let parent_is_select = node_is_select_element(root_dom.node(current_key));
    let select_popup_shell_key = if parent_is_select {
        root_dom.presented_options_popup_shell_key(current_key)
    } else {
        None
    };
    let mut child_key = root_dom
        .node(current_key)
        .ok_or(DomError::InvalidNodeKey)?
        .first_child_key_for_store();
    while let Some(child_current_key) = child_key {
        let child_node = root_dom
            .node(child_current_key)
            .ok_or(DomError::InvalidNodeKey)?;
        child_key = child_node.next_sibling_key_for_store();
        if parent_is_select && Some(child_current_key) != select_popup_shell_key {
            clear_subtree_paint_repaint_state(root_dom, child_current_key)?;
            continue;
        }
        clear_paint_repaint_state_from_node(root_dom, layer_root_key, child_current_key)?;
    }
    Ok(())
}

fn add_node_paint_ops(
    current_key: DomNodeSlotKey,
    root_dom: &RootDom,
    node: &DomNode,
    layer_pool: &mut LayerPool,
    box_model: DomBoxModel,
    inherited_style: &InheritedStyleContext,
    plan: &mut LayerPaintPlan,
) -> Result<AddNodePaintOpsResult, DomError> {
    match node {
        DomNode::Document(_) | DomNode::DocumentFragment(_) => Ok(AddNodePaintOpsResult {
            text_run_id_hint: None,
        }),
        DomNode::Element(element) => add_element_paint_ops(
            ElementPaintInputs {
                current_key,
                root_dom,
                node,
                element,
                layer_pool,
                plan,
            },
            box_model,
        )
        .map(|_| AddNodePaintOpsResult {
            text_run_id_hint: None,
        }),
        DomNode::Text(_) => {
            let DomIntrinsic::Text(text_intrinsic) = node.intrinsic() else {
                panic!("text node should keep text intrinsic state");
            };
            let rect = box_model.border_box;
            let text_node = node.as_text().expect("text node should expose text data");
            let white_space = inherited_style.effective_white_space();
            let text_run_text = match text_layout_input_for_wrap_policy(
                text_node.data(),
                white_space,
                inherited_style.effective_word_break(),
                TextLanguageId::from_text_hint(text_node.data()),
            ) {
                std::borrow::Cow::Borrowed(_) => text_node.data_arc(),
                std::borrow::Cow::Owned(text) => std::sync::Arc::<str>::from(text),
            };
            let text_run_signature =
                crate::domains::layer_ops::TextRunSignature::from_text(text_run_text);
            let text_run_id = layer_pool
                .resolve_text_run_id_from_hint(&text_run_signature, text_node.text_run_id_hint());
            let font_size_px = text_intrinsic.font_size_px;
            let text_run = TextRunPaintOp {
                text_run_id,
                position_px: [rect.left_px, rect.top_px],
                size_px: [rect.width_px, rect.height_px],
                font_size_px,
                line_height_px: text_intrinsic.line_height_px,
                wrap_mode: text_wrap_mode_for_css_layout(
                    white_space,
                    inherited_style.effective_overflow_wrap(),
                    inherited_style.effective_word_break(),
                ),
                alignment: text_align_for_css_value(
                    inherited_style.effective_text_align(),
                    inherited_style.effective_text_direction(),
                ),
                color_rgba: inherited_text_color_rgba(inherited_style),
            };
            // Zero-size text renders nothing (CSS Fonts 4 §3.5); cosmic-text
            // asserts a non-zero font size, so skip the run rather than reaching
            // cosmic. The hint still returns the id for cheap reuse.
            if font_size_px != 0.0 {
                plan.text_runs.push(text_run);
            }
            Ok(AddNodePaintOpsResult {
                text_run_id_hint: Some(text_run_id),
            })
        }
    }
}

struct ElementPaintInputs<'a> {
    current_key: DomNodeSlotKey,
    root_dom: &'a RootDom,
    node: &'a DomNode,
    element: &'a DomElementNode,
    layer_pool: &'a mut LayerPool,
    plan: &'a mut LayerPaintPlan,
}

fn add_element_paint_ops(
    inputs: ElementPaintInputs<'_>,
    box_model: DomBoxModel,
) -> Result<bool, DomError> {
    let ElementPaintInputs {
        current_key,
        root_dom,
        node,
        element,
        layer_pool,
        plan,
    } = inputs;
    let rect = box_model.border_box;
    let position_px = [rect.left_px, rect.top_px];
    let (width_px, height_px) = paintable_node_size(rect.width_px, rect.height_px);
    let visual = node.computed_visual();
    if width_px > 0.0 && height_px > 0.0 {
        plan.rects.push(RectPaintOp {
            position_px,
            size_px: [width_px, height_px],
            fill_rgba: visual.background_rgba,
            corner_radii_px: visual.radius,
            border_widths_px: [
                box_model.border.top_px,
                box_model.border.right_px,
                box_model.border.bottom_px,
                box_model.border.left_px,
            ],
            border_top_rgba: visual.border_rgba[0],
            border_right_rgba: visual.border_rgba[1],
            border_bottom_rgba: visual.border_rgba[2],
            border_left_rgba: visual.border_rgba[3],
        });
        if let Some(outline_rect) = outline_rect_paint_op(node, box_model) {
            plan.rects.push(outline_rect);
        }
        append_specialized_element_paint_ops(&mut ElementPaintContext {
            current_key,
            root_dom,
            node,
            element,
            box_model,
            layer_pool,
            plan,
        })?;
    }
    Ok(width_px > 0.0 && height_px > 0.0)
}

fn outline_rect_paint_op(node: &DomNode, box_model: DomBoxModel) -> Option<RectPaintOp> {
    let visual = node.computed_visual();
    let outline_width_px = visual.outline_width;
    let outline_offset_px = visual.outline_offset;
    let outline_color_rgba = visual.outline_rgba;
    if outline_width_px <= 0.0 || outline_color_rgba[3] <= 0.0 {
        return None;
    }
    let expansion_px = outline_width_px + outline_offset_px;
    let rect = box_model.border_box;
    let border_radii_px = visual.radius;
    Some(RectPaintOp {
        position_px: [rect.left_px - expansion_px, rect.top_px - expansion_px],
        size_px: [
            (rect.width_px + expansion_px * 2.0).max(0.0),
            (rect.height_px + expansion_px * 2.0).max(0.0),
        ],
        fill_rgba: [0.0; 4],
        corner_radii_px: [
            border_radii_px[0] + expansion_px,
            border_radii_px[1] + expansion_px,
            border_radii_px[2] + expansion_px,
            border_radii_px[3] + expansion_px,
        ],
        border_widths_px: [outline_width_px; 4],
        border_top_rgba: outline_color_rgba,
        border_right_rgba: outline_color_rgba,
        border_bottom_rgba: outline_color_rgba,
        border_left_rgba: outline_color_rgba,
    })
}

fn paint_box_from_node(mut box_model: DomBoxModel, absolute_offset: PointPx) -> DomBoxModel {
    box_model.border_box.left_px = absolute_offset.x_px;
    box_model.border_box.top_px = absolute_offset.y_px;
    box_model
}

/// The inherited text color for a text node. Text nodes are never projected,
/// so their color group stays at the initial near-white; the painted text
/// color keeps coming from the carried inherited context, whose color group
/// equals the nearest ancestor element's computed color (spine 3.3d, sourced
/// from groups instead of the legacy stringly `color` attr).
fn inherited_text_color_rgba(inherited_style: &InheritedStyleContext) -> [f32; 4] {
    inherited_style.effective_color()
}

fn paintable_node_size(node_width_px: f32, node_height_px: f32) -> (f32, f32) {
    (node_width_px.max(0.0), node_height_px.max(0.0))
}

pub(crate) fn measure_text_intrinsic(text: &str) -> TextIntrinsic {
    measure_text_intrinsic_with_font_size(text, DEFAULT_TEXT_FONT_SIZE_PX)
}

pub(crate) fn measure_text_intrinsic_with_font_size(
    text: &str,
    font_size_px: f32,
) -> TextIntrinsic {
    measure_text_intrinsic_with_font_size_and_white_space(
        text,
        font_size_px,
        crate::types::DomWhiteSpace::Normal,
    )
}

pub(crate) fn measure_text_intrinsic_with_font_size_and_white_space(
    text: &str,
    font_size_px: f32,
    white_space: crate::types::DomWhiteSpace,
) -> TextIntrinsic {
    measure_text_intrinsic_with_font_size_line_height_white_space_and_wrap_policy(
        text,
        font_size_px,
        None,
        white_space,
        crate::types::DomOverflowWrap::Normal,
        crate::types::DomWordBreak::Normal,
    )
}

pub(crate) fn measure_text_intrinsic_with_font_size_line_height_white_space_and_wrap_policy(
    text: &str,
    font_size_px: f32,
    line_height_px: Option<f32>,
    white_space: crate::types::DomWhiteSpace,
    overflow_wrap: crate::types::DomOverflowWrap,
    word_break: crate::types::DomWordBreak,
) -> TextIntrinsic {
    let metrics = with_repo_text_layout_context(|context| {
        let mut request =
            TextLayoutRequest::for_text_with_white_space(text, font_size_px, white_space);
        request.word_break = word_break;
        request.line_height_px = line_height_px;
        request.wrap_mode =
            text_wrap_mode_for_css_intrinsic(white_space, overflow_wrap, word_break);
        context.measure_intrinsic(&request)
    });
    text_intrinsic_from_metrics(font_size_px, &metrics)
}

fn text_intrinsic_from_metrics(font_size_px: f32, metrics: &TextIntrinsicMetrics) -> TextIntrinsic {
    TextIntrinsic {
        font_size_px,
        min_content_width_px: metrics.min_content_width_px,
        max_content_width_px: metrics.max_content_width_px,
        line_height_px: metrics.line_height_px,
        leading_fragment_width_px: metrics.leading_fragment_width_px,
        trailing_fragment_width_px: metrics.trailing_fragment_width_px,
        starts_with_wrap_opportunity: metrics.starts_with_wrap_opportunity,
        ends_with_wrap_opportunity: metrics.ends_with_wrap_opportunity,
        fragment_line_height_max_px: metrics.fragment_line_height_max_px,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ui_icons::UiIconId;

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= 0.001,
            "expected {expected}, got {actual}"
        );
    }

    fn assert_vec4_close(actual: [f32; 4], expected: [f32; 4]) {
        for index in 0..4 {
            assert_close(actual[index], expected[index]);
        }
    }

    /// Routes a fixture `(name, value)` pair: presentation names go through
    /// `style_mut`, content names through `set_attr_str` (spine 3.4: the
    /// vocabulary split removed presentation attrs from `DomAttribute`).
    fn append_node_with_attrs(
        root: &mut RootDom,
        parent_key: DomNodeSlotKey,
        tag: DomTag,
        attrs: &[(&str, &str)],
    ) -> DomNodeSlotKey {
        let node_key = root.create_element(tag).expect("node");
        for (name, value) in attrs {
            if name_is_presentation(name) {
                root.style_mut(node_key)
                    .set(name, value)
                    .expect("set presentation style");
            } else {
                root.set_attr_str(node_key, name, value).expect("set attr");
            }
        }
        root.append_child(parent_key, node_key)
            .expect("append child");
        node_key
    }

    fn layout_layer_tree(
        root: &mut RootDom,
        layer_root_key: DomNodeSlotKey,
        layer_size: (f32, f32),
    ) {
        recompute_dirty_style_subtree_for_layer_root(root, layer_root_key).expect("style pass");
        let _ = relayout_tree(
            root,
            layer_root_key,
            layer_size,
            &mut IdentityDomLayoutResolver,
        )
        .expect("layout pass");
    }

    fn append_text_node(
        root: &mut RootDom,
        parent_key: DomNodeSlotKey,
        text: &str,
    ) -> DomNodeSlotKey {
        let text_key = root.create_text_node(text).expect("text node");
        root.append_child(parent_key, text_key)
            .expect("append text");
        text_key
    }

    fn build_plan(
        root: &mut RootDom,
        layer_pool: &mut LayerPool,
        layer_root_key: DomNodeSlotKey,
        layer_size: (f32, f32),
    ) -> LayerPaintPlan {
        build_layer_paint_plan(
            root,
            layer_pool,
            layer_root_key,
            layer_size,
            &InheritedStyleContext::default(),
        )
        .expect("plan")
    }

    fn x1_layer_pool() -> LayerPool {
        LayerPool::new(LayerConfig::new(PixelDensity::X1))
    }

    fn rect_ops(plan: &LayerPaintPlan) -> Vec<&RectPaintOp> {
        plan.rects.iter().collect()
    }

    fn text_ops(plan: &LayerPaintPlan) -> Vec<&TextRunPaintOp> {
        plan.text_runs.iter().collect()
    }

    fn icon_ops(plan: &LayerPaintPlan) -> Vec<&IconPaintOp> {
        plan.icons.iter().collect()
    }

    fn image_ops(plan: &LayerPaintPlan) -> Vec<&ImagePaintOp> {
        plan.images.iter().collect()
    }

    #[test]
    fn icon_img_src_emits_icon_paint_op_with_inherited_tint() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[("width", "80px"), ("height", "40px"), ("color", "#112233")],
        );
        append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Img,
            &[("src", "icon:scene"), ("width", "18px"), ("height", "18px")],
        );

        layout_layer_tree(&mut root, layer_root, (80.0, 40.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (80.0, 40.0));
        let icons = icon_ops(&plan);

        assert_eq!(icons.len(), 1);
        assert_eq!(icons[0].icon_id, UiIconId::Scene);
        assert_vec4_close(
            icons[0].color_rgba,
            [17.0 / 255.0, 34.0 / 255.0, 51.0 / 255.0, 1.0],
        );
        assert!(image_ops(&plan).is_empty());
    }

    #[test]
    fn paint_plan_uses_layer_local_coordinates_for_root_and_direct_child() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                ("position", "fixed"),
                ("left", "40px"),
                ("top", "20px"),
                ("width", "120px"),
                ("height", "80px"),
            ],
        );
        append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Div,
            &[
                ("margin-left", "12px"),
                ("margin-top", "9px"),
                ("width", "30px"),
                ("height", "18px"),
            ],
        );

        layout_layer_tree(&mut root, layer_root, (120.0, 80.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (120.0, 80.0));
        let rects = rect_ops(&plan);

        assert_eq!(rects.len(), 2);
        assert_close(rects[0].position_px[0], 0.0);
        assert_close(rects[0].position_px[1], 0.0);
        assert_close(rects[0].size_px[0], 120.0);
        assert_close(rects[0].size_px[1], 80.0);
        assert_close(rects[1].position_px[0], 12.0);
        assert_close(rects[1].position_px[1], 9.0);
        assert_close(rects[1].size_px[0], 30.0);
        assert_close(rects[1].size_px[1], 18.0);
    }

    #[test]
    fn paint_plan_accumulates_offsets_through_non_layer_ancestors() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                ("position", "fixed"),
                ("left", "0px"),
                ("top", "0px"),
                ("width", "160px"),
                ("height", "120px"),
            ],
        );
        let wrapper = append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Div,
            &[
                ("margin-left", "14px"),
                ("margin-top", "11px"),
                ("padding-left", "7px"),
                ("padding-top", "5px"),
                ("width", "50px"),
                ("height", "40px"),
            ],
        );
        append_node_with_attrs(
            &mut root,
            wrapper,
            DomTag::Div,
            &[("width", "13px"), ("height", "9px")],
        );

        layout_layer_tree(&mut root, layer_root, (160.0, 120.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (160.0, 120.0));
        let rects = rect_ops(&plan);

        assert_eq!(rects.len(), 3);
        assert_close(rects[1].position_px[0], 14.0);
        assert_close(rects[1].position_px[1], 11.0);
        assert_close(rects[2].position_px[0], 21.0);
        assert_close(rects[2].position_px[1], 16.0);
        assert_close(rects[2].size_px[0], 13.0);
        assert_close(rects[2].size_px[1], 9.0);
    }

    #[test]
    fn paint_plan_stops_before_nested_layer_subtree() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                ("position", "fixed"),
                ("left", "0px"),
                ("top", "0px"),
                ("width", "160px"),
                ("height", "120px"),
            ],
        );
        append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Div,
            &[
                ("margin-left", "4px"),
                ("margin-top", "6px"),
                ("width", "18px"),
                ("height", "10px"),
            ],
        );
        let nested_layer = append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Div,
            &[
                ("transform", "translateX(1px)"),
                ("left", "40px"),
                ("top", "30px"),
                ("width", "60px"),
                ("height", "50px"),
            ],
        );
        append_node_with_attrs(
            &mut root,
            nested_layer,
            DomTag::Div,
            &[
                ("position", "absolute"),
                ("left", "8px"),
                ("top", "9px"),
                ("width", "12px"),
                ("height", "14px"),
            ],
        );

        layout_layer_tree(&mut root, layer_root, (160.0, 120.0));
        layout_layer_tree(&mut root, nested_layer, (60.0, 50.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (160.0, 120.0));
        let rects = rect_ops(&plan);

        assert_eq!(rects.len(), 2);
        assert_close(rects[1].position_px[0], 4.0);
        assert_close(rects[1].position_px[1], 6.0);
    }

    #[test]
    fn paint_plan_owns_nested_layer_text_only_once() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                ("position", "fixed"),
                ("left", "0px"),
                ("top", "0px"),
                ("width", "160px"),
                ("height", "120px"),
                ("overflow", "scroll"),
            ],
        );
        let nested_layer = append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Div,
            &[
                ("transform", "translateX(1px)"),
                ("width", "160px"),
                ("height", "120px"),
            ],
        );
        append_text_node(
            &mut root,
            nested_layer,
            "Stage A With A Deliberately Longer First Open Menu Label",
        );

        layout_layer_tree(&mut root, layer_root, (160.0, 120.0));
        layout_layer_tree(&mut root, nested_layer, (160.0, 120.0));
        let mut layer_pool = x1_layer_pool();
        let parent_plan = build_plan(&mut root, &mut layer_pool, layer_root, (160.0, 120.0));
        let nested_plan = build_plan(&mut root, &mut layer_pool, nested_layer, (160.0, 120.0));

        assert!(
            text_ops(&parent_plan).is_empty(),
            "parent layer plan should stop before nested-layer text ops",
        );
        assert_eq!(
            text_ops(&nested_plan).len(),
            1,
            "nested layer plan should own exactly one text run for the nested text node",
        );
    }

    #[test]
    fn paint_plan_uses_text_layout_size_for_text_run_geometry() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                ("position", "fixed"),
                ("left", "0px"),
                ("top", "0px"),
                ("width", "220px"),
                ("height", "84px"),
            ],
        );
        let row = append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Button,
            &[("width", "212px"), ("height", "24px")],
        );
        let text_key = append_text_node(&mut root, row, "Stage A Label");

        layout_layer_tree(&mut root, layer_root, (220.0, 84.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (220.0, 84.0));
        let text = text_ops(&plan)
            .into_iter()
            .next()
            .expect("text run should exist");
        let text_node = root.node(text_key).expect("text node");
        let DomIntrinsic::Text(intrinsic) = text_node.intrinsic() else {
            panic!("text node should keep text intrinsic state");
        };
        let text_layout = text_node.layout();

        assert_close(text.font_size_px, intrinsic.font_size_px);
        assert_close(text.size_px[0], text_layout.width_px);
        assert_close(text.size_px[1], text_layout.height_px);
    }

    #[test]
    fn paint_plan_keeps_text_run_geometry_flat_and_separate_from_rect_ops() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                ("position", "fixed"),
                ("left", "0px"),
                ("top", "0px"),
                ("width", "220px"),
                ("height", "84px"),
            ],
        );
        let row = append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Button,
            &[("width", "212px"), ("height", "24px")],
        );
        append_text_node(&mut root, row, "Stage A Label");

        layout_layer_tree(&mut root, layer_root, (220.0, 84.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (220.0, 84.0));
        let text = text_ops(&plan)
            .into_iter()
            .next()
            .expect("text run should exist");
        let row_rect = rect_ops(&plan)
            .into_iter()
            .last()
            .expect("row rect should exist");

        assert_eq!(rect_ops(&plan).len(), 2);
        assert_close(text.position_px[0], 0.0);
        assert_close(text.position_px[1], 0.0);
        assert!(text.size_px[0] > 0.0);
        assert!(text.size_px[1] > 0.0);
        assert_close(row_rect.position_px[0], text.position_px[0]);
        assert_close(row_rect.position_px[1], text.position_px[1]);
        assert_close(row_rect.size_px[0], 212.0);
        assert_close(row_rect.size_px[1], 24.0);
        assert!(text.size_px[0] <= row_rect.size_px[0]);
        assert!(text.size_px[1] <= row_rect.size_px[1]);
        assert_vec4_close(row_rect.border_widths_px, [0.0; 4]);
        assert_vec4_close(row_rect.border_top_rgba, [0.0; 4]);
        assert_vec4_close(row_rect.border_right_rgba, [0.0; 4]);
        assert_vec4_close(row_rect.border_bottom_rgba, [0.0; 4]);
        assert_vec4_close(row_rect.border_left_rgba, [0.0; 4]);
    }

    #[test]
    fn paint_plan_emits_text_run_for_input_value() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                ("position", "fixed"),
                ("width", "220px"),
                ("height", "84px"),
            ],
        );
        append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Input,
            &[
                ("width", "120px"),
                ("height", "24px"),
                ("padding-left", "8px"),
                ("padding-right", "8px"),
                ("padding-top", "6px"),
                ("padding-bottom", "6px"),
                ("value", "hello"),
            ],
        );

        layout_layer_tree(&mut root, layer_root, (220.0, 84.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (220.0, 84.0));
        let text = text_ops(&plan)
            .into_iter()
            .next()
            .expect("input value should emit a text run");

        assert_close(text.position_px[0], 8.0);
        assert_close(text.position_px[1], 6.0);
        assert_vec4_close(
            text.color_rgba,
            [240.0 / 255.0, 240.0 / 255.0, 240.0 / 255.0, 1.0],
        );
        assert_eq!(text.wrap_mode, TextWrapMode::None);
        assert!(text.size_px[0] > 0.0);
        assert!(text.size_px[1] > 0.0);
    }

    #[test]
    fn paint_plan_emits_one_text_run_for_closed_select_label() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                ("position", "fixed"),
                ("width", "220px"),
                ("height", "84px"),
            ],
        );
        let select = append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Select,
            &[
                ("width", "140px"),
                ("height", "28px"),
                ("padding-left", "8px"),
                ("padding-right", "8px"),
                ("padding-top", "5px"),
                ("padding-bottom", "5px"),
                ("value", "cam-1"),
            ],
        );
        let first_option = append_node_with_attrs(
            &mut root,
            select,
            DomTag::Option,
            &[
                ("value", "cam-1"),
                ("label", "Face Camera"),
                ("selected", "true"),
            ],
        );
        append_text_node(&mut root, first_option, "source text should not paint");
        let second_option = append_node_with_attrs(
            &mut root,
            select,
            DomTag::Option,
            &[("value", "cam-2"), ("label", "Desk Camera")],
        );
        append_text_node(
            &mut root,
            second_option,
            "second source text should not paint",
        );

        layout_layer_tree(&mut root, layer_root, (220.0, 84.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (220.0, 84.0));
        let text_runs = text_ops(&plan);

        assert_eq!(text_runs.len(), 1);
        assert_close(text_runs[0].position_px[0], 8.0);
        assert_close(text_runs[0].position_px[1], 5.0);
        assert_eq!(text_runs[0].wrap_mode, TextWrapMode::None);
    }

    #[test]
    fn text_node_paint_plan_uses_inherited_text_align() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                ("position", "fixed"),
                ("width", "160px"),
                ("height", "80px"),
                ("text-align", "center"),
            ],
        );
        let text_parent = append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Div,
            &[("display", "block"), ("width", "160px")],
        );
        append_text_node(&mut root, text_parent, "Aligned Text");

        layout_layer_tree(&mut root, layer_root, (160.0, 80.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (160.0, 80.0));
        let text_runs = text_ops(&plan);

        assert_eq!(text_runs.len(), 1);
        assert_eq!(text_runs[0].alignment, TextAlign::Center);
    }

    #[test]
    fn zero_font_size_text_node_stages_no_text_run() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                ("position", "fixed"),
                ("width", "160px"),
                ("height", "80px"),
                ("font-size", "0"),
            ],
        );
        let text_parent = append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Div,
            &[("display", "block"), ("width", "160px")],
        );
        append_text_node(&mut root, text_parent, "zero-size text");

        layout_layer_tree(&mut root, layer_root, (160.0, 80.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (160.0, 80.0));

        assert_eq!(
            text_ops(&plan).len(),
            0,
            "zero-size text should stage no text run",
        );
    }

    #[test]
    fn stylesheet_text_node_paint_plan_uses_inherited_text_align() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        root.context()
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 0),
                ".aligned { text-align: center; }",
            )
            .expect("stylesheet");
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                ("position", "fixed"),
                ("width", "160px"),
                ("height", "80px"),
                ("class", "aligned"),
            ],
        );
        let text_parent = append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Div,
            &[("display", "block"), ("width", "160px")],
        );
        append_text_node(&mut root, text_parent, "Aligned Text");

        layout_layer_tree(&mut root, layer_root, (160.0, 80.0));
        root.style_mut(text_parent)
            .set("color", "#ffffff")
            .expect("style recompute attr");
        layout_layer_tree(&mut root, layer_root, (160.0, 80.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (160.0, 80.0));
        let text_runs = text_ops(&plan);

        assert_eq!(text_runs.len(), 1);
        assert_eq!(text_runs[0].alignment, TextAlign::Center);
    }

    #[test]
    fn stylesheet_text_node_paint_plan_maps_justify_keyword() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        root.context()
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 0),
                ".justified { text-align: justify; }",
            )
            .expect("stylesheet");
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                ("position", "fixed"),
                ("width", "160px"),
                ("height", "80px"),
                ("class", "justified"),
            ],
        );
        let text_parent = append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Div,
            &[("display", "block"), ("width", "160px")],
        );
        append_text_node(&mut root, text_parent, "Aligned Text");

        layout_layer_tree(&mut root, layer_root, (160.0, 80.0));
        root.style_mut(text_parent)
            .set("color", "#ffffff")
            .expect("style recompute attr");
        layout_layer_tree(&mut root, layer_root, (160.0, 80.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (160.0, 80.0));
        let text_runs = text_ops(&plan);

        assert_eq!(text_runs.len(), 1);
        assert_eq!(text_runs[0].alignment, TextAlign::Justified);
    }

    #[test]
    fn stylesheet_text_node_paint_plan_resolves_end_keyword_in_ltr() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        root.context()
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 0),
                ".aligned-end { text-align: end; }",
            )
            .expect("stylesheet");
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                ("position", "fixed"),
                ("width", "160px"),
                ("height", "80px"),
                ("class", "aligned-end"),
            ],
        );
        let text_parent = append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Div,
            &[("display", "block"), ("width", "160px")],
        );
        append_text_node(&mut root, text_parent, "Aligned Text");

        layout_layer_tree(&mut root, layer_root, (160.0, 80.0));
        root.style_mut(text_parent)
            .set("color", "#ffffff")
            .expect("style recompute attr");
        layout_layer_tree(&mut root, layer_root, (160.0, 80.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (160.0, 80.0));
        let text_runs = text_ops(&plan);

        assert_eq!(text_runs.len(), 1);
        assert_eq!(text_runs[0].alignment, TextAlign::Right);
    }

    #[test]
    fn text_node_paint_plan_resolves_start_keyword_against_rtl_dir_attribute() {
        // HTML `dir` should flip logical start to the physical right side.
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                ("position", "fixed"),
                ("width", "160px"),
                ("height", "80px"),
                ("dir", "rtl"),
                ("text-align", "start"),
            ],
        );
        let text_parent = append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Div,
            &[("display", "block"), ("width", "160px")],
        );
        append_text_node(&mut root, text_parent, "Aligned Text");

        layout_layer_tree(&mut root, layer_root, (160.0, 80.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (160.0, 80.0));
        let text_runs = text_ops(&plan);

        assert_eq!(text_runs.len(), 1);
        assert_eq!(text_runs[0].alignment, TextAlign::Right);
    }

    #[test]
    fn stylesheet_text_node_paint_plan_resolves_end_keyword_against_rtl_direction() {
        // CSS `direction` and `text-align:end` should resolve to the physical
        // left side in RTL, not remain a logical token at paint time.
        let mut root = RootDom::new(RootDomContext::new_gpu());
        root.context()
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 0),
                ".rtl-end { direction: rtl; text-align: end; }",
            )
            .expect("stylesheet");
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                ("position", "fixed"),
                ("width", "160px"),
                ("height", "80px"),
                ("class", "rtl-end"),
            ],
        );
        let text_parent = append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Div,
            &[("display", "block"), ("width", "160px")],
        );
        append_text_node(&mut root, text_parent, "Aligned Text");

        layout_layer_tree(&mut root, layer_root, (160.0, 80.0));
        root.style_mut(text_parent)
            .set("color", "#ffffff")
            .expect("style recompute attr");
        layout_layer_tree(&mut root, layer_root, (160.0, 80.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (160.0, 80.0));
        let text_runs = text_ops(&plan);

        assert_eq!(text_runs.len(), 1);
        assert_eq!(text_runs[0].alignment, TextAlign::Left);
    }

    #[test]
    fn select_paint_plan_inherits_text_align_for_closed_face_label() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                ("position", "fixed"),
                ("width", "220px"),
                ("height", "84px"),
                ("text-align", "right"),
            ],
        );
        let select = append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Select,
            &[
                ("width", "140px"),
                ("height", "28px"),
                ("padding-left", "8px"),
                ("padding-right", "8px"),
                ("padding-top", "5px"),
                ("padding-bottom", "5px"),
                ("value", "cam-1"),
            ],
        );
        append_node_with_attrs(
            &mut root,
            select,
            DomTag::Option,
            &[
                ("value", "cam-1"),
                ("label", "Face Camera"),
                ("selected", "true"),
            ],
        );

        layout_layer_tree(&mut root, layer_root, (220.0, 84.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (220.0, 84.0));
        let text_runs = text_ops(&plan);

        assert_eq!(text_runs.len(), 1);
        assert_eq!(text_runs[0].alignment, TextAlign::Right);
    }

    #[test]
    fn select_paint_plan_inherits_rtl_start_alignment_for_closed_face_label() {
        // Control text should use the same resolved logical alignment contract
        // as ordinary text nodes.
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                ("position", "fixed"),
                ("width", "220px"),
                ("height", "84px"),
                ("direction", "rtl"),
                ("text-align", "start"),
            ],
        );
        let select = append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Select,
            &[
                ("width", "140px"),
                ("height", "28px"),
                ("padding-left", "8px"),
                ("padding-right", "8px"),
                ("padding-top", "5px"),
                ("padding-bottom", "5px"),
                ("value", "cam-1"),
            ],
        );
        append_node_with_attrs(
            &mut root,
            select,
            DomTag::Option,
            &[
                ("value", "cam-1"),
                ("label", "Face Camera"),
                ("selected", "true"),
            ],
        );

        layout_layer_tree(&mut root, layer_root, (220.0, 84.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (220.0, 84.0));
        let text_runs = text_ops(&plan);

        assert_eq!(text_runs.len(), 1);
        assert_eq!(text_runs[0].alignment, TextAlign::Right);
    }

    #[test]
    fn text_node_paint_plan_maps_justify_keyword() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                ("position", "fixed"),
                ("width", "220px"),
                ("height", "84px"),
                ("text-align", "justify"),
            ],
        );
        let text_parent =
            append_node_with_attrs(&mut root, layer_root, DomTag::Div, &[("width", "140px")]);
        append_text_node(&mut root, text_parent, "camera one");

        layout_layer_tree(&mut root, layer_root, (220.0, 84.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (220.0, 84.0));
        let text_runs = text_ops(&plan);

        assert_eq!(text_runs.len(), 1);
        assert_eq!(text_runs[0].alignment, TextAlign::Justified);
    }

    #[test]
    fn paint_plan_emits_closed_select_label_with_auto_height() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                ("position", "fixed"),
                ("width", "220px"),
                ("height", "84px"),
            ],
        );
        let select = append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Select,
            &[
                ("width", "140px"),
                ("padding-left", "8px"),
                ("padding-right", "8px"),
                ("padding-top", "6px"),
                ("padding-bottom", "6px"),
                ("line-height", "2"),
                ("value", "cam-2"),
            ],
        );
        let first_option = append_node_with_attrs(
            &mut root,
            select,
            DomTag::Option,
            &[("value", "cam-1"), ("label", "Face Camera")],
        );
        append_text_node(&mut root, first_option, "source text should not paint");
        let second_option = append_node_with_attrs(
            &mut root,
            select,
            DomTag::Option,
            &[
                ("value", "cam-2"),
                ("label", "Desk Camera"),
                ("selected", "true"),
            ],
        );
        append_text_node(&mut root, second_option, "selected source should not paint");

        layout_layer_tree(&mut root, layer_root, (220.0, 84.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (220.0, 84.0));
        let text_runs = text_ops(&plan);

        assert_eq!(text_runs.len(), 1);
        assert_close(text_runs[0].position_px[0], 8.0);
        assert_close(text_runs[0].position_px[1], 6.0);
        assert_close(text_runs[0].size_px[1], 28.0);
        assert_close(text_runs[0].line_height_px, 28.0);
    }

    #[test]
    fn closed_select_source_options_do_not_force_repeated_layer_paint() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                ("position", "fixed"),
                ("width", "220px"),
                ("height", "84px"),
            ],
        );
        let select = append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Select,
            &[("width", "140px"), ("height", "28px"), ("value", "cam-1")],
        );
        let option = append_node_with_attrs(
            &mut root,
            select,
            DomTag::Option,
            &[
                ("value", "cam-1"),
                ("label", "Face Camera"),
                ("selected", "true"),
            ],
        );
        append_text_node(&mut root, option, "source text should not paint");

        layout_layer_tree(&mut root, layer_root, (220.0, 84.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (220.0, 84.0));

        assert_eq!(text_ops(&plan).len(), 1);
        // Plan building no longer clears dirt; the terminal clear act does.
        clear_paint_repaint_state_for_painted_layer(&mut root, layer_root)
            .expect("terminal clear should succeed");
        assert!(
            !layer_tree_needs_paint(&root, layer_root)
                .expect("layer dirty state should be readable")
        );
        root.set_attr(
            option,
            DomAttribute::Label,
            Some(DomAttributeValue::String("Front Camera".to_owned())),
        )
        .expect("label change should succeed");

        assert!(
            layer_tree_needs_paint(&root, layer_root)
                .expect("layer dirty state should be readable")
        );
    }

    #[test]
    fn paint_plan_refreshes_input_text_run_when_value_changes() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                ("position", "fixed"),
                ("width", "220px"),
                ("height", "84px"),
            ],
        );
        let input = append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Input,
            &[("width", "120px"), ("height", "24px"), ("value", "hello")],
        );

        layout_layer_tree(&mut root, layer_root, (220.0, 84.0));
        let mut layer_pool = x1_layer_pool();
        let first_plan = build_plan(&mut root, &mut layer_pool, layer_root, (220.0, 84.0));
        let first_text_run_id = text_ops(&first_plan)
            .into_iter()
            .next()
            .expect("first input value should emit a text run")
            .text_run_id;

        root.set_attr_str(input, "value", "server")
            .expect("input value should update");
        let second_plan = build_plan(&mut root, &mut layer_pool, layer_root, (220.0, 84.0));
        let second_text_run_id = text_ops(&second_plan)
            .into_iter()
            .next()
            .expect("updated input value should emit a text run")
            .text_run_id;

        assert_ne!(first_text_run_id, second_text_run_id);
    }

    #[test]
    fn paint_plan_adds_focus_outline_for_focused_input() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                ("position", "fixed"),
                ("width", "220px"),
                ("height", "84px"),
            ],
        );
        let input = append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Input,
            &[("width", "120px"), ("height", "24px"), ("value", "hello")],
        );
        root.focus(input).expect("focus input");

        layout_layer_tree(&mut root, layer_root, (220.0, 84.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (220.0, 84.0));
        let outline_rects = rect_ops(&plan)
            .into_iter()
            .filter(|rect| rect.border_widths_px[0] > 0.0)
            .collect::<Vec<_>>();

        assert_eq!(outline_rects.len(), 1);
        assert_vec4_close(outline_rects[0].border_widths_px, [2.0; 4]);
    }

    #[test]
    fn paint_plan_skips_button_outline_without_focus_visible() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                ("position", "fixed"),
                ("width", "220px"),
                ("height", "84px"),
            ],
        );
        let button = append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Button,
            &[("width", "120px"), ("height", "24px")],
        );
        root.focus(button).expect("focus button");

        layout_layer_tree(&mut root, layer_root, (220.0, 84.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (220.0, 84.0));
        let outline_rects = rect_ops(&plan)
            .into_iter()
            .filter(|rect| rect.border_widths_px[0] > 0.0)
            .collect::<Vec<_>>();

        assert!(outline_rects.is_empty());
    }

    #[test]
    fn paint_plan_uses_em_outline_width_from_authored_focus_visible_rule() {
        let context = RootDomContext::new_gpu();
        let mut root = RootDom::new(context.clone());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                ("position", "fixed"),
                ("width", "220px"),
                ("height", "84px"),
            ],
        );
        let input = append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Input,
            &[("width", "120px"), ("height", "24px")],
        );
        let button = append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Button,
            &[("width", "120px"), ("height", "24px"), ("class", "primary")],
        );
        context
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 0),
                ".primary:focus-visible { font-size: 10px; outline-color: #ff3366; outline-width: 0.5em; }",
            )
            .expect("outline stylesheet");
        root.focus(input).expect("focus input");
        root.focus(button).expect("focus button");

        layout_layer_tree(&mut root, layer_root, (220.0, 84.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (220.0, 84.0));
        let outline_rect = rect_ops(&plan)
            .into_iter()
            .find(|rect| rect.border_widths_px[0] > 0.0)
            .expect("outline rect should exist");

        assert_vec4_close(outline_rect.border_widths_px, [5.0; 4]);
    }

    #[test]
    fn paint_plan_adds_caret_rect_for_collapsed_input_selection() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                ("position", "fixed"),
                ("width", "220px"),
                ("height", "84px"),
            ],
        );
        let input = append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Input,
            &[("width", "120px"), ("height", "24px"), ("value", "hello")],
        );
        root.focus(input).expect("focus input");

        layout_layer_tree(&mut root, layer_root, (220.0, 84.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (220.0, 84.0));
        let caret_rects = rect_ops(&plan)
            .into_iter()
            .filter(|rect| (rect.size_px[0] - 1.0).abs() <= 0.001 && rect.size_px[1] > 0.0)
            .collect::<Vec<_>>();

        assert_eq!(caret_rects.len(), 1);
    }

    #[test]
    fn paint_plan_uses_authored_caret_color_for_focused_input() {
        let context = RootDomContext::new_gpu();
        let mut root = RootDom::new(context.clone());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                ("position", "fixed"),
                ("width", "220px"),
                ("height", "84px"),
            ],
        );
        let input = append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Input,
            &[
                ("width", "120px"),
                ("height", "24px"),
                ("value", "hello"),
                ("class", "accent"),
            ],
        );
        context
            .load_stylesheet(
                StyleRuleOrigin::new("theme", 0),
                ".accent:focus { caret-color: #ff3366; }",
            )
            .expect("caret stylesheet");
        root.focus(input).expect("focus input");

        layout_layer_tree(&mut root, layer_root, (220.0, 84.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (220.0, 84.0));
        let caret_rect = rect_ops(&plan)
            .into_iter()
            .find(|rect| (rect.size_px[0] - 1.0).abs() <= 0.001 && rect.size_px[1] > 0.0)
            .expect("caret rect should exist");

        assert_vec4_close(caret_rect.fill_rgba, [1.0, 0.2, 0.4, 1.0]);
    }

    #[test]
    fn paint_plan_hides_caret_for_extended_input_selection() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                ("position", "fixed"),
                ("width", "220px"),
                ("height", "84px"),
            ],
        );
        let input = append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Input,
            &[("width", "120px"), ("height", "24px"), ("value", "hello")],
        );
        root.focus(input).expect("focus input");
        let arrow_left = bevy::input::keyboard::KeyboardInput {
            key_code: bevy::input::keyboard::KeyCode::ArrowLeft,
            logical_key: bevy::input::keyboard::Key::ArrowLeft,
            state: bevy::input::ButtonState::Pressed,
            text: None,
            repeat: false,
            window: bevy::prelude::Entity::PLACEHOLDER,
        };
        assert!(
            root.apply_default_text_behavior_and_dispatch_input_from_keyboard_input(
                &arrow_left,
                true,
            )
            .expect("shift+arrow left should extend"),
        );

        layout_layer_tree(&mut root, layer_root, (220.0, 84.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (220.0, 84.0));
        let caret_rects = rect_ops(&plan)
            .into_iter()
            .filter(|rect| (rect.size_px[0] - 1.0).abs() <= 0.001 && rect.size_px[1] > 0.0)
            .collect::<Vec<_>>();

        assert!(caret_rects.is_empty());
    }

    #[test]
    fn paint_plan_resolves_text_run_hints_through_layer_pool() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let mut layer_pool = x1_layer_pool();
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                ("position", "fixed"),
                ("left", "0px"),
                ("top", "0px"),
                ("width", "220px"),
                ("height", "84px"),
            ],
        );
        let row = append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Button,
            &[("width", "212px"), ("height", "24px")],
        );
        let text_key = append_text_node(&mut root, row, "Stage A Label");
        layout_layer_tree(&mut root, layer_root, (220.0, 84.0));
        let first_plan = build_plan(&mut root, &mut layer_pool, layer_root, (220.0, 84.0));
        let first_text_run_id = text_ops(&first_plan)
            .into_iter()
            .next()
            .expect("text run should exist")
            .text_run_id;
        assert_eq!(
            root.node(text_key)
                .and_then(DomNode::as_text)
                .and_then(DomTextNode::text_run_id_hint),
            Some(first_text_run_id)
        );

        root.set_text_content(text_key, "Stage B Label")
            .expect("text content should update");
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (220.0, 84.0));
        let text = text_ops(&plan)
            .into_iter()
            .next()
            .expect("text run should exist");

        assert_ne!(text.text_run_id, first_text_run_id);
        assert_eq!(
            root.node(text_key)
                .and_then(DomNode::as_text)
                .and_then(DomTextNode::text_run_id_hint),
            Some(text.text_run_id)
        );
    }

    #[test]
    fn paint_plan_reuses_identical_text_across_roots() {
        let mut shared_layer_pool = x1_layer_pool();

        let mut first_root = RootDom::new(RootDomContext::new_gpu());
        let first_document_key = first_root.root;
        let first_layer_root = append_node_with_attrs(
            &mut first_root,
            first_document_key,
            DomTag::Div,
            &[
                ("position", "fixed"),
                ("width", "220px"),
                ("height", "84px"),
            ],
        );
        let first_row = append_node_with_attrs(
            &mut first_root,
            first_layer_root,
            DomTag::Button,
            &[("width", "212px"), ("height", "24px")],
        );
        append_text_node(&mut first_root, first_row, "Window Control");
        layout_layer_tree(&mut first_root, first_layer_root, (220.0, 84.0));
        let first_plan = build_plan(
            &mut first_root,
            &mut shared_layer_pool,
            first_layer_root,
            (220.0, 84.0),
        );

        let mut second_root = RootDom::new(RootDomContext::new_gpu());
        let second_document_key = second_root.root;
        let second_layer_root = append_node_with_attrs(
            &mut second_root,
            second_document_key,
            DomTag::Div,
            &[
                ("position", "fixed"),
                ("width", "220px"),
                ("height", "84px"),
            ],
        );
        let second_row = append_node_with_attrs(
            &mut second_root,
            second_layer_root,
            DomTag::Button,
            &[("width", "212px"), ("height", "24px")],
        );
        append_text_node(&mut second_root, second_row, "Window Control");
        layout_layer_tree(&mut second_root, second_layer_root, (220.0, 84.0));
        let second_plan = build_plan(
            &mut second_root,
            &mut shared_layer_pool,
            second_layer_root,
            (220.0, 84.0),
        );

        assert_eq!(
            text_ops(&first_plan)
                .into_iter()
                .next()
                .expect("first text run")
                .text_run_id,
            text_ops(&second_plan)
                .into_iter()
                .next()
                .expect("second text run")
                .text_run_id
        );
    }

    #[test]
    fn paint_plan_keeps_distinct_graph_titles_separate_across_roots() {
        let mut shared_layer_pool = x1_layer_pool();

        let mut first_root = RootDom::new(RootDomContext::new_gpu());
        let first_document_key = first_root.root;
        let first_layer_root = append_node_with_attrs(
            &mut first_root,
            first_document_key,
            DomTag::Div,
            &[
                ("position", "fixed"),
                ("width", "220px"),
                ("height", "84px"),
            ],
        );
        let first_row = append_node_with_attrs(
            &mut first_root,
            first_layer_root,
            DomTag::Button,
            &[("width", "212px"), ("height", "24px")],
        );
        append_text_node(&mut first_root, first_row, "Window Control");
        layout_layer_tree(&mut first_root, first_layer_root, (220.0, 84.0));
        let first_plan = build_plan(
            &mut first_root,
            &mut shared_layer_pool,
            first_layer_root,
            (220.0, 84.0),
        );

        let mut second_root = RootDom::new(RootDomContext::new_gpu());
        let second_document_key = second_root.root;
        let second_layer_root = append_node_with_attrs(
            &mut second_root,
            second_document_key,
            DomTag::Div,
            &[
                ("position", "fixed"),
                ("width", "220px"),
                ("height", "84px"),
            ],
        );
        let second_row = append_node_with_attrs(
            &mut second_root,
            second_layer_root,
            DomTag::Button,
            &[("width", "212px"), ("height", "24px")],
        );
        append_text_node(&mut second_root, second_row, "Window Texture Plane");
        layout_layer_tree(&mut second_root, second_layer_root, (220.0, 84.0));
        let second_plan = build_plan(
            &mut second_root,
            &mut shared_layer_pool,
            second_layer_root,
            (220.0, 84.0),
        );

        assert_ne!(
            text_ops(&first_plan)
                .into_iter()
                .next()
                .expect("first text run")
                .text_run_id,
            text_ops(&second_plan)
                .into_iter()
                .next()
                .expect("second text run")
                .text_run_id
        );
    }

    #[test]
    fn paint_plan_uses_layer_local_coordinates_for_image_geometry() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                ("position", "fixed"),
                ("width", "160px"),
                ("height", "120px"),
            ],
        );
        let image_key = append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Img,
            &[
                ("margin-left", "9px"),
                ("margin-top", "7px"),
                ("width", "24px"),
                ("height", "16px"),
                ("src", "avatar.png"),
            ],
        );

        layout_layer_tree(&mut root, layer_root, (160.0, 120.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (160.0, 120.0));
        let image = image_ops(&plan)
            .into_iter()
            .next()
            .expect("image op should exist");
        let layout_box = node_box_from_ref(root.node(image_key).expect("image node")).border_box;

        assert_close(image.position_px[0], layout_box.left_px);
        assert_close(image.position_px[1], layout_box.top_px);
        assert_close(image.size_px[0], layout_box.width_px);
        assert_close(image.size_px[1], layout_box.height_px);
        assert_eq!(image.image_id, ImagePaintId::from_name("avatar.png"));
    }

    #[test]
    fn paint_plan_uses_transparent_fill_and_border_fallbacks() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                ("position", "fixed"),
                ("width", "48px"),
                ("height", "32px"),
                ("border-top", "3px"),
                ("border-right", "2px"),
                ("border-bottom", "1px"),
                ("border-left", "4px"),
            ],
        );

        layout_layer_tree(&mut root, layer_root, (48.0, 32.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (48.0, 32.0));
        let rects = rect_ops(&plan);

        assert_eq!(rects.len(), 1);
        assert_vec4_close(rects[0].fill_rgba, [0.0; 4]);
        assert_vec4_close(rects[0].border_top_rgba, [0.0; 4]);
        assert_vec4_close(rects[0].border_right_rgba, [0.0; 4]);
        assert_vec4_close(rects[0].border_bottom_rgba, [0.0; 4]);
        assert_vec4_close(rects[0].border_left_rgba, [0.0; 4]);
        assert_vec4_close(rects[0].border_widths_px, [3.0, 2.0, 1.0, 4.0]);
    }

    #[test]
    fn paint_plan_lowers_per_side_border_colors_and_four_corner_radii() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                ("position", "fixed"),
                ("width", "48px"),
                ("height", "32px"),
                ("border-top", "3px"),
                ("border-right", "2px"),
                ("border-bottom", "1px"),
                ("border-left", "4px"),
            ],
        );
        root.style_mut(layer_root)
            .set("background-color", "#11223344")
            .expect("background-color");
        root.style_mut(layer_root)
            .set("border-top-color", "#ff0000")
            .expect("border-top-color");
        root.style_mut(layer_root)
            .set("border-right-color", "#00ff00")
            .expect("border-right-color");
        root.style_mut(layer_root)
            .set("border-bottom-color", "#0000ff")
            .expect("border-bottom-color");
        root.style_mut(layer_root)
            .set("border-left-color", "#ffff00")
            .expect("border-left-color");
        root.style_mut(layer_root)
            .set("border-radius", "3px 4px 5px 6px")
            .expect("border-radius");

        layout_layer_tree(&mut root, layer_root, (48.0, 32.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (48.0, 32.0));
        let rects = rect_ops(&plan);

        assert_eq!(rects.len(), 1);
        assert_vec4_close(
            rects[0].fill_rgba,
            [17.0 / 255.0, 34.0 / 255.0, 51.0 / 255.0, 68.0 / 255.0],
        );
        assert_vec4_close(rects[0].corner_radii_px, [3.0, 4.0, 5.0, 6.0]);
        assert_vec4_close(rects[0].border_top_rgba, [1.0, 0.0, 0.0, 1.0]);
        assert_vec4_close(rects[0].border_right_rgba, [0.0, 1.0, 0.0, 1.0]);
        assert_vec4_close(rects[0].border_bottom_rgba, [0.0, 0.0, 1.0, 1.0]);
        assert_vec4_close(rects[0].border_left_rgba, [1.0, 1.0, 0.0, 1.0]);
    }

    #[test]
    fn paint_plan_uses_uniform_border_color_when_side_colors_are_absent() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                ("position", "fixed"),
                ("width", "48px"),
                ("height", "32px"),
                ("border-top", "3px"),
                ("border-right", "2px"),
                ("border-bottom", "1px"),
                ("border-left", "4px"),
            ],
        );
        root.style_mut(layer_root)
            .set("border-color", "#abcdef")
            .expect("border-color");

        layout_layer_tree(&mut root, layer_root, (48.0, 32.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (48.0, 32.0));
        let rects = rect_ops(&plan);

        assert_eq!(rects.len(), 1);
        assert_vec4_close(
            rects[0].border_top_rgba,
            [171.0 / 255.0, 205.0 / 255.0, 239.0 / 255.0, 1.0],
        );
        assert_vec4_close(rects[0].border_top_rgba, rects[0].border_right_rgba);
        assert_vec4_close(rects[0].border_top_rgba, rects[0].border_bottom_rgba);
        assert_vec4_close(rects[0].border_top_rgba, rects[0].border_left_rgba);
    }
}
