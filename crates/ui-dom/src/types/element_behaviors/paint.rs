//! Specialized paint behavior for DOM elements.

use std::sync::Arc;

use ui_icons::UiIconId;

use crate::domains::control_widgets::{
    CheckboxMarkSpec, RangeBarSpec, checkbox_mark_ops, range_bar_ops,
};
use crate::domains::layer_ops::{
    IconPaintOp, ImagePaintId, LayerPaintPlan, RectPaintOp, TextRunPaintOp, TextRunSignature,
    TextWrapMode,
};
use crate::domains::layer_pool::LayerPool;
use crate::domains::text_layout::{
    DEFAULT_TEXT_FONT_SIZE_PX, TextLayoutRequest, text_align_for_css_value, text_caret_layout,
    with_repo_text_layout_context,
};
use crate::element_controls::{
    ResolvedElementControl, ResolvedRangeControl, checkbox_indicator_rect, number_editor_box_model,
    number_stepper_rects, range_thumb_rect, range_track_rect, resolved_element_control,
};
use crate::types::{
    AuthoredLength, DomAttribute, DomAttributeValue, DomBoxModel, DomElementEditKind,
    DomElementNode, DomError, DomNode, DomNodeSlotKey, DomSelection, DomTag, DomTextAlign,
    DomTextDirection, RootDom, TextSelectionDirection, parse_authored_length,
    resolve_line_height_px, text_align_from_attr_value, text_direction_from_attr_value,
};

use super::present_options::select_face_label_for_owner_safe;

pub(crate) struct ElementPaintContext<'a> {
    pub current_key: DomNodeSlotKey,
    pub root_dom: &'a RootDom,
    pub node: &'a DomNode,
    pub element: &'a DomElementNode,
    pub box_model: DomBoxModel,
    pub inherited_color_rgba: [f32; 4],
    pub inherited_text_align: DomTextAlign,
    pub inherited_text_direction: DomTextDirection,
    pub layer_pool: &'a mut LayerPool,
    pub plan: &'a mut LayerPaintPlan,
}

pub(crate) fn append_specialized_element_paint_ops(
    context: &mut ElementPaintContext<'_>,
) -> Result<(), DomError> {
    match context.element.tag {
        DomTag::Img => append_img_paint_ops(context),
        DomTag::Input => append_input_paint_ops(context),
        DomTag::Textarea => {
            append_textarea_paint_ops(context);
            Ok(())
        }
        DomTag::Select => append_select_paint_ops(context),
        DomTag::Button
        | DomTag::Datalist
        | DomTag::Custom(_)
        | DomTag::Div
        | DomTag::Li
        | DomTag::Option
        | DomTag::Span
        | DomTag::Ul => Ok(()),
    }
}

fn effective_font_size_px(node: &DomNode, inherited_font_size_px: f32) -> f32 {
    match node.presented_attr(&DomAttribute::FontSize) {
        Some(DomAttributeValue::Float(value)) => *value,
        Some(DomAttributeValue::String(value)) => match parse_authored_length(value) {
            Some(AuthoredLength::Px(value_px)) => value_px,
            Some(AuthoredLength::Em(scale)) => inherited_font_size_px * scale,
            Some(AuthoredLength::Unset) | None => DEFAULT_TEXT_FONT_SIZE_PX,
        },
        _ => inherited_font_size_px,
    }
}

fn effective_line_height_px(node: &DomNode, font_size_px: f32) -> Option<f32> {
    node.presented_attr(&DomAttribute::LineHeight)
        .and_then(|value| resolve_line_height_px(value, font_size_px))
}

fn effective_text_direction(context: &ElementPaintContext<'_>) -> DomTextDirection {
    let Some(value) = context
        .node
        .presented_attr(&DomAttribute::Direction)
        .or_else(|| context.node.presented_attr(&DomAttribute::Dir))
    else {
        return context.inherited_text_direction;
    };
    text_direction_from_attr_value(value)
        .unwrap_or_else(|_| panic!("paint text direction should stay valid after staging"))
}

fn effective_text_align(context: &ElementPaintContext<'_>) -> crate::domains::layer_ops::TextAlign {
    let text_direction = effective_text_direction(context);
    let Some(value) = context.node.presented_attr(&DomAttribute::TextAlign) else {
        return text_align_for_css_value(context.inherited_text_align, text_direction);
    };
    let text_align = text_align_from_attr_value(value)
        .unwrap_or_else(|_| panic!("paint text alignment should stay valid after staging"));
    text_align_for_css_value(text_align, text_direction)
}

