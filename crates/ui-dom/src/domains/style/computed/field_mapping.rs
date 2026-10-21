//! Field-level computed-group diff for the transition scheduler (PLAN.md A7,
//! spine 3.3g-1b).
//!
//! This file owns the single shared *field → `PresentationProperty`* mapping:
//! the inverse of the overlay merge's per-field table in `dom_node.rs` and of
//! the resolver's per-field writes in `project.rs`. The scheduler needs field
//! names, not group names (A7): `transition-property: font-size` must not fire
//! on a `line-height` move even though both live in the font group. So a
//! changed group yields the *moving fields* by field-level compare, and the
//! scheduler matches its property list against those field names.
//!
//! Invariants and rules:
//!
//! - `for_each_moved_field` compares every field of the seven `Copy` groups
//!   (`old != new`) and invokes `visit` once per moved field with the field's
//!   owning `PresentationProperty`. The `transform` group has two fields
//!   (`translate_px`, `has_transform`) that both map to `Transform`; the group
//!   compares as one unit so a transform change fires `visit(Transform)`
//!   exactly once — `Transform` is one transitionable property and must not
//!   double-schedule when both fields move together (e.g. `none` →
//!   `translateX(5px)`).
//! - **Zero allocation.** No buffers, no iteration collectors; the compare is
//!   inline and `visit` is a closure.
//! - The mapping is the mechanical inverse of `resolve_declaration_into_groups`
//!   (`project.rs`): one field per property, except `Transform`'s two fields.

use crate::domains::style::PresentationProperty;

use super::computed_groups::ComputedGroups;

#[cfg(test)]
use super::BoxLength;
#[cfg(test)]
use crate::domains::style::Keyword;
#[cfg(test)]
use crate::domains::style::computed::DisplayKw;

