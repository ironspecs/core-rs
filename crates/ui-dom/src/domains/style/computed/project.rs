//! The shared computed-group resolver (PLAN.md spine 3.3).
//!
//! This file owns the permanent resolution core of the redesign: the
//! parent-group seed (`parent_inherited_groups`), the group-field resolver
//! (`resolve_declaration_into_groups`), and the transition-longhand slot parser
//! (`transition_longhands_from_strings`). The typed winners fold (spine
//! 3.3g-2a) resolves groups directly from typed winners through this resolver;
//! the old stored-attr → groups projection (`project_node_presentation`) and
//! the legacy-attr key table (`stored_attr_for`) are gone with the vocabulary
//! split (spine 3.4) — production resolution goes through the typed winners
//! fold only.
//!
//! Invariants and rules:
//!
//! - **The resolver is read-only over groups.** `resolve_declaration_into_groups`
//!   writes one parsed `DeclValue` into the correct group field; it never reads
//!   stored attrs and never changes a legacy value.
//! - **Group-copy inheritance (spine 3.3d).** `parent_inherited_groups` seeds
//!   from the nearest ancestor ELEMENT's overlay-merged inherited groups
//!   (`computed_*`), so a mid-flight transition on the parent is the inherited
//!   value (CSS Cascade §6.1).

use arrayvec::ArrayVec;

use super::align_content_kw::AlignContentKw;
use super::align_items_kw::AlignItemsKw;
use super::box_sizing_kw::BoxSizingKw;
use super::computed_groups::ComputedGroups;
use super::display_kw::DisplayKw;
use super::flex_basis_kw::FlexBasisKw;
use super::flex_direction_kw::FlexDirectionKw;
use super::flex_wrap_kw::FlexWrapKw;
use super::justify_content_kw::JustifyContentKw;
use super::overflow_kw::OverflowKw;
use super::position_kw::PositionKw;
use super::resolved_transition_longhands::{
    ResolvedTransitionLonghands, TRANSITION_LONGHAND_CAPACITY, TransitionLonghandProperty,
};
use crate::domains::layout::parse_translate;
use crate::domains::style::decl_value::{DeclLength, DeclValue};
use crate::domains::style::keyword::Keyword;
use crate::domains::style::presentation_property::{PresentationGroup, PresentationProperty};
use crate::domains::style::text_interner::TextInterner;
use crate::types::dom_transition::{
    DomTimingFunction, for_each_comma_separated_css_value, parse_timing_function,
    parse_transition_time_s,
};
use crate::types::root_dom_nodes::RootDomNodes;
use crate::types::{
    DomNode, DomOverflowWrap, DomTextAlign, DomTextDirection, DomWhiteSpace, DomWordBreak,
};

#[cfg(test)]
use crate::domains::visitor::style_recompute::recompute_dirty_style_subtree_for_layer_root;
#[cfg(test)]
use crate::types::{DomNodeSlotKey, DomTag, RootDom, RootDomContext, RootDomContextConfig};

/// Per-node projection context: the resolved own font size the `em`/line-height
/// resolution needs, the parent weight for `bolder`/`lighter`, the text source
/// that resolves `DeclValue::Text` keys (the scratch interner in the
/// dual-write; the store interner in the 3.3g winners fold), and the document
/// computed-text interner for `font-family` re-interning.
pub(crate) struct ProjectionContext<'a> {
    pub own_font_size_px: f32,
    pub parent_weight: u16,
    pub text_source: &'a TextInterner,
    pub computed_texts: &'a mut TextInterner,
}