fn select_text_line_height_px(node: &DomNode, font_size_px: f32, content_height_px: f32) -> f32 {
    effective_line_height_px(node, font_size_px).unwrap_or(content_height_px)
}

fn custom_color_attr_rgba(node: &DomNode, attribute_name: &str) -> Option<[f32; 4]> {
    match node.presented_attr(&DomAttribute::Custom(attribute_name.to_string()))? {
        DomAttributeValue::Color(rgba) => Some(*rgba),
        _ => None,
    }
}

fn resolved_node_color_rgba(node: &DomNode, inherited_color_rgba: [f32; 4]) -> [f32; 4] {
    match node.presented_attr(&DomAttribute::Custom("color".to_string())) {
        Some(DomAttributeValue::Color(rgba)) => *rgba,
        _ => inherited_color_rgba,
    }
}

fn append_img_paint_ops(context: &mut ElementPaintContext<'_>) -> Result<(), DomError> {
    let rect = context.box_model.border_box;
    let position_px = [rect.left_px, rect.top_px];
    let size_px = [rect.width_px.max(0.0), rect.height_px.max(0.0)];
    let Some(image_name) = string_attr_for_attribute(context.element, &DomAttribute::Src) else {
        return Ok(());
    };
    match context.element.lowered_image_source_kind() {
        Some(crate::types::dom_node::LoweredImageSourceKind::InternalIcon(icon_id)) => {
            context.plan.icons.push(IconPaintOp {
                position_px,
                size_px,
                icon_id,
                color_rgba: resolved_node_color_rgba(context.node, context.inherited_color_rgba),
            });
            Ok(())
        }
        Some(crate::types::dom_node::LoweredImageSourceKind::External) => {
            context
                .plan
                .images
                .push(crate::domains::layer_ops::ImagePaintOp {
                    position_px,
                    size_px,
                    image_id: ImagePaintId::from_name(image_name),
                });
            Ok(())
        }
        None => Err(DomError::InvalidAttributeValue),
    }
}

fn append_input_paint_ops(context: &mut ElementPaintContext<'_>) -> Result<(), DomError> {
    let Some(control) = resolved_element_control(context.element) else {
        return Ok(());
    };
    match control {
        ResolvedElementControl::Checkbox(control) => {
            append_checkbox_input_paint_ops(context, control.checked)?;
        }
        ResolvedElementControl::Number(_) => {
            append_number_input_chrome(context);
            append_text_entry_caret_rect(context, DomElementEditKind::SingleLine);
            append_text_entry_value_text_run(context, DomElementEditKind::SingleLine);
        }
        ResolvedElementControl::Range(control) => append_range_input_chrome(context, control)?,
        ResolvedElementControl::TextInput => {
            append_text_entry_caret_rect(context, DomElementEditKind::SingleLine);
            append_text_entry_value_text_run(context, DomElementEditKind::SingleLine);
        }
        ResolvedElementControl::Textarea => panic!("input paint should not resolve textarea"),
    }
    Ok(())
}

fn append_textarea_paint_ops(context: &mut ElementPaintContext<'_>) {
    append_text_entry_caret_rect(context, DomElementEditKind::MultiLine);
    append_text_entry_value_text_run(context, DomElementEditKind::MultiLine);
}

fn append_select_paint_ops(context: &mut ElementPaintContext<'_>) -> Result<(), DomError> {
    let label = select_face_label_for_owner_safe(context.root_dom, context.current_key)?;
    append_select_text_run(context, label.as_str());
    Ok(())
}

fn append_select_text_run(context: &mut ElementPaintContext<'_>, value: &str) {
    if value.is_empty() {
        return;
    }
    let paint_box = text_entry_paint_box(context.box_model);
    if paint_box.content_width <= 0.0 || paint_box.content_height <= 0.0 {
        return;
    }
    let font_size_px = effective_font_size_px(context.node, DEFAULT_TEXT_FONT_SIZE_PX);
    let line_height_px =
        select_text_line_height_px(context.node, font_size_px, paint_box.content_height);
    let text_height_px = line_height_px.max(0.0);
    if text_height_px <= 0.0 {
        return;
    }
    let mut request = TextLayoutRequest::for_text(value, font_size_px);
    request.width_px = Some(paint_box.content_width);
    request.line_height_px = Some(line_height_px);
    request.wrap_mode = TextWrapMode::None;
    request.alignment = effective_text_align(context);
    let origin_top =
        paint_box.origin_top + ((paint_box.content_height - text_height_px).max(0.0) * 0.5);
    let text_run_id = context
        .layer_pool
        .resolve_text_run_id_from_hint(&TextRunSignature::from_text(Arc::<str>::from(value)), None);
    context.plan.text_runs.push(TextRunPaintOp {
        text_run_id,
        position_px: [paint_box.origin_left, origin_top],
        size_px: [paint_box.content_width, text_height_px],
        font_size_px,
        line_height_px,
        wrap_mode: request.wrap_mode,
        alignment: request.alignment,
        color_rgba: resolved_node_color_rgba(context.node, context.inherited_color_rgba),
    });
}