/// Visits one `PresentationProperty` per moved field between `old` and `new`.
///
/// This is the field-level compare the transition scheduler keys on (A7): the
/// scheduler matches its `transition-property` list against these field names,
/// never the group name.
#[allow(clippy::too_many_lines)] // mechanical per-field compare, low complexity
#[allow(clippy::float_cmp)] // exact field diff, matching the groups' derived PartialEq
pub(crate) fn for_each_moved_field(
    old: &ComputedGroups,
    new: &ComputedGroups,
    mut visit: impl FnMut(PresentationProperty),
) {
    // Inherited — font group.
    if old.font.size_px != new.font.size_px {
        visit(PresentationProperty::FontSize);
    }
    if old.font.line_height_px != new.font.line_height_px {
        visit(PresentationProperty::LineHeight);
    }
    if old.font.family != new.font.family {
        visit(PresentationProperty::FontFamily);
    }
    if old.font.weight != new.font.weight {
        visit(PresentationProperty::FontWeight);
    }
    if old.font.style != new.font.style {
        visit(PresentationProperty::FontStyle);
    }

    // Inherited — text group.
    if old.text.align != new.text.align {
        visit(PresentationProperty::TextAlign);
    }
    if old.text.direction != new.text.direction {
        visit(PresentationProperty::Direction);
    }
    if old.text.white_space != new.text.white_space {
        visit(PresentationProperty::WhiteSpace);
    }
    if old.text.overflow_wrap != new.text.overflow_wrap {
        visit(PresentationProperty::OverflowWrap);
    }
    if old.text.word_break != new.text.word_break {
        visit(PresentationProperty::WordBreak);
    }

    // Inherited — color group.
    if old.color.color_rgba != new.color.color_rgba {
        visit(PresentationProperty::Color);
    }

    // Non-inherited — box group.
    if old.box_.display != new.box_.display {
        visit(PresentationProperty::Display);
    }
    if old.box_.position != new.box_.position {
        visit(PresentationProperty::Position);
    }
    if old.box_.overflow != new.box_.overflow {
        visit(PresentationProperty::Overflow);
    }
    if old.box_.isolate != new.box_.isolate {
        visit(PresentationProperty::Isolate);
    }
    if old.box_.box_sizing != new.box_.box_sizing {
        visit(PresentationProperty::BoxSizing);
    }
    if old.box_.width != new.box_.width {
        visit(PresentationProperty::Width);
    }
    if old.box_.height != new.box_.height {
        visit(PresentationProperty::Height);
    }
    if old.box_.min_width != new.box_.min_width {
        visit(PresentationProperty::MinWidth);
    }
    if old.box_.min_height != new.box_.min_height {
        visit(PresentationProperty::MinHeight);
    }
    if old.box_.max_width != new.box_.max_width {
        visit(PresentationProperty::MaxWidth);
    }
    if old.box_.max_height != new.box_.max_height {
        visit(PresentationProperty::MaxHeight);
    }
    if old.box_.top != new.box_.top {
        visit(PresentationProperty::Top);
    }
    if old.box_.left != new.box_.left {
        visit(PresentationProperty::Left);
    }
    if old.box_.right != new.box_.right {
        visit(PresentationProperty::Right);
    }
    if old.box_.bottom != new.box_.bottom {
        visit(PresentationProperty::Bottom);
    }
    visit_moved_inset(
        old.box_.margin,
        new.box_.margin,
        MARGIN_PROPERTIES,
        &mut visit,
    );
    visit_moved_inset(
        old.box_.padding,
        new.box_.padding,
        PADDING_PROPERTIES,
        &mut visit,
    );
    visit_moved_inset(
        old.box_.border_width,
        new.box_.border_width,
        BORDER_WIDTH_PROPERTIES,
        &mut visit,
    );

    // Non-inherited — flex group.
    if old.flex.direction != new.flex.direction {
        visit(PresentationProperty::FlexDirection);
    }
    if old.flex.wrap != new.flex.wrap {
        visit(PresentationProperty::FlexWrap);
    }
    if old.flex.grow != new.flex.grow {
        visit(PresentationProperty::FlexGrow);
    }
    if old.flex.shrink != new.flex.shrink {
        visit(PresentationProperty::FlexShrink);
    }
    if old.flex.basis != new.flex.basis {
        visit(PresentationProperty::FlexBasis);
    }
    if old.flex.order != new.flex.order {
        visit(PresentationProperty::Order);
    }
    if old.flex.justify_content != new.flex.justify_content {
        visit(PresentationProperty::JustifyContent);
    }
    if old.flex.align_items != new.flex.align_items {
        visit(PresentationProperty::AlignItems);
    }
    if old.flex.align_self != new.flex.align_self {
        visit(PresentationProperty::AlignSelf);
    }
    if old.flex.align_content != new.flex.align_content {
        visit(PresentationProperty::AlignContent);
    }
    if old.flex.gap[0] != new.flex.gap[0] {
        visit(PresentationProperty::RowGap);
    }
    if old.flex.gap[1] != new.flex.gap[1] {
        visit(PresentationProperty::ColumnGap);
    }

    // Non-inherited — grid group.
    if old.grid.template_columns != new.grid.template_columns {
        visit(PresentationProperty::GridTemplateColumns);
    }
    if old.grid.template_rows != new.grid.template_rows {
        visit(PresentationProperty::GridTemplateRows);
    }
    if old.grid.template_areas != new.grid.template_areas {
        visit(PresentationProperty::GridTemplateAreas);
    }
    if old.grid.auto_flow != new.grid.auto_flow {
        visit(PresentationProperty::GridAutoFlow);
    }
    if old.grid.auto_columns != new.grid.auto_columns {
        visit(PresentationProperty::GridAutoColumns);
    }
    if old.grid.auto_rows != new.grid.auto_rows {
        visit(PresentationProperty::GridAutoRows);
    }
    if old.grid.column_start != new.grid.column_start {
        visit(PresentationProperty::GridColumnStart);
    }
    if old.grid.column_end != new.grid.column_end {
        visit(PresentationProperty::GridColumnEnd);
    }
    if old.grid.row_start != new.grid.row_start {
        visit(PresentationProperty::GridRowStart);
    }
    if old.grid.row_end != new.grid.row_end {
        visit(PresentationProperty::GridRowEnd);
    }
    if old.grid.area_name != new.grid.area_name {
        visit(PresentationProperty::GridArea);
    }
    if old.grid.justify_items != new.grid.justify_items {
        visit(PresentationProperty::JustifyItems);
    }
    if old.grid.justify_self != new.grid.justify_self {
        visit(PresentationProperty::JustifySelf);
    }

    // Non-inherited — visual group.
    if old.visual.opacity != new.visual.opacity {
        visit(PresentationProperty::Opacity);
    }
    if old.visual.background_rgba != new.visual.background_rgba {
        visit(PresentationProperty::BackgroundColor);
    }
    visit_moved_rgba(
        old.visual.border_rgba,
        new.visual.border_rgba,
        BORDER_COLOR_PROPERTIES,
        &mut visit,
    );
    if old.visual.outline_rgba != new.visual.outline_rgba {
        visit(PresentationProperty::OutlineColor);
    }
    if old.visual.outline_width != new.visual.outline_width {
        visit(PresentationProperty::OutlineWidth);
    }
    if old.visual.outline_offset != new.visual.outline_offset {
        visit(PresentationProperty::OutlineOffset);
    }
    if old.visual.caret_rgba != new.visual.caret_rgba {
        visit(PresentationProperty::CaretColor);
    }
    visit_moved_inset(
        old.visual.radius,
        new.visual.radius,
        RADIUS_PROPERTIES,
        &mut visit,
    );

    // Non-inherited — transform group (one property: both fields map to
    // `Transform`, so the group compares as a unit to visit exactly once).
    if old.transform != new.transform {
        visit(PresentationProperty::Transform);
    }
}

/// `[top, right, bottom, left]` order, mirroring the resolver's inset writes.
const MARGIN_PROPERTIES: [PresentationProperty; 4] = [
    PresentationProperty::MarginTop,
    PresentationProperty::MarginRight,
    PresentationProperty::MarginBottom,
    PresentationProperty::MarginLeft,
];