/// The parent inherited-group seed for one node: the nearest ancestor ELEMENT's
/// three inherited groups, read through the OVERLAY-MERGED accessors
/// (`computed_*`) so a mid-flight transition on the parent is the inherited
/// value (CSS Cascade §6.1 — the transition tier wins the cascade, so the
/// interpolated values are the cascaded values and inherit; spine 3.3e). The
/// ancestor's own stored groups stay transition-free (the projection reads
/// stored attrs), so there is no feedback loop. Walks the `parent_key` chain
/// crossing layer-root boundaries exactly like the legacy
/// `inherited_style_context_from_ancestors` climb it replaces. When no ancestor
/// element exists, the seed is `ComputedGroups::initial()`.
pub(crate) fn parent_inherited_groups(nodes: &RootDomNodes, node: &DomNode) -> ComputedGroups {
    let mut cursor = node.parent_key();
    while let Some(key) = cursor {
        let Some(ancestor) = nodes.get(key) else {
            break;
        };
        if matches!(ancestor, DomNode::Element(_)) {
            let mut groups = ComputedGroups::initial();
            groups.font = ancestor.computed_font();
            groups.text = ancestor.computed_text();
            groups.color = ancestor.computed_color();
            return groups;
        }
        cursor = ancestor.parent_key();
    }
    ComputedGroups::initial()
}

/// The shared, permanent group-field resolver: writes one parsed declaration
/// value into the correct group field. 3.3g's typed winners fold calls this
/// same code with store-resolved `DeclValue`s.
pub(crate) fn resolve_declaration_into_groups(
    groups: &mut ComputedGroups,
    property: PresentationProperty,
    value: DeclValue,
    context: &mut ProjectionContext<'_>,
) {
    match property.group() {
        PresentationGroup::Font => resolve_font_field(groups, property, value, context),
        PresentationGroup::Text => resolve_text_field(groups, property, value),
        PresentationGroup::Color => resolve_color_field(groups, property, value),
        PresentationGroup::Box => {
            resolve_box_field(groups, property, value, context.own_font_size_px);
        }
        PresentationGroup::Flex => {
            resolve_flex_field(groups, property, value, context.own_font_size_px);
        }
        PresentationGroup::Visual => {
            resolve_visual_field(groups, property, value, context.own_font_size_px);
        }
        PresentationGroup::Transform => resolve_transform_field(groups, value, context),
        PresentationGroup::TransitionLonghands => {}
    }
}

// — Font group resolution — //

fn resolve_font_field(
    groups: &mut ComputedGroups,
    property: PresentationProperty,
    value: DeclValue,
    context: &mut ProjectionContext<'_>,
) {
    match property {
        PresentationProperty::LineHeight => {
            let size = context.own_font_size_px;
            groups.font.line_height_px = match value {
                DeclValue::Keyword(Keyword::Normal) => None,
                DeclValue::Number(n) => Some((n * size).max(1.0)),
                DeclValue::Length(DeclLength::Px(px)) => Some(px.max(1.0)),
                DeclValue::Length(DeclLength::Em(em)) => Some((em * size).max(1.0)),
                _ => groups.font.line_height_px,
            };
        }
        PresentationProperty::FontFamily => {
            if let DeclValue::Text(key) = value
                && let Some(text) = context.text_source.resolve(key)
            {
                groups.font.family = Some(context.computed_texts.intern(text));
            }
        }
        PresentationProperty::FontWeight => {
            // `bolder`/`lighter` resolve against the parent's *resolved* group
            // weight (full fidelity, spine 3.3d), replacing the 3.3a
            // nearest-ancestor-attr approximation.
            groups.font.weight = match value {
                DeclValue::Keyword(Keyword::Normal) => 400,
                DeclValue::Keyword(Keyword::Bold) => 700,
                DeclValue::Keyword(Keyword::Bolder) => bolder_weight(context.parent_weight),
                DeclValue::Keyword(Keyword::Lighter) => lighter_weight(context.parent_weight),
                DeclValue::Integer(n) => u16::try_from(n).unwrap_or(groups.font.weight),
                _ => groups.font.weight,
            };
        }
        PresentationProperty::FontStyle => {
            if let DeclValue::Keyword(keyword) = value {
                groups.font.style = keyword;
            }
        }
        _ => {}
    }
}