fn append_text_entry_value_text_run(
    context: &mut ElementPaintContext<'_>,
    edit_kind: DomElementEditKind,
) {
    let Some(value) = string_attr_for_attribute(context.element, &DomAttribute::Value) else {
        return;
    };
    append_control_text_run(context, value, edit_kind);
}

fn append_control_text_run(
    context: &mut ElementPaintContext<'_>,
    value: &str,
    edit_kind: DomElementEditKind,
) {
    if value.is_empty() {
        return;
    }
    let box_model = text_entry_paint_box_model(context.element, context.box_model);
    let paint_box = text_entry_paint_box(box_model);
    if paint_box.content_width <= 0.0 || paint_box.content_height <= 0.0 {
        return;
    }
    let font_size_px = effective_font_size_px(context.node, DEFAULT_TEXT_FONT_SIZE_PX);
    let mut request = TextLayoutRequest::for_text(value, font_size_px);
    request.width_px = Some(paint_box.content_width);
    request.line_height_px = effective_line_height_px(context.node, font_size_px);
    if matches!(edit_kind, DomElementEditKind::SingleLine) {
        request.wrap_mode = TextWrapMode::None;
    }
    request.alignment = effective_text_align(context);
    let text_height_px = resolved_text_run_height_px(&request, edit_kind, box_model);
    if text_height_px <= 0.0 {
        return;
    }
    let text_run_id = context
        .layer_pool
        .resolve_text_run_id_from_hint(&TextRunSignature::from_text(Arc::<str>::from(value)), None);
    context.plan.text_runs.push(TextRunPaintOp {
        text_run_id,
        position_px: [paint_box.origin_left, paint_box.origin_top],
        size_px: [paint_box.content_width, text_height_px],
        font_size_px,
        line_height_px: request.line_height_px.unwrap_or(font_size_px),
        wrap_mode: request.wrap_mode,
        alignment: request.alignment,
        color_rgba: resolved_node_color_rgba(context.node, context.inherited_color_rgba),
    });
}

fn append_text_entry_caret_rect(
    context: &mut ElementPaintContext<'_>,
    edit_kind: DomElementEditKind,
) {
    if context.root_dom.focused_element_key() != Some(context.current_key) {
        return;
    }
    if matches!(
        context
            .element
            .attrs
            .attributes
            .get(&DomAttribute::Disabled),
        Some(DomAttributeValue::Bool(true))
    ) {
        return;
    }
    let DomSelection::TextSelected(selection) = context.root_dom.selection() else {
        return;
    };
    if selection.target_element != context.current_key
        || selection.selection_start != selection.selection_end
        || selection.selection_direction != TextSelectionDirection::None
    {
        return;
    }
    let value = string_attr_for_attribute(context.element, &DomAttribute::Value).unwrap_or("");
    let box_model = text_entry_paint_box_model(context.element, context.box_model);
    let paint_box = text_entry_paint_box(box_model);
    let font_size_px = effective_font_size_px(context.node, DEFAULT_TEXT_FONT_SIZE_PX);
    let mut request = TextLayoutRequest::for_text(value, font_size_px);
    request.width_px = if matches!(edit_kind, DomElementEditKind::MultiLine) {
        Some(paint_box.content_width)
    } else {
        None
    };
    request.line_height_px = effective_line_height_px(context.node, font_size_px);
    if matches!(edit_kind, DomElementEditKind::SingleLine) {
        request.wrap_mode = TextWrapMode::None;
    }
    request.alignment = effective_text_align(context);
    let caret = text_caret_layout(&request, selection.selection_start);
    let caret_color_rgba = custom_color_attr_rgba(context.node, "caret-color")
        .unwrap_or_else(|| resolved_node_color_rgba(context.node, context.inherited_color_rgba));
    let caret_top_px = if matches!(edit_kind, DomElementEditKind::SingleLine) {
        paint_box.origin_top + ((paint_box.content_height - caret.height_px).max(0.0) / 2.0)
    } else {
        paint_box.origin_top + caret.y_px
    };
    context.plan.rects.push(RectPaintOp {
        position_px: [paint_box.origin_left + caret.x_px, caret_top_px],
        size_px: [1.0, caret.height_px.max(0.0)],
        fill_rgba: caret_color_rgba,
        corner_radii_px: [0.0; 4],
        border_widths_px: [0.0; 4],
        border_top_rgba: [0.0; 4],
        border_right_rgba: [0.0; 4],
        border_bottom_rgba: [0.0; 4],
        border_left_rgba: [0.0; 4],
    });
}