const PADDING_PROPERTIES: [PresentationProperty; 4] = [
    PresentationProperty::PaddingTop,
    PresentationProperty::PaddingRight,
    PresentationProperty::PaddingBottom,
    PresentationProperty::PaddingLeft,
];

const BORDER_WIDTH_PROPERTIES: [PresentationProperty; 4] = [
    PresentationProperty::BorderTopWidth,
    PresentationProperty::BorderRightWidth,
    PresentationProperty::BorderBottomWidth,
    PresentationProperty::BorderLeftWidth,
];

const BORDER_COLOR_PROPERTIES: [PresentationProperty; 4] = [
    PresentationProperty::BorderTopColor,
    PresentationProperty::BorderRightColor,
    PresentationProperty::BorderBottomColor,
    PresentationProperty::BorderLeftColor,
];

/// `[TL, TR, BR, BL]` order, mirroring the resolver's radius writes.
const RADIUS_PROPERTIES: [PresentationProperty; 4] = [
    PresentationProperty::BorderTopLeftRadius,
    PresentationProperty::BorderTopRightRadius,
    PresentationProperty::BorderBottomRightRadius,
    PresentationProperty::BorderBottomLeftRadius,
];

#[allow(clippy::float_cmp)] // exact field diff, matching the groups' derived PartialEq
fn visit_moved_inset<T: Copy + PartialEq>(
    old: [T; 4],
    new: [T; 4],
    properties: [PresentationProperty; 4],
    visit: &mut impl FnMut(PresentationProperty),
) {
    for index in 0..4 {
        if old[index] != new[index] {
            visit(properties[index]);
        }
    }
}

#[allow(clippy::float_cmp)] // exact field diff, matching the groups' derived PartialEq
fn visit_moved_rgba(
    old: [[f32; 4]; 4],
    new: [[f32; 4]; 4],
    properties: [PresentationProperty; 4],
    visit: &mut impl FnMut(PresentationProperty),
) {
    for index in 0..4 {
        if old[index] != new[index] {
            visit(properties[index]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moved(old: &ComputedGroups, new: &ComputedGroups) -> Vec<PresentationProperty> {
        let mut properties = Vec::new();
        for_each_moved_field(old, new, |property| properties.push(property));
        properties
    }

    #[test]
    fn no_change_visits_nothing() {
        let groups = ComputedGroups::initial();
        assert!(moved(&groups, &groups).is_empty());
    }

    #[test]
    fn one_moved_field_maps_to_its_property_not_its_group() {
        let old = ComputedGroups::initial();
        let mut new = old;
        new.font.line_height_px = Some(24.0);
        // A line-height move in the font group must NOT visit FontSize.
        assert_eq!(moved(&old, &new), vec![PresentationProperty::LineHeight]);
    }

    #[test]
    fn inset_and_side_fields_map_to_their_edges() {
        let old = ComputedGroups::initial();
        let mut new = old;
        new.box_.margin[1] = BoxLength::Px(8.0);
        new.box_.padding[3] = BoxLength::Px(4.0);
        new.box_.border_width[0] = 2.0;
        new.visual.border_rgba[2] = [1.0, 0.0, 0.0, 1.0];
        new.visual.radius[0] = 6.0;
        let mut moved = moved(&old, &new);
        moved.sort();
        let mut expected = vec![
            PresentationProperty::MarginRight,
            PresentationProperty::PaddingLeft,
            PresentationProperty::BorderTopWidth,
            PresentationProperty::BorderBottomColor,
            PresentationProperty::BorderTopLeftRadius,
        ];
        expected.sort();
        assert_eq!(moved, expected);
    }

    #[test]
    fn transform_group_maps_both_fields_to_transform() {
        let old = ComputedGroups::initial();
        let mut new = old;
        new.transform.has_transform = true;
        assert_eq!(moved(&old, &new), vec![PresentationProperty::Transform]);

        let mut new = old;
        new.transform.translate_px = [3.0, 4.0];
        assert_eq!(moved(&old, &new), vec![PresentationProperty::Transform]);

        // A change that moves BOTH fields still fires once: the group is one
        // transitionable property and must not double-schedule.
        let mut new = old;
        new.transform.translate_px = [5.0, 0.0];
        new.transform.has_transform = true;
        assert_eq!(moved(&old, &new), vec![PresentationProperty::Transform]);
    }

    #[test]
    fn keyword_fields_map_to_their_properties() {
        let old = ComputedGroups::initial();
        let mut new = old;
        new.font.style = Keyword::Italic;
        new.box_.display = DisplayKw::None;
        let mut moved = moved(&old, &new);
        moved.sort();
        let mut expected = vec![
            PresentationProperty::FontStyle,
            PresentationProperty::Display,
        ];
        expected.sort();
        assert_eq!(moved, expected);
    }
}