fn bolder_weight(parent: u16) -> u16 {
    match parent {
        100..=300 => 400,
        400..=500 => 700,
        _ => 900,
    }
}

fn lighter_weight(parent: u16) -> u16 {
    match parent {
        100..=500 => 100,
        600..=700 => 400,
        _ => 700,
    }
}

// — Text group resolution — //

fn resolve_text_field(
    groups: &mut ComputedGroups,
    property: PresentationProperty,
    value: DeclValue,
) {
    let DeclValue::Keyword(keyword) = value else {
        return;
    };
    match property {
        PresentationProperty::TextAlign => {
            if let Some(align) = text_align(keyword) {
                groups.text.align = align;
            }
        }
        PresentationProperty::Direction => {
            if let Some(direction) = text_direction(keyword) {
                groups.text.direction = direction;
            }
        }
        PresentationProperty::WhiteSpace => {
            if let Some(white_space) = white_space(keyword) {
                groups.text.white_space = white_space;
            }
        }
        PresentationProperty::OverflowWrap => {
            if let Some(overflow_wrap) = overflow_wrap(keyword) {
                groups.text.overflow_wrap = overflow_wrap;
            }
        }
        PresentationProperty::WordBreak => {
            if let Some(word_break) = word_break(keyword) {
                groups.text.word_break = word_break;
            }
        }
        _ => {}
    }
}

pub(crate) fn text_align(keyword: Keyword) -> Option<DomTextAlign> {
    Some(match keyword {
        Keyword::Start => DomTextAlign::Start,
        Keyword::Left => DomTextAlign::Left,
        Keyword::Center => DomTextAlign::Center,
        Keyword::Right => DomTextAlign::Right,
        Keyword::Justified => DomTextAlign::Justified,
        Keyword::End => DomTextAlign::End,
        _ => return None,
    })
}

pub(crate) fn text_direction(keyword: Keyword) -> Option<DomTextDirection> {
    Some(match keyword {
        Keyword::Ltr => DomTextDirection::Ltr,
        Keyword::Rtl => DomTextDirection::Rtl,
        _ => return None,
    })
}

pub(crate) fn white_space(keyword: Keyword) -> Option<DomWhiteSpace> {
    Some(match keyword {
        Keyword::Normal => DomWhiteSpace::Normal,
        Keyword::Nowrap => DomWhiteSpace::Nowrap,
        Keyword::Pre => DomWhiteSpace::Pre,
        Keyword::PreWrap => DomWhiteSpace::PreWrap,
        Keyword::PreLine => DomWhiteSpace::PreLine,
        Keyword::BreakSpaces => DomWhiteSpace::BreakSpaces,
        _ => return None,
    })
}

pub(crate) fn overflow_wrap(keyword: Keyword) -> Option<DomOverflowWrap> {
    Some(match keyword {
        Keyword::Normal => DomOverflowWrap::Normal,
        Keyword::BreakWord => DomOverflowWrap::BreakWord,
        Keyword::Anywhere => DomOverflowWrap::Anywhere,
        _ => return None,
    })
}

pub(crate) fn word_break(keyword: Keyword) -> Option<DomWordBreak> {
    Some(match keyword {
        Keyword::Normal => DomWordBreak::Normal,
        Keyword::BreakAll => DomWordBreak::BreakAll,
        Keyword::BreakWord => DomWordBreak::BreakWord,
        Keyword::KeepAll => DomWordBreak::KeepAll,
        _ => return None,
    })
}

// — Color group resolution — //

fn resolve_color_field(
    groups: &mut ComputedGroups,
    property: PresentationProperty,
    value: DeclValue,
) {
    if property == PresentationProperty::Color
        && let DeclValue::Color(rgba) = value
    {
        groups.color.color_rgba = rgba;
    }
}