fn append_checkbox_input_paint_ops(
    context: &mut ElementPaintContext<'_>,
    checked: bool,
) -> Result<(), DomError> {
    let indicator_rect = checkbox_indicator_rect(context.box_model);
    let inset_px = (indicator_rect.width_px * 0.25).max(2.0);
    let mark_rect = checked.then_some(crate::types::DomRect {
        left_px: indicator_rect.left_px + inset_px,
        top_px: indicator_rect.top_px + inset_px,
        width_px: (indicator_rect.width_px - inset_px * 2.0).max(0.0),
        height_px: (indicator_rect.height_px - inset_px * 2.0).max(0.0),
    });
    let color_rgba = resolved_node_color_rgba(context.node, context.inherited_color_rgba);
    let background_rgba =
        custom_color_attr_rgba(context.node, "background-color").unwrap_or([0.0; 4]);
    let border_widths_px = nonzero_border_widths_or_default(context.box_model, [1.5; 4]);
    let border_colors = input_border_colors_or_default(context.node, color_rgba);
    checkbox_mark_ops(CheckboxMarkSpec {
        outer_rect: indicator_rect,
        mark_rect,
        background_rgba,
        mark_rgba: color_rgba,
        border_widths_px,
        border_top_rgba: border_colors[0],
        border_right_rgba: border_colors[1],
        border_bottom_rgba: border_colors[2],
        border_left_rgba: border_colors[3],
        corner_radii_px: crate::domains::paint::border_radii_px(
            context.root_dom,
            context.current_key,
        )?,
        mark_corner_radii_px: [2.0; 4],
    })
    .append_to_plan(context.plan);
    Ok(())
}

fn append_number_input_chrome(context: &mut ElementPaintContext<'_>) {
    let (decrement_rect, increment_rect) = number_stepper_rects(context.box_model);
    for rect in [decrement_rect, increment_rect] {
        context.plan.rects.push(RectPaintOp {
            position_px: [rect.left_px, rect.top_px],
            size_px: [rect.width_px, rect.height_px],
            fill_rgba: [0.08, 0.10, 0.13, 1.0],
            corner_radii_px: [0.0; 4],
            border_widths_px: [1.0; 4],
            border_top_rgba: [0.16, 0.19, 0.24, 1.0],
            border_right_rgba: [0.16, 0.19, 0.24, 1.0],
            border_bottom_rgba: [0.16, 0.19, 0.24, 1.0],
            border_left_rgba: [0.16, 0.19, 0.24, 1.0],
        });
    }
    let icon_color_rgba = resolved_node_color_rgba(context.node, context.inherited_color_rgba);
    append_control_icon(context, decrement_rect, UiIconId::Minus, icon_color_rgba);
    append_control_icon(context, increment_rect, UiIconId::Plus, icon_color_rgba);
}

fn append_range_input_chrome(
    context: &mut ElementPaintContext<'_>,
    control: ResolvedRangeControl,
) -> Result<(), DomError> {
    let track_rect = range_track_rect(context.box_model);
    let thumb_rect = range_thumb_rect(context.box_model, control);
    let filled_track_rect = range_filled_track_rect(track_rect, control);
    let color_rgba = resolved_node_color_rgba(context.node, context.inherited_color_rgba);
    let base_track_rgba = custom_color_attr_rgba(context.node, "background-color").unwrap_or([
        color_rgba[0],
        color_rgba[1],
        color_rgba[2],
        0.25,
    ]);
    let authored_corner_radii_px =
        crate::domains::paint::border_radii_px(context.root_dom, context.current_key)?;
    let has_authored_border_radius = authored_border_radius_is_present(context.node);
    range_bar_ops(RangeBarSpec {
        base_track_rect: track_rect,
        filled_track_rect,
        thumb_rect,
        base_track_rgba,
        filled_track_rgba: color_rgba,
        thumb_rgba: color_rgba,
        track_corner_radii_px: range_track_corner_radii_px(
            has_authored_border_radius,
            track_rect,
            authored_corner_radii_px,
        ),
        thumb_corner_radii_px: range_thumb_corner_radii_px(
            has_authored_border_radius,
            thumb_rect,
            authored_corner_radii_px,
        ),
    })
    .append_to_plan(context.plan);
    Ok(())
}