// — Box group resolution — //

fn resolve_box_field(
    groups: &mut ComputedGroups,
    property: PresentationProperty,
    value: DeclValue,
    size_px: f32,
) {
    match property {
        PresentationProperty::Display
        | PresentationProperty::Position
        | PresentationProperty::Overflow
        | PresentationProperty::Isolate
        | PresentationProperty::BoxSizing => resolve_box_keyword(groups, property, value),
        PresentationProperty::Width
        | PresentationProperty::Height
        | PresentationProperty::MinWidth
        | PresentationProperty::MinHeight
        | PresentationProperty::MaxWidth
        | PresentationProperty::MaxHeight
        | PresentationProperty::Top
        | PresentationProperty::Left
        | PresentationProperty::Right
        | PresentationProperty::Bottom => {
            resolve_optional_box_length(groups, property, value, size_px);
        }
        _ => resolve_box_inset(groups, property, value, size_px),
    }
}

fn resolve_box_keyword(
    groups: &mut ComputedGroups,
    property: PresentationProperty,
    value: DeclValue,
) {
    let DeclValue::Keyword(keyword) = value else {
        return;
    };
    match property {
        PresentationProperty::Display => {
            if let Some(display) = DisplayKw::from_keyword(keyword) {
                groups.box_.display = display;
            }
        }
        PresentationProperty::Position => {
            if let Some(position) = PositionKw::from_keyword(keyword) {
                groups.box_.position = position;
            }
        }
        PresentationProperty::Overflow => {
            if let Some(overflow) = OverflowKw::from_keyword(keyword) {
                groups.box_.overflow = overflow;
            }
        }
        PresentationProperty::Isolate => {
            groups.box_.isolate = keyword == Keyword::Isolate;
        }
        PresentationProperty::BoxSizing => {
            if let Some(box_sizing) = BoxSizingKw::from_keyword(keyword) {
                groups.box_.box_sizing = box_sizing;
            }
        }
        _ => {}
    }
}

fn resolve_optional_box_length(
    groups: &mut ComputedGroups,
    property: PresentationProperty,
    value: DeclValue,
    size_px: f32,
) {
    let target = match property {
        PresentationProperty::Width => &mut groups.box_.width,
        PresentationProperty::Height => &mut groups.box_.height,
        PresentationProperty::MinWidth => &mut groups.box_.min_width,
        PresentationProperty::MinHeight => &mut groups.box_.min_height,
        PresentationProperty::MaxWidth => &mut groups.box_.max_width,
        PresentationProperty::MaxHeight => &mut groups.box_.max_height,
        PresentationProperty::Top => &mut groups.box_.top,
        PresentationProperty::Left => &mut groups.box_.left,
        PresentationProperty::Right => &mut groups.box_.right,
        PresentationProperty::Bottom => &mut groups.box_.bottom,
        _ => return,
    };
    write_optional_length(target, value, size_px);
}

fn resolve_box_inset(
    groups: &mut ComputedGroups,
    property: PresentationProperty,
    value: DeclValue,
    size_px: f32,
) {
    let target = match property {
        PresentationProperty::MarginTop => &mut groups.box_.margin[0],
        PresentationProperty::MarginRight => &mut groups.box_.margin[1],
        PresentationProperty::MarginBottom => &mut groups.box_.margin[2],
        PresentationProperty::MarginLeft => &mut groups.box_.margin[3],
        PresentationProperty::PaddingTop => &mut groups.box_.padding[0],
        PresentationProperty::PaddingRight => &mut groups.box_.padding[1],
        PresentationProperty::PaddingBottom => &mut groups.box_.padding[2],
        PresentationProperty::PaddingLeft => &mut groups.box_.padding[3],
        PresentationProperty::BorderTopWidth => &mut groups.box_.border_width[0],
        PresentationProperty::BorderRightWidth => &mut groups.box_.border_width[1],
        PresentationProperty::BorderBottomWidth => &mut groups.box_.border_width[2],
        PresentationProperty::BorderLeftWidth => &mut groups.box_.border_width[3],
        _ => return,
    };
    write_inset_length(target, value, size_px);
}

// — Flex group resolution — //

fn resolve_flex_field(
    groups: &mut ComputedGroups,
    property: PresentationProperty,
    value: DeclValue,
    size_px: f32,
) {
    match property {
        PresentationProperty::FlexDirection => {
            if let DeclValue::Keyword(keyword) = value
                && let Some(direction) = FlexDirectionKw::from_keyword(keyword)
            {
                groups.flex.direction = direction;
            }
        }
        PresentationProperty::FlexWrap => {
            if let DeclValue::Keyword(keyword) = value
                && let Some(wrap) = FlexWrapKw::from_keyword(keyword)
            {
                groups.flex.wrap = wrap;
            }
        }
        PresentationProperty::FlexGrow => {
            if let DeclValue::Number(n) = value {
                groups.flex.grow = n;
            }
        }
        PresentationProperty::FlexShrink => {
            if let DeclValue::Number(n) = value {
                groups.flex.shrink = n;
            }
        }
        PresentationProperty::FlexBasis => {
            groups.flex.basis = match value {
                DeclValue::Keyword(Keyword::Auto) => FlexBasisKw::Auto,
                DeclValue::Keyword(Keyword::Content) => FlexBasisKw::Content,
                DeclValue::Length(DeclLength::Px(px)) => FlexBasisKw::Length(px),
                DeclValue::Length(DeclLength::Em(em)) => FlexBasisKw::Length(em * size_px),
                _ => groups.flex.basis,
            };
        }
        PresentationProperty::Order => {
            if let DeclValue::Integer(n) = value {
                groups.flex.order =
                    i32::try_from(n.clamp(i64::from(i32::MIN), i64::from(i32::MAX)))
                        .expect("clamped i64 order should fit into i32");
            }
        }
        PresentationProperty::JustifyContent => {
            if let DeclValue::Keyword(keyword) = value
                && let Some(justify) = JustifyContentKw::from_keyword(keyword)
            {
                groups.flex.justify_content = justify;
            }
        }
        PresentationProperty::AlignItems => {
            if let DeclValue::Keyword(keyword) = value
                && let Some(align) = AlignItemsKw::from_keyword(keyword)
            {
                groups.flex.align_items = align;
            }
        }
        PresentationProperty::AlignSelf => {
            if let DeclValue::Keyword(keyword) = value {
                if keyword == Keyword::Auto {
                    groups.flex.align_self = None;
                } else if let Some(align) = AlignItemsKw::from_keyword(keyword) {
                    groups.flex.align_self = Some(align);
                }
            }
        }
        PresentationProperty::AlignContent => {
            if let DeclValue::Keyword(keyword) = value
                && let Some(align) = AlignContentKw::from_keyword(keyword)
            {
                groups.flex.align_content = align;
            }
        }
        PresentationProperty::RowGap => write_inset_length(&mut groups.flex.gap[0], value, size_px),
        PresentationProperty::ColumnGap => {
            write_inset_length(&mut groups.flex.gap[1], value, size_px);
        }
        _ => {}
    }
}

// — Visual group resolution — //