fn text_entry_paint_box_model(element: &DomElementNode, box_model: DomBoxModel) -> DomBoxModel {
    match resolved_element_control(element) {
        Some(ResolvedElementControl::Number(_)) => number_editor_box_model(box_model),
        _ => box_model,
    }
}

fn nonzero_border_widths_or_default(
    box_model: DomBoxModel,
    default_widths_px: [f32; 4],
) -> [f32; 4] {
    let border_widths_px = [
        box_model.border.top_px,
        box_model.border.right_px,
        box_model.border.bottom_px,
        box_model.border.left_px,
    ];
    if border_widths_px.iter().any(|width_px| *width_px > 0.0) {
        return border_widths_px;
    }
    default_widths_px
}

fn input_border_colors_or_default(node: &DomNode, default_rgba: [f32; 4]) -> [[f32; 4]; 4] {
    [
        input_border_side_color_or_default(node, "border-top-color", default_rgba),
        input_border_side_color_or_default(node, "border-right-color", default_rgba),
        input_border_side_color_or_default(node, "border-bottom-color", default_rgba),
        input_border_side_color_or_default(node, "border-left-color", default_rgba),
    ]
}

fn input_border_side_color_or_default(
    node: &DomNode,
    side_name: &str,
    default_rgba: [f32; 4],
) -> [f32; 4] {
    custom_color_attr_rgba(node, side_name)
        .or_else(|| custom_color_attr_rgba(node, "border-color"))
        .unwrap_or(default_rgba)
}

fn range_filled_track_rect(
    track_rect: crate::types::DomRect,
    control: ResolvedRangeControl,
) -> crate::types::DomRect {
    crate::types::DomRect {
        width_px: track_rect.width_px * control.numeric.value_fraction(),
        ..track_rect
    }
}

fn authored_border_radius_is_present(node: &DomNode) -> bool {
    node.presented_attr(&DomAttribute::Custom("border-radius".to_string()))
        .is_some()
}

fn range_track_corner_radii_px(
    authored_border_radius_is_present: bool,
    track_rect: crate::types::DomRect,
    authored_corner_radii_px: [f32; 4],
) -> [f32; 4] {
    if authored_border_radius_is_present {
        return authored_corner_radii_px;
    }
    [track_rect.height_px * 0.5; 4]
}

fn range_thumb_corner_radii_px(
    authored_border_radius_is_present: bool,
    thumb_rect: crate::types::DomRect,
    authored_corner_radii_px: [f32; 4],
) -> [f32; 4] {
    if authored_border_radius_is_present {
        return authored_corner_radii_px;
    }
    [thumb_rect.height_px * 0.5; 4]
}

fn resolved_text_run_height_px(
    request: &TextLayoutRequest,
    edit_kind: DomElementEditKind,
    box_model: DomBoxModel,
) -> f32 {
    if matches!(edit_kind, DomElementEditKind::SingleLine) {
        return box_model.border_box.height_px.max(0.0);
    }
    with_repo_text_layout_context(|context| context.layout_wrapped_text(request))
        .height_px
        .max(request.line_height_px.unwrap_or(request.font_size_px))
        .max(0.0)
}

fn append_control_icon(
    context: &mut ElementPaintContext<'_>,
    rect: crate::types::DomRect,
    icon_id: UiIconId,
    color_rgba: [f32; 4],
) {
    let icon_size_px = rect.width_px.min(rect.height_px) * 0.5;
    context.plan.icons.push(IconPaintOp {
        position_px: [
            rect.left_px + ((rect.width_px - icon_size_px) * 0.5),
            rect.top_px + ((rect.height_px - icon_size_px) * 0.5),
        ],
        size_px: [icon_size_px, icon_size_px],
        icon_id,
        color_rgba,
    });
}

fn string_attr_for_attribute<'a>(
    node: &'a DomElementNode,
    attribute_name: &DomAttribute,
) -> Option<&'a str> {
    match node.attrs.attributes.get(attribute_name)? {
        DomAttributeValue::String(value) => Some(value.as_str()),
        _ => None,
    }
}

struct TextEntryPaintBox {
    origin_left: f32,
    origin_top: f32,
    content_width: f32,
    content_height: f32,
}

fn text_entry_paint_box(box_model: DomBoxModel) -> TextEntryPaintBox {
    TextEntryPaintBox {
        origin_left: box_model.border_box.left_px
            + box_model.border.left_px
            + box_model.padding.left_px,
        origin_top: box_model.border_box.top_px
            + box_model.border.top_px
            + box_model.padding.top_px,
        content_width: (box_model.border_box.width_px
            - box_model.border.left_px
            - box_model.border.right_px
            - box_model.padding.left_px
            - box_model.padding.right_px)
            .max(0.0),
        content_height: (box_model.border_box.height_px
            - box_model.border.top_px
            - box_model.border.bottom_px
            - box_model.padding.top_px
            - box_model.padding.bottom_px)
            .max(0.0),
    }
}