fn resolve_visual_field(
    groups: &mut ComputedGroups,
    property: PresentationProperty,
    value: DeclValue,
    size_px: f32,
) {
    match property {
        PresentationProperty::Opacity => {
            if let DeclValue::Number(n) = value {
                groups.visual.opacity = n;
            }
        }
        PresentationProperty::BackgroundColor => {
            if let DeclValue::Color(rgba) = value {
                groups.visual.background_rgba = rgba;
            }
        }
        PresentationProperty::BorderTopColor => {
            if let DeclValue::Color(rgba) = value {
                groups.visual.border_rgba[0] = rgba;
            }
        }
        PresentationProperty::BorderRightColor => {
            if let DeclValue::Color(rgba) = value {
                groups.visual.border_rgba[1] = rgba;
            }
        }
        PresentationProperty::BorderBottomColor => {
            if let DeclValue::Color(rgba) = value {
                groups.visual.border_rgba[2] = rgba;
            }
        }
        PresentationProperty::BorderLeftColor => {
            if let DeclValue::Color(rgba) = value {
                groups.visual.border_rgba[3] = rgba;
            }
        }
        PresentationProperty::OutlineColor => {
            if let DeclValue::Color(rgba) = value {
                groups.visual.outline_rgba = rgba;
            }
        }
        PresentationProperty::OutlineWidth => {
            write_inset_length(&mut groups.visual.outline_width, value, size_px);
        }
        PresentationProperty::OutlineOffset => {
            write_inset_length(&mut groups.visual.outline_offset, value, size_px);
        }
        PresentationProperty::CaretColor => {
            if let DeclValue::Color(rgba) = value {
                groups.visual.caret_rgba = Some(rgba);
            }
        }
        PresentationProperty::BorderTopLeftRadius => {
            write_inset_length(&mut groups.visual.radius[0], value, size_px);
        }
        PresentationProperty::BorderTopRightRadius => {
            write_inset_length(&mut groups.visual.radius[1], value, size_px);
        }
        PresentationProperty::BorderBottomRightRadius => {
            write_inset_length(&mut groups.visual.radius[2], value, size_px);
        }
        PresentationProperty::BorderBottomLeftRadius => {
            write_inset_length(&mut groups.visual.radius[3], value, size_px);
        }
        _ => {}
    }
}

// — Transform group resolution — //

fn resolve_transform_field(
    groups: &mut ComputedGroups,
    value: DeclValue,
    context: &mut ProjectionContext<'_>,
) {
    match value {
        DeclValue::Keyword(Keyword::None) => {
            groups.transform.translate_px = [0.0, 0.0];
            groups.transform.has_transform = false;
        }
        DeclValue::Text(key) => {
            if let Some(text) = context.text_source.resolve(key) {
                let translate = parse_translate(text);
                groups.transform.translate_px = [translate.x_px, translate.y_px];
                groups.transform.has_transform = true;
            }
        }
        _ => {}
    }
}

// — Length resolution helpers — //

enum LengthResolution {
    Unset,
    Length(f32),
    Leave,
}

fn resolve_length(value: DeclValue, size_px: f32) -> LengthResolution {
    match value {
        DeclValue::Length(DeclLength::Unset) => LengthResolution::Unset,
        DeclValue::Length(DeclLength::Px(px)) => LengthResolution::Length(px),
        DeclValue::Length(DeclLength::Em(em)) => LengthResolution::Length(em * size_px),
        _ => LengthResolution::Leave,
    }
}

fn write_optional_length(target: &mut Option<f32>, value: DeclValue, size_px: f32) {
    match resolve_length(value, size_px) {
        LengthResolution::Unset => *target = None,
        LengthResolution::Length(px) => *target = Some(px),
        LengthResolution::Leave => {}
    }
}

fn write_inset_length(target: &mut f32, value: DeclValue, size_px: f32) {
    match resolve_length(value, size_px) {
        LengthResolution::Unset => *target = 0.0,
        LengthResolution::Length(px) => *target = px,
        LengthResolution::Leave => {}
    }
}

// — Transition longhand slot — //

/// The shared transition-longhand slot parser: builds the scheduler slot from
/// the four resolved longhand strings (cap 4, list-repeat noted). The typed
/// winners fold resolves the four winning `Text` values to strings and calls
/// this same helper.
pub(crate) fn transition_longhands_from_strings(
    property: Option<&str>,
    duration: Option<&str>,
    timing_function: Option<&str>,
    delay: Option<&str>,
) -> ResolvedTransitionLonghands {
    let mut longhands = ResolvedTransitionLonghands::default();

    if let Some(value) = property {
        let _ = for_each_comma_separated_css_value(value, |_, item| {
            push_transition_property(&mut longhands.properties, item);
            Ok(())
        });
    }
    if let Some(value) = duration {
        collect_transition_times(value, &mut longhands.durations_s);
    }
    if let Some(value) = timing_function {
        collect_transition_timing(value, &mut longhands.timing_functions);
    }
    if let Some(value) = delay {
        collect_transition_times(value, &mut longhands.delays_s);
    }

    longhands
}

fn push_transition_property(
    properties: &mut ArrayVec<TransitionLonghandProperty, TRANSITION_LONGHAND_CAPACITY>,
    item: &str,
) {
    if properties.len() >= TRANSITION_LONGHAND_CAPACITY {
        return;
    }
    match item {
        "all" => properties.push(TransitionLonghandProperty::All),
        "none" => properties.push(TransitionLonghandProperty::None),
        name => {
            if let Some(property) = PresentationProperty::from_name(name) {
                properties.push(TransitionLonghandProperty::Property(property));
            }
            // Unknown names in a direct-written list are today's tolerated
            // garbage and are skipped, matching the scheduler's tolerance.
        }
    }
}

fn collect_transition_times(value: &str, target: &mut ArrayVec<f32, TRANSITION_LONGHAND_CAPACITY>) {
    let _ = for_each_comma_separated_css_value(value, |_, item| {
        if target.len() < TRANSITION_LONGHAND_CAPACITY
            && let Some(seconds) = parse_transition_time_s(item)
        {
            target.push(seconds);
        }
        Ok(())
    });
}

fn collect_transition_timing(
    value: &str,
    target: &mut ArrayVec<Keyword, TRANSITION_LONGHAND_CAPACITY>,
) {
    let _ = for_each_comma_separated_css_value(value, |_, item| {
        if target.len() < TRANSITION_LONGHAND_CAPACITY
            && let Some(timing) = parse_timing_function(item)
        {
            target.push(timing_keyword(timing));
        }
        Ok(())
    });
}