#[cfg(test)]
mod tests {
    use crate::domains::layer_pool::{LayerConfig, LayerPool};
    use crate::domains::layout::{IdentityDomLayoutResolver, relayout_tree};
    use crate::domains::paint::{LayerPaintPlan, build_layer_paint_plan};
    use crate::domains::visitor::inherited_style::InheritedStyleContext;
    use crate::domains::visitor::style_recompute::recompute_dirty_style_subtree_for_layer_root;
    use crate::types::{
        DomAttribute, DomAttributeValue, DomNodeSlotKey, DomTag, PixelDensity, RootDom,
        RootDomContext,
    };

    use super::{RectPaintOp, TextRunPaintOp};

    fn append_node_with_attrs(
        root: &mut RootDom,
        parent_key: DomNodeSlotKey,
        tag: DomTag,
        attrs: &[(DomAttribute, &str)],
    ) -> DomNodeSlotKey {
        let node_key = root.create_element(tag).expect("node");
        for (attribute, value) in attrs {
            root.set_attr(
                node_key,
                attribute.clone(),
                Some(DomAttributeValue::String((*value).to_owned())),
            )
            .expect("set attr");
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

    fn vec4_is_close(actual: [f32; 4], expected: [f32; 4]) -> bool {
        actual
            .into_iter()
            .zip(expected)
            .all(|(actual, expected)| (actual - expected).abs() <= 0.001)
    }

    #[test]
    fn paint_input_emits_text_run_for_value() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                (DomAttribute::Position, "fixed"),
                (DomAttribute::Width, "220px"),
                (DomAttribute::Height, "84px"),
            ],
        );
        append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Input,
            &[
                (DomAttribute::Width, "120px"),
                (DomAttribute::Height, "24px"),
                (DomAttribute::PaddingLeft, "8px"),
                (DomAttribute::PaddingRight, "8px"),
                (DomAttribute::PaddingTop, "6px"),
                (DomAttribute::PaddingBottom, "6px"),
                (DomAttribute::Value, "hello"),
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
        assert_eq!(
            text.wrap_mode,
            crate::domains::layer_ops::TextWrapMode::None
        );
        assert!(text.size_px[0] > 0.0);
        assert!(text.size_px[1] > 0.0);
    }

    #[test]
    fn paint_input_adds_caret_rect_for_collapsed_selection() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                (DomAttribute::Position, "fixed"),
                (DomAttribute::Width, "220px"),
                (DomAttribute::Height, "84px"),
            ],
        );
        let input = append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Input,
            &[
                (DomAttribute::Width, "120px"),
                (DomAttribute::Height, "24px"),
                (DomAttribute::Value, "hello"),
            ],
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
    fn paint_input_uses_authored_caret_color_for_focus() {
        let context = RootDomContext::new_gpu();
        let mut root = RootDom::new(context.clone());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                (DomAttribute::Position, "fixed"),
                (DomAttribute::Width, "220px"),
                (DomAttribute::Height, "84px"),
            ],
        );
        let input = append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Input,
            &[
                (DomAttribute::Width, "120px"),
                (DomAttribute::Height, "24px"),
                (DomAttribute::Value, "hello"),
                (DomAttribute::Class, "accent"),
            ],
        );
        context
            .load_stylesheet(
                crate::domains::style::StyleRuleOrigin::new("theme", 0),
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
    fn paint_input_defaults_caret_color_to_element_color() {
        let context = RootDomContext::new_gpu();
        let mut root = RootDom::new(context.clone());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                (DomAttribute::Position, "fixed"),
                (DomAttribute::Width, "220px"),
                (DomAttribute::Height, "84px"),
            ],
        );
        let input = append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Input,
            &[
                (DomAttribute::Width, "120px"),
                (DomAttribute::Height, "24px"),
                (DomAttribute::Value, "hello"),
                (DomAttribute::Class, "accent"),
            ],
        );
        context
            .load_stylesheet(
                crate::domains::style::StyleRuleOrigin::new("theme", 0),
                ".accent { color: #33cc66; }",
            )
            .expect("color stylesheet");
        root.focus(input).expect("focus input");

        layout_layer_tree(&mut root, layer_root, (220.0, 84.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (220.0, 84.0));
        let caret_rect = rect_ops(&plan)
            .into_iter()
            .find(|rect| (rect.size_px[0] - 1.0).abs() <= 0.001 && rect.size_px[1] > 0.0)
            .expect("caret rect should exist");

        assert_vec4_close(caret_rect.fill_rgba, [0.2, 0.8, 0.4, 1.0]);
    }

    #[test]
    fn paint_checkbox_input_fills_the_indicator_when_checked() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                (DomAttribute::Position, "fixed"),
                (DomAttribute::Width, "84px"),
                (DomAttribute::Height, "84px"),
            ],
        );
        let checkbox = root.create_element(DomTag::Input).expect("checkbox");
        root.set_attr_str(checkbox, "type", "checkbox")
            .expect("type");
        root.set_attr(
            checkbox,
            DomAttribute::Checked,
            Some(DomAttributeValue::Bool(true)),
        )
        .expect("checked");
        root.set_attr_str(checkbox, "width", "24px").expect("width");
        root.set_attr_str(checkbox, "height", "24px")
            .expect("height");
        root.append_child(layer_root, checkbox)
            .expect("append checkbox");

        layout_layer_tree(&mut root, layer_root, (84.0, 84.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (84.0, 84.0));
        let indicator_fill = rect_ops(&plan)
            .into_iter()
            .find(|rect| {
                rect.fill_rgba[3] > 0.0 && rect.size_px[0] < 24.0 && rect.size_px[1] < 24.0
            })
            .expect("checked checkbox should emit a filled indicator rect");

        assert!(indicator_fill.size_px[0] > 0.0);
        assert!(indicator_fill.size_px[1] > 0.0);
    }

    #[test]
    fn paint_checkbox_input_uses_presented_focus_colors_for_custom_mark() {
        let context = RootDomContext::new_gpu();
        let mut root = RootDom::new(context.clone());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                (DomAttribute::Position, "fixed"),
                (DomAttribute::Width, "84px"),
                (DomAttribute::Height, "84px"),
            ],
        );
        let checkbox = append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Input,
            &[
                (DomAttribute::Type, "checkbox"),
                (DomAttribute::Width, "24px"),
                (DomAttribute::Height, "24px"),
                (DomAttribute::Class, "accent"),
            ],
        );
        root.set_attr(
            checkbox,
            DomAttribute::Checked,
            Some(DomAttributeValue::Bool(true)),
        )
        .expect("checked");
        context
            .load_stylesheet(
                crate::domains::style::StyleRuleOrigin::new("theme", 0),
                ".accent:focus { color: #33cc66; border-color: #ff3366; background-color: #112233; border-radius: 4px; }",
            )
            .expect("checkbox focus stylesheet");
        root.focus(checkbox).expect("focus checkbox");

        layout_layer_tree(&mut root, layer_root, (84.0, 84.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (84.0, 84.0));
        let checkbox_frame = rect_ops(&plan)
            .into_iter()
            .find(|rect| (rect.border_widths_px[0] - 1.5).abs() <= 0.001)
            .expect("custom checkbox frame should exist");
        let checkbox_mark = rect_ops(&plan)
            .into_iter()
            .find(|rect| {
                vec4_is_close(rect.fill_rgba, [0.2, 0.8, 0.4, 1.0])
                    && vec4_is_close(rect.border_widths_px, [0.0; 4])
            })
            .expect("checked checkbox mark should exist");

        assert_vec4_close(checkbox_frame.fill_rgba, [0.066, 0.133, 0.2, 1.0]);
        assert_vec4_close(checkbox_frame.border_top_rgba, [1.0, 0.2, 0.4, 1.0]);
        assert_vec4_close(checkbox_mark.fill_rgba, [0.2, 0.8, 0.4, 1.0]);
        assert_vec4_close(checkbox_frame.corner_radii_px, [4.0; 4]);
    }

    #[test]
    fn paint_range_input_uses_presented_focus_colors_for_custom_bar() {
        let context = RootDomContext::new_gpu();
        let mut root = RootDom::new(context.clone());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                (DomAttribute::Position, "fixed"),
                (DomAttribute::Width, "180px"),
                (DomAttribute::Height, "84px"),
            ],
        );
        let range = append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Input,
            &[
                (DomAttribute::Type, "range"),
                (DomAttribute::Width, "120px"),
                (DomAttribute::Height, "24px"),
                (DomAttribute::Value, "50"),
                (DomAttribute::Class, "accent"),
            ],
        );
        context
            .load_stylesheet(
                crate::domains::style::StyleRuleOrigin::new("theme", 0),
                ".accent:focus { color: #33cc66; background-color: #112233; border-radius: 3px; }",
            )
            .expect("range focus stylesheet");
        root.focus(range).expect("focus range");

        layout_layer_tree(&mut root, layer_root, (180.0, 84.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (180.0, 84.0));
        let rects = rect_ops(&plan);
        let range_base_track = rects
            .iter()
            .copied()
            .find(|rect| {
                vec4_is_close(rect.fill_rgba, [0.066, 0.133, 0.2, 1.0])
                    && rect.size_px[1] > 0.0
                    && rect.size_px[1] < 24.0
            })
            .expect("range base track should use presented background color");
        let range_filled_track = rects
            .iter()
            .copied()
            .find(|rect| {
                vec4_is_close(rect.fill_rgba, [0.2, 0.8, 0.4, 1.0])
                    && (rect.size_px[1] - range_base_track.size_px[1]).abs() <= 0.001
            })
            .expect("range filled track should use presented color");
        let range_thumb = rects
            .iter()
            .copied()
            .find(|rect| {
                vec4_is_close(rect.fill_rgba, [0.2, 0.8, 0.4, 1.0])
                    && rect.size_px[1] > range_base_track.size_px[1]
            })
            .expect("range thumb should use presented color");

        assert_vec4_close(range_base_track.fill_rgba, [0.066, 0.133, 0.2, 1.0]);
        assert_vec4_close(range_base_track.corner_radii_px, [3.0; 4]);
        assert!(range_filled_track.size_px[0] > 0.0);
        assert!(range_filled_track.size_px[0] < range_base_track.size_px[0]);
        assert_vec4_close(range_thumb.corner_radii_px, [3.0; 4]);
    }

    #[test]
    fn paint_number_input_icons_use_element_color() {
        let context = RootDomContext::new_gpu();
        let mut root = RootDom::new(context.clone());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                (DomAttribute::Position, "fixed"),
                (DomAttribute::Width, "220px"),
                (DomAttribute::Height, "84px"),
            ],
        );
        append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Input,
            &[
                (DomAttribute::Type, "number"),
                (DomAttribute::Width, "120px"),
                (DomAttribute::Height, "24px"),
                (DomAttribute::Value, "4"),
                (DomAttribute::Class, "accent"),
            ],
        );
        context
            .load_stylesheet(
                crate::domains::style::StyleRuleOrigin::new("theme", 0),
                ".accent { color: #3366ff; }",
            )
            .expect("number color stylesheet");

        layout_layer_tree(&mut root, layer_root, (220.0, 84.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (220.0, 84.0));

        assert_eq!(plan.icons.len(), 2);
        for icon in &plan.icons {
            assert_vec4_close(icon.color_rgba, [51.0 / 255.0, 102.0 / 255.0, 1.0, 1.0]);
        }
    }

    #[test]
    fn paint_textarea_emits_wrapped_value_text() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                (DomAttribute::Position, "fixed"),
                (DomAttribute::Width, "220px"),
                (DomAttribute::Height, "120px"),
            ],
        );
        append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Textarea,
            &[
                (DomAttribute::Width, "120px"),
                (DomAttribute::Height, "60px"),
                (DomAttribute::Value, "first line\nsecond line"),
            ],
        );

        layout_layer_tree(&mut root, layer_root, (220.0, 120.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (220.0, 120.0));
        let text = text_ops(&plan)
            .into_iter()
            .next()
            .expect("textarea should emit a text run");

        assert_ne!(
            text.wrap_mode,
            crate::domains::layer_ops::TextWrapMode::None
        );
        assert!(text.size_px[1] > 0.0);
    }

    #[test]
    fn paint_img_emits_icon_op_for_internal_icon_source() {
        let mut root = RootDom::new(RootDomContext::new_gpu());
        let document_key = root.root;
        let layer_root = append_node_with_attrs(
            &mut root,
            document_key,
            DomTag::Div,
            &[
                (DomAttribute::Position, "fixed"),
                (DomAttribute::Width, "84px"),
                (DomAttribute::Height, "84px"),
            ],
        );
        append_node_with_attrs(
            &mut root,
            layer_root,
            DomTag::Img,
            &[
                (DomAttribute::Src, "icon:plus"),
                (DomAttribute::Width, "24px"),
                (DomAttribute::Height, "24px"),
            ],
        );

        layout_layer_tree(&mut root, layer_root, (84.0, 84.0));
        let mut layer_pool = x1_layer_pool();
        let plan = build_plan(&mut root, &mut layer_pool, layer_root, (84.0, 84.0));

        assert_eq!(plan.icons.len(), 1);
        assert!(plan.images.is_empty());
    }
}