fn timing_keyword(timing: DomTimingFunction) -> Keyword {
    match timing {
        DomTimingFunction::Linear => Keyword::Linear,
        DomTimingFunction::Ease => Keyword::Ease,
        DomTimingFunction::EaseIn => Keyword::EaseIn,
        DomTimingFunction::EaseOut => Keyword::EaseOut,
        DomTimingFunction::EaseInOut => Keyword::EaseInOut,
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // projection tests compare exact resolved literals
mod tests {
    use super::*;

    fn test_owner_document_key() -> DomNodeSlotKey {
        let mut keys = slotmap::SlotMap::with_key();
        keys.insert(())
    }

    fn element_node() -> DomNode {
        DomNode::element(test_owner_document_key(), DomTag::Div)
    }

    #[test]
    fn bolder_and_lighter_follow_css_fonts_4_table() {
        assert_eq!(bolder_weight(100), 400);
        assert_eq!(bolder_weight(300), 400);
        assert_eq!(bolder_weight(400), 700);
        assert_eq!(bolder_weight(500), 700);
        assert_eq!(bolder_weight(600), 900);
        assert_eq!(bolder_weight(900), 900);

        assert_eq!(lighter_weight(100), 100);
        assert_eq!(lighter_weight(500), 100);
        assert_eq!(lighter_weight(600), 400);
        assert_eq!(lighter_weight(700), 400);
        assert_eq!(lighter_weight(800), 700);
        assert_eq!(lighter_weight(900), 700);
    }

    #[test]
    fn length_resolution_handles_unset_px_and_em() {
        assert!(matches!(
            resolve_length(DeclValue::Length(DeclLength::Unset), 14.0),
            LengthResolution::Unset
        ));
        assert!(matches!(
            resolve_length(DeclValue::Length(DeclLength::Px(10.0)), 14.0),
            LengthResolution::Length(10.0)
        ));
        assert!(matches!(
            resolve_length(DeclValue::Length(DeclLength::Em(1.5)), 14.0),
            LengthResolution::Length(21.0)
        ));
        assert!(matches!(
            resolve_length(DeclValue::Number(1.0), 14.0),
            LengthResolution::Leave
        ));
    }

    #[test]
    fn resolver_writes_box_width_and_visual_color_from_typed_values() {
        // The surviving resolver (`resolve_declaration_into_groups`) writes
        // typed `DeclValue`s into the correct group fields — the surface the
        // deleted stored-attr projection used to drive. Assert the box width
        // and visual background-color field writes directly.
        let scratch = TextInterner::default();
        let mut computed_texts = TextInterner::default();
        let mut context = ProjectionContext {
            own_font_size_px: 14.0,
            parent_weight: 400,
            text_source: &scratch,
            computed_texts: &mut computed_texts,
        };
        let mut groups = ComputedGroups::initial();
        resolve_declaration_into_groups(
            &mut groups,
            PresentationProperty::Width,
            DeclValue::Length(DeclLength::Px(120.0)),
            &mut context,
        );
        resolve_declaration_into_groups(
            &mut groups,
            PresentationProperty::BackgroundColor,
            DeclValue::Color([1.0, 0.0, 0.0, 1.0]),
            &mut context,
        );

        assert_eq!(groups.box_.width, Some(120.0));
        assert_eq!(groups.visual.background_rgba, [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn transition_property_unknown_names_are_skipped() {
        let mut properties: ArrayVec<TransitionLonghandProperty, TRANSITION_LONGHAND_CAPACITY> =
            ArrayVec::new();
        push_transition_property(&mut properties, "all");
        push_transition_property(&mut properties, "mystery");
        push_transition_property(&mut properties, "left");
        push_transition_property(&mut properties, "none");
        push_transition_property(&mut properties, "width");
        assert_eq!(
            properties.as_slice(),
            &[
                TransitionLonghandProperty::All,
                TransitionLonghandProperty::Property(PresentationProperty::Left),
                TransitionLonghandProperty::None,
                TransitionLonghandProperty::Property(PresentationProperty::Width),
            ]
        );
    }

    #[test]
    fn parent_inherited_groups_seeds_from_nearest_element_or_initial() {
        // A node without a parent seeds from the no-parent initial groups.
        let node = element_node();
        assert_eq!(
            parent_inherited_groups(&RootDomNodes::with_key(), &node),
            ComputedGroups::initial()
        );

        // Build a chain: far (font-size 10px) -> near (font-size 1.5em) ->
        // child. The style pass resolves the near ancestor's groups (1.5em
        // against far's 10px -> 15px), so the child's seed carries 15px.
        let mut root = RootDom::new(std::sync::Arc::new(RootDomContext::new(
            RootDomContextConfig::cpu(),
        )));
        let far = root.create_element(DomTag::Div).expect("far");
        let near = root.create_element(DomTag::Div).expect("near");
        let child = root.create_element(DomTag::Div).expect("child");
        root.append_child(root.root, far).expect("append far");
        root.append_child(far, near).expect("append near");
        root.append_child(near, child).expect("append child");
        root.style_mut(far)
            .set("font-size", "10px")
            .expect("far font-size");
        root.style_mut(near)
            .set("font-size", "1.5em")
            .expect("near font-size");
        let document_key = root.root;
        recompute_dirty_style_subtree_for_layer_root(&mut root, document_key).expect("style pass");

        let child_node = root.nodes.get(child).expect("child");
        let seed = parent_inherited_groups(&root.nodes, child_node);
        assert_eq!(seed.font.size_px, 15.0);
    }
}
