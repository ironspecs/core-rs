//! A1 declaration parser for the `ui-dom` style system.
//!
//! This file owns A1 — declaration parsing with forward-compatible error
//! handling (CSS 2.1 §4.1.7, css-style-attr §3). It is the ONE
//! shorthand-expansion point in the system (CSS Cascade 5 §3: a shorthand
//! sets all its longhands exactly as if expanded in place, at declaration
//! time). Grammars are hand-rolled and closed (justified: closed vocabulary,
//! fail-closed, zero backtracking — a documented mechanics divergence from
//! css-syntax's token model, adopted per PLAN.md A1).
//!
//! Invariants and rules:
//!
//! - this is boundary parsing: no input string may panic this file; every
//!   malformed declaration becomes a counted drop (block parse) or a typed
//!   `DomError` (strict parse)
//! - shorthand expansion happens here and only here; longhands are the only
//!   thing a caller ever sees produced
//! - every grammar family is a small helper (<20 complexity); the
//!   `DeclGrammar` match only dispatches, it never parses
//! - numeric parsing always rejects non-finite values (`NaN`, `inf`, `-inf`)
//! - grid track `fr` and `%` leaves reject negative values at admission (CSS
//!   Grid §7.1: `<flex>` is non-negative, negative track sizes are invalid),
//!   exactly like the px arm's `px >= 0.0` gate — the four sites are the leaf
//!   `fr` arm, the leaf `%` arm, and both minmax bound percent arms
//! - the `auto` keyword is admitted only for the closed `AUTO_ADMISSIBLE`
//!   property set (fail-closed, before grammar dispatch); properties outside
//!   the set fall through to their grammar, which rejects `auto` (e.g.
//!   `padding: auto`, `border-top-width: auto`, `font-size: auto` all drop)
//! - `<percentage>` lengths parse to `DeclLength::Percent`; signed `Length`
//!   admits negative percentages and `NonNegativeLength` rejects them, while
//!   the grammars whose fields have no percentage resolution target reject ALL
//!   percentages at admission (a counted drop, never admit-and-be-wrong):
//!   `Gap` (the flex gap field is `[f32; 2]`), `LineWidth` (border widths —
//!   spec-closed `<line-width>` — and the four radius corners, whose
//!   `radius: [f32; 4]` circular-corner model cannot represent the elliptical
//!   `<length-percentage>` resolution), and `SignedLineWidth` (outline-offset,
//!   spec-closed `<length>`)
//! - expansion helpers parse each produced token through the target longhand's
//!   grammar (the sign policy cannot be bypassed via a shorthand). Two helpers
//!   are equivalent-by-construction rather than literally routed:
//!   `expand_border_color` calls the same leaf the `Color` grammar wraps, and
//!   `expand_flex` calls the `FlexBasis`/`NonNegativeNumber` dispatch targets
//!   directly — behaviorally identical today, noted so a future grammar
//!   re-point is applied at those leaves too. The radius corners route through
//!   `BorderTopLeftRadius`, the gaps through `RowGap`, the four margins through
//!   `MarginTop` (signed `Length`), and the four paddings through `PaddingTop`
//!   (`NonNegativeLength`) as representatives of their (currently identical)
//!   longhand grammars. `expand_margin` additionally admits the `auto` keyword
//!   per token (CSS Box 3 §4.2 — `margin: auto`/`margin: 0 auto` centering),
//!   mirroring the longhand's `AUTO_ADMISSIBLE` entry and producing
//!   `DeclValue::Keyword(Keyword::Auto)`; `expand_padding` does not (padding
//!   stays `NonNegativeLength` and `auto` keeps dropping)

// `parse_declaration_block` is wired into the stylesheet load path (spine
// step 2); `parse_declaration_strict` is wired into the `style_mut` single
// declaration write contract (spine step 2d).

use arrayvec::ArrayVec;
use smallvec::SmallVec;

use super::computed::{
    AreaRect, GRID_AREA_COL_CAPACITY, GRID_AREA_NAME_CAPACITY, GRID_AREA_ROW_CAPACITY,
    GridAutoFlowKw, GridLine, GridRepeat, GridTemplateAreas, GridTrackList, GridTrackSize,
    TrackBreadth, TrackMaxBreadth,
};
use super::decl_block::DeclBlock;
use super::decl_drop_report::DeclDropReport;
use super::decl_store::DeclStore;
use super::decl_value::{DeclLength, DeclValue, GridAreasKey, GridTrackListKey, TextKey};
use super::declaration::Declaration;
use super::keyword::Keyword;
use super::parsed_declaration::ParsedDeclaration;
use super::presentation_property::{DeclGrammar, PresentationProperty};
use crate::types::dom_transition::transition_longhand_values_from_shorthand;
use crate::types::{
    AuthoredLength, DomAttributeValue, DomError, parse_authored_length, parse_css_color_rgba,
};

/// A1 block parse with G semantics: parses a declaration-list; malformed
/// declarations are dropped and parsing continues (CSS 2.1 §4.1.7).
/// `report.applied` counts successfully parsed declaration segments (each may
/// expand to several longhands); `report.dropped` counts malformed segments.
pub(crate) fn parse_declaration_block(
    text: &str,
    store: &mut DeclStore,
) -> (DeclBlock, DeclDropReport) {
    let mut block = DeclBlock::new();
    let mut report = DeclDropReport::default();
    for segment in text.split(';') {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        let Some((name, value)) = parse_segment(segment) else {
            report.dropped += 1;
            continue;
        };
        let (value, important) = strip_importance_marker(value);
        let Some(shape) = property_shape(name) else {
            report.dropped += 1;
            continue;
        };
        let Some(declarations) = expand_shape(shape, value, store) else {
            report.dropped += 1;
            continue;
        };
        for declaration in declarations {
            block.upsert(declaration, important);
        }
        report.applied += 1;
    }
    (block, report)
}

/// Strict single-authored-declaration parse (the `StyleMut::set` contract):
/// all-or-nothing — any malformation is a typed `DomError`, never a drop.
/// Shorthands expand to their longhands; a longhand yields one declaration.
/// The `!important` marker (if any) is stripped from the value and carried on
/// every produced `ParsedDeclaration`.
pub(crate) fn parse_declaration_strict(
    property: &str,
    value: &str,
    store: &mut DeclStore,
) -> Result<SmallVec<[ParsedDeclaration; 4]>, DomError> {
    let property = property.trim();
    let value = value.trim();
    if property.is_empty() || value.is_empty() {
        return Err(DomError::InvalidAttributeValue);
    }
    let (value, important) = strip_importance_marker(value);
    if value.is_empty() {
        return Err(DomError::InvalidAttributeValue);
    }
    let shape = property_shape(property).ok_or(DomError::InvalidAttributeValue)?;
    let declarations = expand_shape(shape, value, store).ok_or(DomError::InvalidAttributeValue)?;
    let declarations: SmallVec<[ParsedDeclaration; 4]> = declarations
        .into_iter()
        .map(|declaration| ParsedDeclaration {
            property: declaration.property,
            value: declaration.value,
            important,
        })
        .collect();
    Ok(declarations)
}

/// Splits one pre-trimmed, non-empty `;`-segment into a `(name, value)` pair.
/// `None` when the segment lacks a colon, carries a second colon, or has an
/// empty name or value (CSS 2.1 §4.1.7 drop rules). Empty segments are the
/// caller's concern and are skipped, not dropped.
fn parse_segment(segment: &str) -> Option<(&str, &str)> {
    let (name, value) = segment.split_once(':')?;
    if value.contains(':') {
        return None;
    }
    let name = name.trim();
    let value = value.trim();
    if name.is_empty() || value.is_empty() {
        return None;
    }
    Some((name, value))
}

/// Splits a trailing `!important` marker off a declaration value, per CSS
/// Syntax 3 §5.5 (the importance flag is part of the declaration, not the
/// value). Returns `(value_without_marker, important)`.
///
/// The marker is `!` + optional whitespace + `important` (ASCII
/// case-insensitive), preceded by optional whitespace, matched ONLY at the end
/// of the value string — `10px !important`, `10px!important`, and
/// `10px ! IMPORTANT` are markers, while `10px !important 5px` and
/// `!important 10px` are NOT (the whole string is returned unchanged with
/// `important == false`, and the value grammar rejects it later).
fn strip_importance_marker(value: &str) -> (&str, bool) {
    let Some(exclaim) = value.rfind('!') else {
        return (value, false);
    };
    let after = value[exclaim + 1..].trim_start();
    if let (Some(head), Some(tail)) = (
        after.get(.."important".len()),
        after.get("important".len()..),
    ) && head.eq_ignore_ascii_case("important")
        && tail.trim().is_empty()
    {
        return (value[..exclaim].trim_end(), true);
    }
    (value, false)
}

/// The resolved shape of a declaration name: a longhand property or a
/// shorthand that expands to longhands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PropertyShape {
    Longhand(PresentationProperty),
    Shorthand(Shorthand),
}

/// The closed shorthand vocabulary admitted by A1.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Shorthand {
    BorderRadius,
    BorderColor,
    Gap,
    Flex,
    FlexFlow,
    PlaceContent,
    PlaceItems,
    PlaceSelf,
    GridColumn,
    GridRow,
    GridArea,
    Transition,
    BorderSide(BorderSide),
    Margin,
    Padding,
}

/// The four sides of the border-side (width-only) shorthand aliases.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BorderSide {
    Top,
    Right,
    Bottom,
    Left,
}

/// Resolves a property name to a longhand or a shorthand. Unknown names yield
/// `None` (the caller drops or errors them).
///
/// Shorthand names resolve BEFORE longhand names: `grid-area` is a shorthand
/// that expands to the four placement longhands, so the shorthand must win the
/// name lookup over any same-named longhand.
fn property_shape(name: &str) -> Option<PropertyShape> {
    if let Some(shorthand) = shorthand_for_name(name) {
        return Some(PropertyShape::Shorthand(shorthand));
    }
    PresentationProperty::from_name(name).map(PropertyShape::Longhand)
}

/// The closed shorthand name table (`property_shape`'s first dispatch).
fn shorthand_for_name(name: &str) -> Option<Shorthand> {
    Some(match name {
        "border-radius" => Shorthand::BorderRadius,
        "border-color" => Shorthand::BorderColor,
        "gap" => Shorthand::Gap,
        "flex" => Shorthand::Flex,
        "flex-flow" => Shorthand::FlexFlow,
        "place-content" => Shorthand::PlaceContent,
        "place-items" => Shorthand::PlaceItems,
        "place-self" => Shorthand::PlaceSelf,
        "grid-column" => Shorthand::GridColumn,
        "grid-row" => Shorthand::GridRow,
        "grid-area" => Shorthand::GridArea,
        "transition" => Shorthand::Transition,
        "border-top" => Shorthand::BorderSide(BorderSide::Top),
        "border-right" => Shorthand::BorderSide(BorderSide::Right),
        "border-bottom" => Shorthand::BorderSide(BorderSide::Bottom),
        "border-left" => Shorthand::BorderSide(BorderSide::Left),
        "margin" => Shorthand::Margin,
        "padding" => Shorthand::Padding,
        _ => return None,
    })
}

/// Resolves a property name to the longhand set it writes (a longhand maps
/// to itself; a shorthand to its expansion set). Unknown names return `None`.
///
/// This is the `remove`-contract mirror of `expand_shape`: it names the
/// longhands a write touches without parsing a value.
pub(crate) fn longhands_for_name(name: &str) -> Option<SmallVec<[PresentationProperty; 5]>> {
    let shape = property_shape(name)?;
    Some(match shape {
        PropertyShape::Longhand(property) => SmallVec::from_slice(&[property]),
        PropertyShape::Shorthand(shorthand) => longhands_for_shorthand(shorthand),
    })
}

/// True when `name` is a presentation-property write: one of the 71 longhand
/// names or one of the 16 shorthand names (the `longhands_for_name` admission
/// surface). The D5 enforcement gate (`set_attr_str`) consults this BEFORE
/// `DomAttribute::try_from`, so a presentation name never reaches the content
/// vocabulary's `Custom` fallthrough; `set_attr` applies the same check to
/// `Custom(name)` typed writes.
pub(crate) fn name_is_presentation(name: &str) -> bool {
    longhands_for_name(name).is_some()
}

/// The longhand set a shorthand expands to, in expansion order. Names only;
/// the value grammar is `expand_shorthand`'s concern.
fn longhands_for_shorthand(shorthand: Shorthand) -> SmallVec<[PresentationProperty; 5]> {
    let longhands: &[PresentationProperty] = match shorthand {
        Shorthand::BorderRadius => &[
            PresentationProperty::BorderTopLeftRadius,
            PresentationProperty::BorderTopRightRadius,
            PresentationProperty::BorderBottomRightRadius,
            PresentationProperty::BorderBottomLeftRadius,
        ],
        Shorthand::BorderColor => &[
            PresentationProperty::BorderTopColor,
            PresentationProperty::BorderRightColor,
            PresentationProperty::BorderBottomColor,
            PresentationProperty::BorderLeftColor,
        ],
        Shorthand::Gap => &[
            PresentationProperty::RowGap,
            PresentationProperty::ColumnGap,
        ],
        Shorthand::Flex => &[
            PresentationProperty::FlexGrow,
            PresentationProperty::FlexShrink,
            PresentationProperty::FlexBasis,
        ],
        Shorthand::FlexFlow => &[
            PresentationProperty::FlexDirection,
            PresentationProperty::FlexWrap,
        ],
        Shorthand::PlaceContent => &[
            PresentationProperty::AlignContent,
            PresentationProperty::JustifyContent,
        ],
        Shorthand::PlaceItems => &[
            PresentationProperty::AlignItems,
            PresentationProperty::JustifyItems,
        ],
        Shorthand::PlaceSelf => &[
            PresentationProperty::AlignSelf,
            PresentationProperty::JustifySelf,
        ],
        Shorthand::GridColumn => &[
            PresentationProperty::GridColumnStart,
            PresentationProperty::GridColumnEnd,
        ],
        Shorthand::GridRow => &[
            PresentationProperty::GridRowStart,
            PresentationProperty::GridRowEnd,
        ],
        Shorthand::GridArea => &[
            PresentationProperty::GridRowStart,
            PresentationProperty::GridColumnStart,
            PresentationProperty::GridRowEnd,
            PresentationProperty::GridColumnEnd,
        ],
        Shorthand::Transition => &[
            PresentationProperty::TransitionProperty,
            PresentationProperty::TransitionDuration,
            PresentationProperty::TransitionTimingFunction,
            PresentationProperty::TransitionDelay,
        ],
        Shorthand::BorderSide(side) => match side {
            BorderSide::Top => &[PresentationProperty::BorderTopWidth],
            BorderSide::Right => &[PresentationProperty::BorderRightWidth],
            BorderSide::Bottom => &[PresentationProperty::BorderBottomWidth],
            BorderSide::Left => &[PresentationProperty::BorderLeftWidth],
        },
        Shorthand::Margin => &[
            PresentationProperty::MarginTop,
            PresentationProperty::MarginRight,
            PresentationProperty::MarginBottom,
            PresentationProperty::MarginLeft,
        ],
        Shorthand::Padding => &[
            PresentationProperty::PaddingTop,
            PresentationProperty::PaddingRight,
            PresentationProperty::PaddingBottom,
            PresentationProperty::PaddingLeft,
        ],
    };
    SmallVec::from_slice(longhands)
}

/// Expands a resolved shape into its longhand declarations. `None` on any
/// grammar or expansion failure.
fn expand_shape(
    shape: PropertyShape,
    value: &str,
    store: &mut DeclStore,
) -> Option<Vec<Declaration>> {
    match shape {
        PropertyShape::Longhand(property) => {
            let parsed = parse_property_value(property, value, store)?;
            Some(vec![declaration(property, parsed)])
        }
        PropertyShape::Shorthand(shorthand) => expand_shorthand(shorthand, value, store),
    }
}

/// The properties whose grammar admits the `auto` keyword (CSS: `auto` is
/// invalid for padding, border widths, radii, gaps, outlines, and font sizes).
/// This is the closed, fail-closed admission table for the segment parser: a
/// property NOT in the set falls through to its grammar, which rejects `auto`.
const AUTO_ADMISSIBLE: &[PresentationProperty] = &[
    PresentationProperty::Width,
    PresentationProperty::Height,
    PresentationProperty::MinWidth,
    PresentationProperty::MinHeight,
    PresentationProperty::MaxWidth,
    PresentationProperty::MaxHeight,
    PresentationProperty::Top,
    PresentationProperty::Left,
    PresentationProperty::Right,
    PresentationProperty::Bottom,
    PresentationProperty::MarginTop,
    PresentationProperty::MarginRight,
    PresentationProperty::MarginBottom,
    PresentationProperty::MarginLeft,
];

/// Parses one longhand's declared value, admitting the `auto` keyword BEFORE
/// grammar dispatch for the closed `AUTO_ADMISSIBLE` property set. Properties
/// outside the set fall through to their grammar, which rejects `auto` (a
/// counted drop) — e.g. `padding: auto`, `border-top-width: auto`, and
/// `font-size: auto` all drop.
fn parse_property_value(
    property: PresentationProperty,
    value: &str,
    store: &mut DeclStore,
) -> Option<DeclValue> {
    if value == "auto" && AUTO_ADMISSIBLE.contains(&property) {
        return Some(DeclValue::Keyword(Keyword::Auto));
    }
    // The row-axis track list gates content-based auto-repeat span tracks
    // (their inline-axis repetition basis is wrong for the block axis, D1/NB1).
    if property == PresentationProperty::GridTemplateRows {
        return parse_row_track_list(value, store).map(DeclValue::TrackList);
    }
    parse_value(property.grammar(), value, store)
}

fn expand_shorthand(
    shorthand: Shorthand,
    value: &str,
    store: &mut DeclStore,
) -> Option<Vec<Declaration>> {
    match shorthand {
        Shorthand::BorderRadius => expand_border_radius(value),
        Shorthand::BorderColor => expand_border_color(value),
        Shorthand::Gap => expand_gap(value),
        Shorthand::Flex => expand_flex(value),
        Shorthand::FlexFlow => expand_flex_flow(value),
        Shorthand::PlaceContent => expand_place_content(value),
        Shorthand::PlaceItems => expand_place_items(value),
        Shorthand::PlaceSelf => expand_place_self(value),
        Shorthand::GridColumn => expand_grid_column(value, store),
        Shorthand::GridRow => expand_grid_row(value, store),
        Shorthand::GridArea => expand_grid_area(value, store),
        Shorthand::Transition => expand_transition(value, store),
        Shorthand::BorderSide(side) => expand_border_side(side, value),
        Shorthand::Margin => expand_margin(value),
        Shorthand::Padding => expand_padding(value),
    }
}

fn declaration(property: PresentationProperty, value: DeclValue) -> Declaration {
    Declaration { property, value }
}

/// Dispatches one declared value to its grammar-family helper. This match only
/// routes; it never parses.
fn parse_value(grammar: DeclGrammar, value: &str, store: &mut DeclStore) -> Option<DeclValue> {
    match grammar {
        DeclGrammar::Length => parse_length(value).map(DeclValue::Length),
        DeclGrammar::NonNegativeLength => {
            parse_non_negative_length_or_unset(value).map(DeclValue::Length)
        }
        DeclGrammar::LineWidth => parse_line_width(value).map(DeclValue::Length),
        DeclGrammar::SignedLineWidth => parse_signed_line_width(value).map(DeclValue::Length),
        DeclGrammar::Gap => parse_gap_longhand(value).map(DeclValue::Length),
        DeclGrammar::LineHeight => parse_line_height(value),
        DeclGrammar::Color => parse_color(value),
        DeclGrammar::Opacity => parse_opacity(value),
        DeclGrammar::NonNegativeNumber => parse_non_negative_number(value),
        DeclGrammar::Integer => parse_integer(value),
        DeclGrammar::FontWeight => parse_font_weight(value),
        DeclGrammar::FlexBasis => parse_flex_basis_value(value),
        DeclGrammar::Transform => parse_transform(value, store),
        DeclGrammar::Text => parse_text(value, store),
        DeclGrammar::Keywords(set) => Keyword::from_name_in(set, value).map(DeclValue::Keyword),
        DeclGrammar::TrackList => parse_track_list(value, store).map(DeclValue::TrackList),
        DeclGrammar::GridAreas => parse_grid_areas(value, store).map(DeclValue::GridAreas),
        DeclGrammar::GridAutoFlow => parse_grid_auto_flow(value).map(DeclValue::GridAutoFlow),
        DeclGrammar::TrackSize => parse_track_size(value).map(DeclValue::TrackSize),
        DeclGrammar::GridLine => parse_grid_line(value, store).map(DeclValue::GridLine),
        DeclGrammar::TransitionPropertyList => parse_transition_property_list(value, store),
        DeclGrammar::TransitionTimeList => parse_transition_time_list(value, store),
        DeclGrammar::TransitionSignedTimeList => parse_transition_signed_time_list(value, store),
        DeclGrammar::TransitionTimingList => parse_transition_timing_list(value, store),
    }
}

// — Length family — //

/// The `unset | 0 | <px> | <em>` unit core shared by every length grammar:
/// `non_negative` applies the sign rule to the px/em arms. Percentages are NOT
/// parsed here — each grammar admits or rejects the `<f32>%` arm separately
/// (see `parse_length`/`parse_non_negative_length_or_unset` for the admitting
/// grammars; `parse_line_width`/`parse_signed_line_width` reject it).
fn parse_unit_length(value: &str, non_negative: bool) -> Option<DeclLength> {
    match parse_authored_length(value)? {
        AuthoredLength::Unset => Some(DeclLength::Unset),
        AuthoredLength::Px(px) if !non_negative || px >= 0.0 => Some(DeclLength::Px(px)),
        AuthoredLength::Em(em) if !non_negative || em >= 0.0 => Some(DeclLength::Em(em)),
        _ => None,
    }
}

/// Parses a `<f32>%` percentage scalar (finite-only). The trailing `%` is the
/// percentage marker; anything else is left to the caller's unit grammar.
fn parse_percentage(value: &str) -> Option<f32> {
    let scalar = value.strip_suffix('%')?;
    let parsed = scalar.trim().parse::<f32>().ok()?;
    parsed.is_finite().then_some(parsed)
}

/// `unset | 0 | <px> | <em> | <%>` — the signed length grammar (margins,
/// insets): negative px/em and negative percentages are admitted.
fn parse_length(value: &str) -> Option<DeclLength> {
    parse_unit_length(value, false).or_else(|| parse_percentage(value).map(DeclLength::Percent))
}

/// `unset | 0 | <non-negative px> | <non-negative em> | <non-negative %>` —
/// the sign policy for non-negative length properties (padding, box sizes,
/// font-size; CSS rejects negatives here, including negative percentages).
fn parse_non_negative_length_or_unset(value: &str) -> Option<DeclLength> {
    parse_unit_length(value, true).or_else(|| {
        let percentage = parse_percentage(value)?;
        (percentage >= 0.0).then_some(DeclLength::Percent(percentage))
    })
}

/// `unset | 0 | <non-negative px> | <non-negative em>` — the CSS `<line-width>`
/// grammar (CSS Backgrounds 3 §4.3, CSS UI 4 §4.1), which admits NO
/// percentages. Used by the four border widths, outline-width, and the four
/// border-radius longhands. For the radius corners, CSS Backgrounds 3 §5.1
/// admits `<length-percentage>`, but the circular-corner model
/// (`radius: [f32; 4]`, one radius per corner) cannot represent the elliptical
/// resolution CSS requires (50% on a non-square border box computes different
/// horizontal and vertical radii per corner): the percentage is rejected at
/// admission exactly like `gap` (see `parse_gap_longhand`), admission-gated on
/// a corner-pair (elliptical) model — never silently resolved.
fn parse_line_width(value: &str) -> Option<DeclLength> {
    parse_unit_length(value, true)
}

/// `unset | 0 | <px> | <em>` — outline-offset (CSS UI 4 §4.3): a signed CSS
/// `<length>` with NO percentages.
fn parse_signed_line_width(value: &str) -> Option<DeclLength> {
    parse_unit_length(value, false)
}

/// `normal | unset | 0 | <non-negative px> | <non-negative em>` (CSS Box
/// Alignment 3 §8.2: `normal` computes to zero, matching the shorthand).
///
/// Percentages are deliberately NOT admitted here even though
/// `parse_non_negative_length_or_unset` (which this delegates to) admits them.
/// This is the shared reject-at-admission rule (see the file header): a grammar
/// whose field has no percentage resolution target drops + counts the
/// percentage rather than admit-and-be-wrong. For gap specifically, the flex
/// gap field is `[f32; 2]` and gap percentage resolution against the flex
/// container's content box is out of scope for PR 1 item 7.
fn parse_gap_longhand(value: &str) -> Option<DeclLength> {
    if value == "normal" {
        return Some(DeclLength::Px(0.0));
    }
    match parse_non_negative_length_or_unset(value) {
        Some(DeclLength::Percent(_)) => None,
        other => other,
    }
}

fn parse_line_height(value: &str) -> Option<DeclValue> {
    if value == "normal" {
        return Some(DeclValue::Keyword(Keyword::Normal));
    }
    if let Some(number) = parse_non_negative_f32(value) {
        return Some(DeclValue::Number(number));
    }
    Some(DeclValue::Length(parse_non_negative_length(value)?))
}

fn parse_non_negative_length(value: &str) -> Option<DeclLength> {
    if let Some(authored) = parse_authored_length(value) {
        return match authored {
            AuthoredLength::Px(px) if px >= 0.0 => Some(DeclLength::Px(px)),
            AuthoredLength::Em(em) if em >= 0.0 => Some(DeclLength::Em(em)),
            _ => None,
        };
    }
    let percentage = parse_percentage(value)?;
    (percentage >= 0.0).then_some(DeclLength::Percent(percentage))
}

// — Number/integer family — //

fn parse_finite_f32(value: &str) -> Option<f32> {
    let parsed = value.trim().parse::<f32>().ok()?;
    parsed.is_finite().then_some(parsed)
}

fn parse_non_negative_f32(value: &str) -> Option<f32> {
    let parsed = value.trim().parse::<f32>().ok()?;
    (parsed.is_finite() && parsed >= 0.0).then_some(parsed)
}

fn parse_opacity(value: &str) -> Option<DeclValue> {
    let number = value.trim().parse::<f32>().ok()?;
    // CSS Color 4 §12.2: opacity clamps to [0, 1] at computed time — the
    // declaration is valid at any finite value and clamps, never drops.
    number
        .is_finite()
        .then(|| DeclValue::Number(number.clamp(0.0, 1.0)))
}

fn parse_non_negative_number(value: &str) -> Option<DeclValue> {
    parse_non_negative_f32(value).map(DeclValue::Number)
}

fn parse_integer(value: &str) -> Option<DeclValue> {
    value.trim().parse::<i64>().ok().map(DeclValue::Integer)
}

const FONT_WEIGHT_KEYWORDS: &[Keyword] = &[
    Keyword::Normal,
    Keyword::Bold,
    Keyword::Bolder,
    Keyword::Lighter,
];

fn parse_font_weight(value: &str) -> Option<DeclValue> {
    if let Some(keyword) = Keyword::from_name_in(FONT_WEIGHT_KEYWORDS, value) {
        return Some(DeclValue::Keyword(keyword));
    }
    let weight = value.parse::<i64>().ok()?;
    if (100..=900).contains(&weight) && weight % 100 == 0 {
        Some(DeclValue::Integer(weight))
    } else {
        None
    }
}

// — Color family — //

fn parse_color(value: &str) -> Option<DeclValue> {
    parse_css_color_rgba(value).ok().map(DeclValue::Color)
}

fn parse_color_rgba(value: &str) -> Option<[f32; 4]> {
    parse_css_color_rgba(value).ok()
}

// — Flex-basis family — //

fn parse_flex_basis_value(value: &str) -> Option<DeclValue> {
    match value {
        "auto" => Some(DeclValue::Keyword(Keyword::Auto)),
        "content" => Some(DeclValue::Keyword(Keyword::Content)),
        _ => parse_flex_basis_length(value).map(DeclValue::Length),
    }
}

fn parse_flex_basis_length(value: &str) -> Option<DeclLength> {
    match parse_authored_length(value)? {
        AuthoredLength::Px(px) if px >= 0.0 => Some(DeclLength::Px(px)),
        AuthoredLength::Em(em) if em >= 0.0 => Some(DeclLength::Em(em)),
        _ => None,
    }
}

// — Grid family — //

/// Parses a grid track list (`grid-template-columns`/`rows`): space-separated
/// `<track-size>` tokens with `[name]` line-name brackets and `repeat(…)`
/// (`repeat(<positive-integer>, <track-list>)` expanded at parse, and
/// `repeat(auto-fill|auto-fit, <track-list>)` stored as ONE copy plus a span
/// marker — T3.5). The result interns value-deduped into the owning store's
/// arena. Gates (each a counted drop, never a crash): a name over
/// `GRID_LINE_NAME_CAPACITY` registrations, a name over
/// `GRID_LINE_NAME_PER_NAME_CAPACITY` lines, a nested `repeat()`, a bare `fr`
/// track inside an auto-fill/fit span (the deferred content-based repetition
/// basis), a names-only (zero track-size) auto-repeat inner list (§7.2.3
/// requires ≥1 `<track-size>`), a non-positive/non-integer repeat count, an
/// empty/missing inner list, a second auto-repeat span, and an expanded count
/// over `GRID_TRACK_LIST_CAPACITY`. The ROW axis additionally gates
/// content-based span tracks (`min-content`/`max-content`/`auto`) — see
/// `parse_row_track_list` (D1/NB1).
fn parse_track_list(value: &str, store: &mut DeclStore) -> Option<GridTrackListKey> {
    parse_track_list_axis(value, store, false)
}

/// Parses a ROW-axis track list (`grid-template-rows`), which additionally
/// gates content-based auto-repeat span tracks (their inline-axis repetition
/// basis is wrong for the block axis — D1/NB1).
fn parse_row_track_list(value: &str, store: &mut DeclStore) -> Option<GridTrackListKey> {
    parse_track_list_axis(value, store, true)
}

/// The shared track-list parser. `is_row_axis` selects the row-axis gate for
/// content-based auto-repeat span tracks; column-axis lists keep the content
/// bases (`h_min`/`h_max` are inline-axis, the correct axis for columns).
fn parse_track_list_axis(
    value: &str,
    store: &mut DeclStore,
    is_row_axis: bool,
) -> Option<GridTrackListKey> {
    let mut list = GridTrackList::default();
    expand_track_list_tokens(value, store, &mut list, is_row_axis)?;
    Some(store.intern_track_list(&list))
}

/// The repeat-count forms `split_repeat_inner` returns.
enum RepeatCount {
    Count(usize),
    AutoFill,
    AutoFit,
}

/// Expands one space-separated track-list token slice into `out`, failing the
/// whole list on any gate (the `?`-propagated `None`). `[name1 name2]` bracket
/// groups register their names against the line at the current `tracks.len()`.
/// A fixed `repeat(N, list)` expands to N copies; an `repeat(auto-fill|auto-fit,
/// list)` stores the inner list as ONE copy plus `auto_repeat_span` and the
/// span's OWNED line-name range (which of the registrations are span-inner vs
/// suffix — B3). `is_row_axis` gates content-based span tracks on the block
/// axis (D1/NB1).
fn expand_track_list_tokens(
    value: &str,
    store: &mut DeclStore,
    out: &mut GridTrackList,
    is_row_axis: bool,
) -> Option<()> {
    for token in split_ws_paren_aware(value) {
        if token.starts_with('[') {
            register_line_names(token, store, out)?;
            continue;
        }
        if let Some(inner) = strip_repeat(token) {
            let (count, list) = split_repeat_inner(inner)?;
            if list.contains("repeat(") {
                return None; // nested repeat — CSS forbids it
            }
            match count {
                RepeatCount::Count(n) => {
                    for _ in 0..n {
                        for inner_token in split_ws_paren_aware(list) {
                            if inner_token.starts_with('[') {
                                register_line_names(inner_token, store, out)?;
                            } else {
                                out.tracks.try_push(parse_track_size(inner_token)?).ok()?;
                            }
                        }
                    }
                }
                kind @ (RepeatCount::AutoFill | RepeatCount::AutoFit) => {
                    // CSS Grid §7.2.3 admits at most ONE auto-repeat per list; a
                    // second one has no span field to carry it, so it drops.
                    if out.auto_repeat_span.is_some() {
                        return None;
                    }
                    let start = u8::try_from(out.tracks.len()).ok()?;
                    let span_start_len = out.tracks.len();
                    let owned_start = u8::try_from(out.line_names.len()).ok()?;
                    for inner_token in split_ws_paren_aware(list) {
                        if inner_token.starts_with('[') {
                            register_line_names(inner_token, store, out)?;
                        } else {
                            let track = parse_track_size(inner_token)?;
                            if is_bare_fr(track) {
                                // A bare `fr` span track has a zero repetition
                                // basis (the deferred content-based repetition
                                // basis) — drop + count at parse.
                                return None;
                            }
                            if is_row_axis && span_track_basis_is_content_based(track) {
                                // Row-axis content-based span tracks
                                // (`min-content`/`max-content`/`auto`) would
                                // count from INLINE-axis contributions — a
                                // wrong-axis count is worse than a gate (D1/NB1).
                                return None;
                            }
                            out.tracks.try_push(track).ok()?;
                        }
                    }
                    let count = u8::try_from(out.tracks.len() - span_start_len).ok()?;
                    // §7.2.3's `<auto-repeat>` requires ≥1 `<track-size>`: a
                    // names-only inner list (`repeat(auto-fill, [x])`) has zero
                    // track sizes and would divide by zero at layout — drop +
                    // count at parse (B1).
                    if count == 0 {
                        return None;
                    }
                    let owned_end = u8::try_from(out.line_names.len()).ok()?;
                    let grid_repeat = match kind {
                        RepeatCount::AutoFill => GridRepeat::AutoFill,
                        RepeatCount::AutoFit => GridRepeat::AutoFit,
                        RepeatCount::Count(_) => unreachable!("matched the auto arm"),
                    };
                    out.auto_repeat_span = Some((start, count, grid_repeat));
                    out.owned_line_name_range = Some((owned_start, owned_end));
                }
            }
        } else {
            out.tracks.try_push(parse_track_size(token)?).ok()?;
        }
    }
    Some(())
}

/// True when a parsed track is a bare `fr` leaf (`{ Auto, Fr(_) }`) — the
/// auto-fill/fit span's deferred-admission gate. A `fr` MAX inside `minmax`
/// (the idiom `minmax(240px, 1fr)`) has a non-`Auto` min bound and is admitted.
fn is_bare_fr(track: GridTrackSize) -> bool {
    track.min == TrackBreadth::Auto && matches!(track.max, TrackMaxBreadth::Fr(_))
}

/// True when a span track's repetition basis reads the item content
/// contributions (`h_min`/`h_max`) rather than a definite length. Only a bare
/// `min-content`/`max-content`/`auto` leaf has such a min bound (a `minmax`
/// min is always px/percent), so a ROW-axis auto-repeat span carrying one is
/// wrong-axis and drops at parse (D1/NB1). Column-axis content bases are
/// correct-axis (inline) and admitted.
fn span_track_basis_is_content_based(track: GridTrackSize) -> bool {
    matches!(
        track.min,
        TrackBreadth::MinContent | TrackBreadth::MaxContent | TrackBreadth::Auto
    )
}

/// Registers one `[name1 name2]` bracket group's names against the line at the
/// current `tracks.len()` (the line BEFORE the following track; a trailing
/// bracket names the final line). Fails the whole list on any capacity or
/// per-name-count gate.
fn register_line_names(
    bracket: &str,
    store: &mut DeclStore,
    out: &mut GridTrackList,
) -> Option<()> {
    let inner = bracket.strip_prefix('[')?.strip_suffix(']')?;
    if inner.trim().is_empty() {
        return None;
    }
    let line_index = u8::try_from(out.tracks.len()).ok()?;
    for name in inner.split_whitespace() {
        if !is_custom_ident(name) {
            return None;
        }
        let key = store.intern(name);
        if !out.register_line_name(key, line_index) {
            return None;
        }
    }
    Some(())
}

/// True when `value` is a valid grid `<custom-ident>`: a non-empty identifier
/// whose first character is alphabetic (or `-`/`_`) and whose remainder is
/// alphanumeric/`-`/`_`, and NOT a signed integer (`-1` is a line, never a
/// name). The CSS-wide keywords (`initial`, `inherit`, `unset`, `revert`,
/// `revert-layer` — CSS Values 4 §3.2), `span` (CSS Grid §7.1), and `auto`
/// (the grid shorthand's keyword, not a name) are rejected here so they never
/// become names in area cells, `[name]` line names, or the lone-ident
/// `grid-area` branch; the CSS-wide keywords then fall through to the css-wide
/// value path upstream rather than becoming area/line state.
fn is_custom_ident(value: &str) -> bool {
    if matches!(
        value,
        "span" | "initial" | "inherit" | "unset" | "revert" | "revert-layer" | "auto"
    ) {
        return false;
    }
    if value.parse::<i64>().is_ok() {
        return false;
    }
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_' || first == '-') {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

/// Strips a `repeat(...)` call, returning the inner `count, <track-list>` text.
fn strip_repeat(token: &str) -> Option<&str> {
    let inner = token.strip_prefix("repeat(")?;
    inner.strip_suffix(')')
}

/// Splits a `repeat` inner into `(count, track-list text)`. Rejects a
/// non-positive or non-integer count, an empty/missing list, and a
/// `repeat(...)` with no comma (the bare `repeat(auto-fill)` form). The count
/// is one of `Count(usize)` / `AutoFill` / `AutoFit`.
fn split_repeat_inner(inner: &str) -> Option<(RepeatCount, &str)> {
    let (count_text, list) = inner.split_once(',')?;
    let count_text = count_text.trim();
    let count = match count_text {
        "auto-fill" => RepeatCount::AutoFill,
        "auto-fit" => RepeatCount::AutoFit,
        _ => {
            let count = count_text.parse::<usize>().ok()?;
            if count == 0 {
                return None;
            }
            RepeatCount::Count(count)
        }
    };
    let list = list.trim();
    if list.is_empty() {
        return None;
    }
    Some((count, list))
}

/// Parses one grid `<track-size>` leaf into the uniform minmax shape
/// (`GRID_STEP2_DESIGN.md` correction 3): `auto | min-content | max-content |
/// <px> | <%> | <fr> | minmax(<len-pct>, <len-pct>|min-content|max-content|auto)`.
/// `em` is not admitted (the `Px` breadth carries a resolved px value and no
/// parse-time font size exists); it drops fail-closed. `auto` is admitted as a
/// minmax MAX bound (correction 2 — CSS Grid §7.2's `<track-breadth>`).
/// Negative `fr` and `%` leaves drop fail-closed (CSS Grid §7.1: `<flex>` is
/// non-negative and negative track sizes are invalid — the px arm's `px >= 0.0`
/// gate generalized to the fr/percent arms and both minmax bounds).
fn parse_track_size(token: &str) -> Option<GridTrackSize> {
    match token {
        "auto" => {
            return Some(GridTrackSize {
                min: TrackBreadth::Auto,
                max: TrackMaxBreadth::Auto,
            });
        }
        "min-content" => {
            return Some(GridTrackSize {
                min: TrackBreadth::MinContent,
                max: TrackMaxBreadth::MinContent,
            });
        }
        "max-content" => {
            return Some(GridTrackSize {
                min: TrackBreadth::MaxContent,
                max: TrackMaxBreadth::MaxContent,
            });
        }
        _ => {}
    }
    if let Some(fr) = token.strip_suffix("fr") {
        // CSS Grid §7.1: `<flex>` is a non-negative `<number>`; a negative fr
        // drops + counts exactly like the px arm's `px >= 0.0` gate.
        let value = parse_non_negative_f32(fr)?;
        return Some(GridTrackSize {
            min: TrackBreadth::Auto,
            max: TrackMaxBreadth::Fr(value),
        });
    }
    if let Some(percent) = token.strip_suffix('%') {
        // CSS Grid §7.1 rejects negative track sizes; the leaf percent gate
        // mirrors the px arm (negative percentages drop + count).
        let value = parse_non_negative_f32(percent)?;
        return Some(GridTrackSize {
            min: TrackBreadth::Percent(value),
            max: TrackMaxBreadth::Percent(value),
        });
    }
    if let Some(inner) = strip_minmax(token) {
        return parse_minmax(inner);
    }
    match parse_authored_length(token)? {
        AuthoredLength::Px(px) if px >= 0.0 => Some(GridTrackSize {
            min: TrackBreadth::Px(px),
            max: TrackMaxBreadth::Px(px),
        }),
        _ => None,
    }
}

/// Strips a `minmax(...)` call, returning the inner `A, B` text.
fn strip_minmax(token: &str) -> Option<&str> {
    let inner = token.strip_prefix("minmax(")?;
    inner.strip_suffix(')')
}

/// Parses a `minmax(<len-pct>, <len-pct>|min-content|max-content|auto)` inner
/// into a `GridTrackSize`. The min bound is px/percent; the max bound is
/// px/percent/`min-content`/`max-content`/`auto` (correction 2).
fn parse_minmax(inner: &str) -> Option<GridTrackSize> {
    let (min_text, max_text) = inner.split_once(',')?;
    let min = parse_minmax_min_bound(min_text.trim())?;
    let max = parse_minmax_max_bound(max_text.trim())?;
    Some(GridTrackSize { min, max })
}

/// Parses the `minmax` first bound (`<length-percentage>`) as a `TrackBreadth`,
/// preserving the percent-vs-px unit (correction 3).
fn parse_minmax_min_bound(text: &str) -> Option<TrackBreadth> {
    if let Some(percent) = text.strip_suffix('%') {
        // CSS Grid §7.1 rejects negative track sizes; the min bound drops the
        // negative percentage like the px arm (`px >= 0.0`).
        return parse_non_negative_f32(percent).map(TrackBreadth::Percent);
    }
    match parse_authored_length(text)? {
        AuthoredLength::Px(px) if px >= 0.0 => Some(TrackBreadth::Px(px)),
        _ => None,
    }
}

/// Parses the `minmax` second bound: px, percent, `min-content`, `max-content`,
/// `auto`, or `<flex>` (correction 2 — `auto` is a valid `<track-breadth>` max,
/// and a `fr` MAX is the T3.5 card-wall idiom `minmax(240px, 1fr)`).
fn parse_minmax_max_bound(text: &str) -> Option<TrackMaxBreadth> {
    match text {
        "auto" => Some(TrackMaxBreadth::Auto),
        "min-content" => Some(TrackMaxBreadth::MinContent),
        "max-content" => Some(TrackMaxBreadth::MaxContent),
        _ => {
            if let Some(fr) = text.strip_suffix("fr") {
                // CSS Grid §7.1: `<flex>` is a non-negative `<number>`; the
                // negative-fr gate mirrors the leaf `fr` arm.
                return parse_non_negative_f32(fr).map(TrackMaxBreadth::Fr);
            }
            if let Some(percent) = text.strip_suffix('%') {
                // CSS Grid §7.1 rejects negative track sizes; the max bound
                // drops the negative percentage like the px arm.
                return parse_non_negative_f32(percent).map(TrackMaxBreadth::Percent);
            }
            match parse_authored_length(text)? {
                AuthoredLength::Px(px) if px >= 0.0 => Some(TrackMaxBreadth::Px(px)),
                _ => None,
            }
        }
    }
}

/// Parses `grid-auto-flow`: `[row | column] && dense?` — one or two tokens, any
/// order; `dense` alone defaults to row. Maps to the four `GridAutoFlowKw`.
fn parse_grid_auto_flow(value: &str) -> Option<GridAutoFlowKw> {
    let tokens = split_ws_paren_aware(value);
    let mut flow = Keyword::Row;
    let mut dense = false;
    let mut saw_direction = false;
    for token in tokens {
        match token {
            "row" => {
                if saw_direction {
                    return None;
                }
                flow = Keyword::Row;
                saw_direction = true;
            }
            "column" => {
                if saw_direction {
                    return None;
                }
                flow = Keyword::Column;
                saw_direction = true;
            }
            "dense" => {
                if dense {
                    return None;
                }
                dense = true;
            }
            _ => return None,
        }
    }
    Some(match (flow, dense) {
        (Keyword::Row, false) => GridAutoFlowKw::Row,
        (Keyword::Row, true) => GridAutoFlowKw::RowDense,
        (Keyword::Column, false) => GridAutoFlowKw::Column,
        (Keyword::Column, true) => GridAutoFlowKw::ColumnDense,
        _ => unreachable!("the flow token is only ever Row or Column"),
    })
}

/// Parses one `<grid-line>`: `auto | <nonzero-integer> | span <positive-integer>
/// | <custom-ident> | span <custom-ident> [integer]` (CSS Grid §7.1;
/// `GRID_STEP3_DESIGN.md` D1 + `GRID_STEP4_DESIGN.md` D2). Gates (each a counted
/// drop, never a crash): the integer line `0` (zero is not a line), `span 0`/
/// `span -1` (a span is positive), bare `span` (no integer/name), and any
/// invalid `<custom-ident>` or multi-token form. Negatives are admitted for
/// `<integer>` and translated at placement (D2). Names intern through `store`.
fn parse_grid_line(value: &str, store: &mut DeclStore) -> Option<GridLine> {
    let tokens = split_ws_paren_aware(value);
    match tokens.as_slice() {
        ["auto"] => Some(GridLine::Auto),
        ["span"] => None, // bare `span` (no integer/name) drops + counts
        ["span", n] => {
            if let Some(count) = parse_positive_line_integer(n) {
                Some(GridLine::Span(count))
            } else if is_custom_ident(n) {
                Some(GridLine::NamedSpan(store.intern(n), 0))
            } else {
                None
            }
        }
        ["span", name, n] => {
            if is_custom_ident(name) && parse_positive_line_integer(n).is_some() {
                Some(GridLine::NamedSpan(
                    store.intern(name),
                    parse_positive_line_integer(n)?,
                ))
            } else {
                None
            }
        }
        [n] => {
            if let Some(line) = parse_nonzero_line_integer(n) {
                Some(GridLine::Line(line))
            } else if is_custom_ident(n) {
                Some(GridLine::Named(store.intern(n)))
            } else {
                None
            }
        }
        _ => None, // multi-token forms drop + count
    }
}

/// Parses a `<grid-line>` integer line, rejecting zero (zero is not a line,
/// CSS Grid §7.1).
fn parse_nonzero_line_integer(value: &str) -> Option<i32> {
    let parsed = value.trim().parse::<i32>().ok()?;
    (parsed != 0).then_some(parsed)
}

/// Parses a `span <integer>` count, requiring a positive integer (zero and
/// negative drop + count).
fn parse_positive_line_integer(value: &str) -> Option<u32> {
    let parsed = value.trim().parse::<i32>().ok()?;
    if parsed <= 0 {
        return None;
    }
    u32::try_from(parsed).ok()
}

// — Grid areas family — //

/// Parses `grid-template-areas: none | <string>+` into a validated, interned
/// `GridTemplateAreas` map (`GRID_STEP4_DESIGN.md` D1). `none` is the CSS
/// initial value: it returns the empty-map sentinel (`GridAreasKey::default()`)
/// — never a dropped declaration — so re-setting the longhand to `none` clears
/// any prior areas. Gates (each a counted drop, never a crash): > 8 rows, > 12
/// columns, > 12 distinct names, a non-uniform column count, a non-rectangular
/// name, and any malformed cell token.
fn parse_grid_areas(value: &str, store: &mut DeclStore) -> Option<GridAreasKey> {
    // `none` is matched ASCII case-insensitively (CSS 2.1 §4.1.3) — the
    // spec-correct entry point for the initial-value reset, so `NONE`/`None`
    // admit identically. The REST of the grammar's keyword/unit matching stays
    // case-sensitive; that broader divergence is registered separately
    // (`off_spec_keyword_grammars_are_case_sensitive`, layout_proofs/README.md).
    if value.trim().eq_ignore_ascii_case("none") {
        return Some(GridAreasKey::default());
    }
    let rows = split_grid_area_rows(value)?;
    if rows.is_empty() || rows.len() > GRID_AREA_ROW_CAPACITY {
        return None;
    }

    let mut cells: ArrayVec<
        ArrayVec<Option<TextKey>, GRID_AREA_COL_CAPACITY>,
        GRID_AREA_ROW_CAPACITY,
    > = ArrayVec::new();
    let mut col_count: Option<usize> = None;
    for row in &rows {
        let row_cells = parse_area_row(row, store)?;
        match col_count {
            None => col_count = Some(row_cells.len()),
            Some(count) if count != row_cells.len() => return None, // non-uniform columns
            Some(_) => {}
        }
        cells.push(row_cells);
    }
    let col_count = col_count.unwrap_or(0);

    // Deduped name → bounding rect, in first-appearance order.
    let mut names: ArrayVec<(TextKey, AreaRect), GRID_AREA_NAME_CAPACITY> = ArrayVec::new();
    for (row_idx, row_cells) in cells.iter().enumerate() {
        for (col_idx, cell) in row_cells.iter().enumerate() {
            let Some(name_key) = cell else {
                continue;
            };
            let row = u8::try_from(row_idx).expect("area row index fits u8");
            let col = u8::try_from(col_idx).expect("area column index fits u8");
            if let Some((_, rect)) = names.iter_mut().find(|(key, _)| key == name_key) {
                rect.min_row = rect.min_row.min(row);
                rect.min_col = rect.min_col.min(col);
                rect.max_row = rect.max_row.max(row);
                rect.max_col = rect.max_col.max(col);
            } else {
                if names.len() >= GRID_AREA_NAME_CAPACITY {
                    return None; // > 12 distinct names
                }
                names.push((
                    *name_key,
                    AreaRect {
                        min_row: row,
                        min_col: col,
                        max_row: row,
                        max_col: col,
                    },
                ));
            }
        }
    }

    // Rectangularity: each name's cell count must equal its bounding-rect area
    // (a name that does not tile a rectangle has fewer cells than its rect).
    for (name_key, rect) in &names {
        let area = usize::from(rect.max_row - rect.min_row + 1)
            * usize::from(rect.max_col - rect.min_col + 1);
        let mut count = 0usize;
        for row_cells in &cells {
            for cell in row_cells {
                if cell == &Some(*name_key) {
                    count += 1;
                }
            }
        }
        if count != area {
            return None;
        }
    }

    Some(store.intern_grid_areas(&GridTemplateAreas {
        cells,
        names,
        row_count: u8::try_from(rows.len()).ok()?,
        col_count: u8::try_from(col_count).ok()?,
    }))
}

/// Splits a `<string>+` value into its unquoted row strings. Each row must be a
/// single `"…"` token (quotes stripped); anything else fails.
fn split_grid_area_rows(value: &str) -> Option<Vec<&str>> {
    let mut rows = Vec::new();
    let mut cursor = value.trim();
    while !cursor.is_empty() {
        let rest = cursor.strip_prefix('"')?;
        let end = rest.find('"')?;
        rows.push(&rest[..end]);
        cursor = rest[end + 1..].trim_start();
    }
    Some(rows)
}

/// Parses one areas row string into its cell tokens: a dot-run → `None` (empty
/// cell), a `<custom-ident>` → `Some(interned name)`. Over `GRID_AREA_COL_CAPACITY`
/// cells or any malformed token fails the whole declaration.
fn parse_area_row(
    row: &str,
    store: &mut DeclStore,
) -> Option<ArrayVec<Option<TextKey>, GRID_AREA_COL_CAPACITY>> {
    let mut cells = ArrayVec::<Option<TextKey>, GRID_AREA_COL_CAPACITY>::new();
    for token in row.split_whitespace() {
        if cells.len() >= GRID_AREA_COL_CAPACITY {
            return None;
        }
        if token.bytes().all(|byte| byte == b'.') {
            cells.push(None);
        } else if is_custom_ident(token) {
            cells.push(Some(store.intern(token)));
        } else {
            return None;
        }
    }
    Some(cells)
}

// — Text/transform family — //

fn parse_transform(value: &str, store: &mut DeclStore) -> Option<DeclValue> {
    if value.trim() == "none" {
        return Some(DeclValue::Keyword(Keyword::None));
    }
    if parse_translate_text(value) {
        Some(DeclValue::Text(store.intern(value)))
    } else {
        None
    }
}

fn parse_text(value: &str, store: &mut DeclStore) -> Option<DeclValue> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(DeclValue::Text(store.intern(trimmed)))
}

/// Validates the closed translate grammar: the whole trimmed string must be
/// exactly `translateX(<num>[px])`, `translateY(<num>[px])`, or
/// `translate(<num>[px], <num>[px])`. Matches the consumption grammar in
/// `src/domains/layout/style.rs` but strict about trailing garbage.
fn parse_translate_text(value: &str) -> bool {
    let trimmed = value.trim();
    if let Some(inner) = strip_function_call(trimmed, "translateX") {
        return parse_px_number(inner).is_some();
    }
    if let Some(inner) = strip_function_call(trimmed, "translateY") {
        return parse_px_number(inner).is_some();
    }
    let Some(inner) = strip_function_call(trimmed, "translate") else {
        return false;
    };
    parse_translate_pair(inner)
}

fn strip_function_call<'a>(value: &'a str, name: &str) -> Option<&'a str> {
    let inner = value.strip_prefix(name)?;
    let inner = inner.strip_prefix('(')?;
    inner.strip_suffix(')')
}

fn parse_translate_pair(inner: &str) -> bool {
    let mut parts = inner.split(',');
    let Some(first) = parts.next() else {
        return false;
    };
    let Some(second) = parts.next() else {
        return false;
    };
    if parts.next().is_some() {
        return false;
    }
    parse_px_number(first).is_some() && parse_px_number(second).is_some()
}

fn parse_px_number(value: &str) -> Option<f32> {
    let trimmed = value.trim();
    let number = trimmed.strip_suffix("px").unwrap_or(trimmed);
    parse_finite_f32(number)
}

// — Transition list family — //

fn comma_split_trim(value: &str) -> Option<Vec<&str>> {
    let items: Vec<&str> = value.split(',').map(str::trim).collect();
    if items.len() > crate::domains::style::computed::TRANSITION_LONGHAND_CAPACITY
        || items.iter().any(|item| item.is_empty())
    {
        return None;
    }
    Some(items)
}

fn parse_transition_property_list(value: &str, store: &mut DeclStore) -> Option<DeclValue> {
    let items = comma_split_trim(value)?;
    for &item in &items {
        if item == "all" || item == "none" {
            continue;
        }
        PresentationProperty::from_name(item)?;
    }
    Some(DeclValue::Text(store.intern(value)))
}

fn parse_transition_time_list(value: &str, store: &mut DeclStore) -> Option<DeclValue> {
    parse_transition_time_list_impl(value, store, false)
}

fn parse_transition_signed_time_list(value: &str, store: &mut DeclStore) -> Option<DeclValue> {
    parse_transition_time_list_impl(value, store, true)
}

fn parse_transition_time_list_impl(
    value: &str,
    store: &mut DeclStore,
    allow_negative: bool,
) -> Option<DeclValue> {
    let items = comma_split_trim(value)?;
    for item in &items {
        if !parse_time(item, allow_negative) {
            return None;
        }
    }
    Some(DeclValue::Text(store.intern(value)))
}

fn parse_time(value: &str, allow_negative: bool) -> bool {
    if value == "0" {
        return true;
    }
    if let Some(milliseconds) = value.strip_suffix("ms") {
        return parse_time_number(milliseconds, allow_negative).is_some();
    }
    if let Some(seconds) = value.strip_suffix("s") {
        return parse_time_number(seconds, allow_negative).is_some();
    }
    false
}

fn parse_time_number(value: &str, allow_negative: bool) -> Option<f32> {
    let parsed = value.trim().parse::<f32>().ok()?;
    (parsed.is_finite() && (allow_negative || parsed >= 0.0)).then_some(parsed)
}

const TRANSITION_TIMING_KEYWORDS: &[Keyword] = &[
    Keyword::Linear,
    Keyword::Ease,
    Keyword::EaseIn,
    Keyword::EaseOut,
    Keyword::EaseInOut,
];

fn parse_transition_timing_list(value: &str, store: &mut DeclStore) -> Option<DeclValue> {
    let items = comma_split_trim(value)?;
    for item in &items {
        Keyword::from_name_in(TRANSITION_TIMING_KEYWORDS, item)?;
    }
    Some(DeclValue::Text(store.intern(value)))
}

// — Shared shorthand helpers — //

/// Reads a `Keywords` grammar off a property that is known to use one. A
/// self-owned structural invariant: these properties always declare a
/// `Keywords` grammar in the presentation registry.
fn keyword_set(property: PresentationProperty) -> &'static [Keyword] {
    match property.grammar() {
        DeclGrammar::Keywords(set) => set,
        _ => unreachable!("{property:?} must use the Keywords grammar"),
    }
}

/// Splits on whitespace only at paren/bracket depth 0, so `rgb(255, 0, 0)`
/// stays one token even when it contains spaces, and a `[name1 name2]` line-name
/// bracket stays one token.
fn split_ws_paren_aware(value: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut depth = 0usize;
    let mut token_start = 0usize;
    let mut in_token = false;
    for (index, ch) in value.char_indices() {
        if ch == '(' || ch == '[' {
            depth = depth.saturating_add(1);
            if !in_token {
                token_start = index;
                in_token = true;
            }
            continue;
        }
        if ch == ')' || ch == ']' {
            depth = depth.saturating_sub(1);
            if !in_token {
                token_start = index;
                in_token = true;
            }
            continue;
        }
        if ch.is_whitespace() && depth == 0 {
            if in_token {
                tokens.push(value[token_start..index].trim());
                in_token = false;
            }
            continue;
        }
        if !in_token {
            token_start = index;
            in_token = true;
        }
    }
    if in_token {
        tokens.push(value[token_start..].trim());
    }
    tokens
}

/// The CSS 1–4 value corner mapping: [a]→(a,a,a,a); [a,b]→(a,b,a,b);
/// [a,b,c]→(a,b,c,b); [a,b,c,d]→(a,b,c,d). Anything else is malformed.
fn corner_quad<T: Copy>(items: &[T]) -> Option<(T, T, T, T)> {
    match items {
        [a] => Some((*a, *a, *a, *a)),
        [a, b] => Some((*a, *b, *a, *b)),
        [a, b, c] => Some((*a, *b, *c, *b)),
        [a, b, c, d] => Some((*a, *b, *c, *d)),
        _ => None,
    }
}

fn parse_token_list<T>(value: &str, parse: impl Fn(&str) -> Option<T>) -> Option<Vec<T>> {
    split_ws_paren_aware(value)
        .iter()
        .map(|token| parse(token))
        .collect()
}

/// Parses one expanded token through the target longhand's grammar, so the
/// sign policy (and any future grammar rule) cannot be bypassed via a
/// shorthand. All four radius corners share one grammar, as do both gaps,
/// the four margins (`Length`, signed), and the four paddings
/// (`NonNegativeLength`); the representative property is named for
/// readability only.
fn parse_length_for(property: PresentationProperty, value: &str) -> Option<DeclLength> {
    let mut scratch = DeclStore::default();
    match parse_value(property.grammar(), value, &mut scratch)? {
        DeclValue::Length(length) => Some(length),
        _ => None,
    }
}

// — Shorthand expansions — //

fn expand_border_radius(value: &str) -> Option<Vec<Declaration>> {
    let lengths = parse_token_list(value, |token| {
        parse_length_for(PresentationProperty::BorderTopLeftRadius, token)
    })?;
    let (tl, tr, br, bl) = corner_quad(&lengths)?;
    Some(vec![
        declaration(
            PresentationProperty::BorderTopLeftRadius,
            DeclValue::Length(tl),
        ),
        declaration(
            PresentationProperty::BorderTopRightRadius,
            DeclValue::Length(tr),
        ),
        declaration(
            PresentationProperty::BorderBottomRightRadius,
            DeclValue::Length(br),
        ),
        declaration(
            PresentationProperty::BorderBottomLeftRadius,
            DeclValue::Length(bl),
        ),
    ])
}

fn expand_margin(value: &str) -> Option<Vec<Declaration>> {
    let values = parse_token_list(value, |token| {
        if token == "auto" {
            Some(DeclValue::Keyword(Keyword::Auto))
        } else {
            parse_length_for(PresentationProperty::MarginTop, token).map(DeclValue::Length)
        }
    })?;
    let (top, right, bottom, left) = corner_quad(&values)?;
    Some(vec![
        declaration(PresentationProperty::MarginTop, top),
        declaration(PresentationProperty::MarginRight, right),
        declaration(PresentationProperty::MarginBottom, bottom),
        declaration(PresentationProperty::MarginLeft, left),
    ])
}

fn expand_padding(value: &str) -> Option<Vec<Declaration>> {
    let lengths = parse_token_list(value, |token| {
        parse_length_for(PresentationProperty::PaddingTop, token)
    })?;
    let (top, right, bottom, left) = corner_quad(&lengths)?;
    Some(vec![
        declaration(PresentationProperty::PaddingTop, DeclValue::Length(top)),
        declaration(PresentationProperty::PaddingRight, DeclValue::Length(right)),
        declaration(
            PresentationProperty::PaddingBottom,
            DeclValue::Length(bottom),
        ),
        declaration(PresentationProperty::PaddingLeft, DeclValue::Length(left)),
    ])
}

fn expand_border_color(value: &str) -> Option<Vec<Declaration>> {
    let colors = parse_token_list(value, parse_color_rgba)?;
    let (top, right, bottom, left) = corner_quad(&colors)?;
    Some(vec![
        declaration(PresentationProperty::BorderTopColor, DeclValue::Color(top)),
        declaration(
            PresentationProperty::BorderRightColor,
            DeclValue::Color(right),
        ),
        declaration(
            PresentationProperty::BorderBottomColor,
            DeclValue::Color(bottom),
        ),
        declaration(
            PresentationProperty::BorderLeftColor,
            DeclValue::Color(left),
        ),
    ])
}

fn expand_gap(value: &str) -> Option<Vec<Declaration>> {
    let gaps = parse_token_list(value, parse_gap_length)?;
    match gaps.as_slice() {
        [row] => Some(vec![
            declaration(PresentationProperty::RowGap, DeclValue::Length(*row)),
            declaration(PresentationProperty::ColumnGap, DeclValue::Length(*row)),
        ]),
        [row, column] => Some(vec![
            declaration(PresentationProperty::RowGap, DeclValue::Length(*row)),
            declaration(PresentationProperty::ColumnGap, DeclValue::Length(*column)),
        ]),
        _ => None,
    }
}

fn parse_gap_length(value: &str) -> Option<DeclLength> {
    if value == "normal" {
        return Some(DeclLength::Px(0.0));
    }
    parse_length_for(PresentationProperty::RowGap, value)
}

fn expand_flex(value: &str) -> Option<Vec<Declaration>> {
    match value {
        "none" => {
            return Some(flex_declarations(
                0.0,
                0.0,
                DeclValue::Keyword(Keyword::Auto),
            ));
        }
        "auto" => {
            return Some(flex_declarations(
                1.0,
                1.0,
                DeclValue::Keyword(Keyword::Auto),
            ));
        }
        "initial" => {
            return Some(flex_declarations(
                0.0,
                1.0,
                DeclValue::Keyword(Keyword::Auto),
            ));
        }
        _ => {}
    }
    let tokens = split_ws_paren_aware(value);
    match tokens.as_slice() {
        [single] => {
            if let Some(grow) = parse_non_negative_f32(single) {
                Some(flex_declarations(
                    grow,
                    1.0,
                    DeclValue::Length(DeclLength::Px(0.0)),
                ))
            } else {
                let basis = parse_flex_basis_value(single)?;
                Some(flex_declarations(1.0, 1.0, basis))
            }
        }
        [first, second] => {
            let grow = parse_non_negative_f32(first)?;
            if let Some(shrink) = parse_non_negative_f32(second) {
                Some(flex_declarations(
                    grow,
                    shrink,
                    DeclValue::Length(DeclLength::Px(0.0)),
                ))
            } else {
                let basis = parse_flex_basis_value(second)?;
                Some(flex_declarations(grow, 1.0, basis))
            }
        }
        [first, second, third] => {
            let grow = parse_non_negative_f32(first)?;
            let shrink = parse_non_negative_f32(second)?;
            let basis = parse_flex_basis_value(third)?;
            Some(flex_declarations(grow, shrink, basis))
        }
        _ => None,
    }
}

fn flex_declarations(grow: f32, shrink: f32, basis: DeclValue) -> Vec<Declaration> {
    vec![
        declaration(PresentationProperty::FlexGrow, DeclValue::Number(grow)),
        declaration(PresentationProperty::FlexShrink, DeclValue::Number(shrink)),
        declaration(PresentationProperty::FlexBasis, basis),
    ]
}

fn expand_flex_flow(value: &str) -> Option<Vec<Declaration>> {
    let tokens = split_ws_paren_aware(value);
    if tokens.is_empty() {
        return None;
    }
    let direction_set = keyword_set(PresentationProperty::FlexDirection);
    let wrap_set = keyword_set(PresentationProperty::FlexWrap);
    let mut direction = Keyword::Row;
    let mut wrap = Keyword::Nowrap;
    let mut saw_direction = false;
    let mut saw_wrap = false;
    for token in tokens {
        if let Some(found) = Keyword::from_name_in(direction_set, token) {
            if saw_direction {
                return None;
            }
            direction = found;
            saw_direction = true;
            continue;
        }
        if let Some(found) = Keyword::from_name_in(wrap_set, token) {
            if saw_wrap {
                return None;
            }
            wrap = found;
            saw_wrap = true;
            continue;
        }
        return None;
    }
    Some(vec![
        declaration(
            PresentationProperty::FlexDirection,
            DeclValue::Keyword(direction),
        ),
        declaration(PresentationProperty::FlexWrap, DeclValue::Keyword(wrap)),
    ])
}

fn expand_place_content(value: &str) -> Option<Vec<Declaration>> {
    let tokens = split_ws_paren_aware(value);
    let align_set = keyword_set(PresentationProperty::AlignContent);
    let justify_set = keyword_set(PresentationProperty::JustifyContent);
    let (align_token, justify_token) = match tokens.as_slice() {
        [align] => (*align, *align),
        [align, justify] => (*align, *justify),
        _ => return None,
    };
    let align = Keyword::from_name_in(align_set, align_token)?;
    let justify = Keyword::from_name_in(justify_set, justify_token)?;
    Some(vec![
        declaration(
            PresentationProperty::AlignContent,
            DeclValue::Keyword(align),
        ),
        declaration(
            PresentationProperty::JustifyContent,
            DeclValue::Keyword(justify),
        ),
    ])
}

fn expand_place_items(value: &str) -> Option<Vec<Declaration>> {
    let tokens = split_ws_paren_aware(value);
    let align_set = keyword_set(PresentationProperty::AlignItems);
    let justify_set = keyword_set(PresentationProperty::JustifyItems);
    let (align_token, justify_token) = match tokens.as_slice() {
        [align] => (*align, *align),
        [align, justify] => (*align, *justify),
        _ => return None,
    };
    let align = Keyword::from_name_in(align_set, align_token)?;
    let justify = Keyword::from_name_in(justify_set, justify_token)?;
    // The D1 correction: `place-items` expands to BOTH sides (align-items AND
    // justify-items), not just the align side.
    Some(vec![
        declaration(PresentationProperty::AlignItems, DeclValue::Keyword(align)),
        declaration(
            PresentationProperty::JustifyItems,
            DeclValue::Keyword(justify),
        ),
    ])
}

fn expand_place_self(value: &str) -> Option<Vec<Declaration>> {
    let tokens = split_ws_paren_aware(value);
    let align_set = keyword_set(PresentationProperty::AlignSelf);
    let justify_set = keyword_set(PresentationProperty::JustifySelf);
    let (align_token, justify_token) = match tokens.as_slice() {
        [align] => (*align, *align),
        [align, justify] => (*align, *justify),
        _ => return None,
    };
    let align = Keyword::from_name_in(align_set, align_token)?;
    let justify = Keyword::from_name_in(justify_set, justify_token)?;
    // The D1 correction: `place-self` expands to BOTH sides (align-self AND
    // justify-self), not just the align side.
    Some(vec![
        declaration(PresentationProperty::AlignSelf, DeclValue::Keyword(align)),
        declaration(
            PresentationProperty::JustifySelf,
            DeclValue::Keyword(justify),
        ),
    ])
}

/// Expands the `grid-column`/`grid-row` shorthand per D1: a lone `span N` (or
/// `span <name> [n]`) writes `Auto`/the span; a lone other value writes
/// value/`Auto`; two values write start/end in order.
fn expand_grid_placement_shorthand(
    value: &str,
    start_property: PresentationProperty,
    end_property: PresentationProperty,
    store: &mut DeclStore,
) -> Option<Vec<Declaration>> {
    let mut parts = value.split('/');
    let start_text = parts.next()?.trim();
    let end_text = parts.next().map(str::trim);
    if parts.next().is_some() {
        return None; // more than one '/'
    }
    let start = parse_grid_line(start_text, store)?;
    let (start_line, end_line) = match end_text {
        None => match start {
            GridLine::Span(n) => (GridLine::Auto, GridLine::Span(n)),
            GridLine::NamedSpan(name, n) => (GridLine::Auto, GridLine::NamedSpan(name, n)),
            other => (other, GridLine::Auto),
        },
        Some(end) => (start, parse_grid_line(end, store)?),
    };
    Some(vec![
        declaration(start_property, DeclValue::GridLine(start_line)),
        declaration(end_property, DeclValue::GridLine(end_line)),
    ])
}

fn expand_grid_column(value: &str, store: &mut DeclStore) -> Option<Vec<Declaration>> {
    expand_grid_placement_shorthand(
        value,
        PresentationProperty::GridColumnStart,
        PresentationProperty::GridColumnEnd,
        store,
    )
}

fn expand_grid_row(value: &str, store: &mut DeclStore) -> Option<Vec<Declaration>> {
    expand_grid_placement_shorthand(
        value,
        PresentationProperty::GridRowStart,
        PresentationProperty::GridRowEnd,
        store,
    )
}

/// Expands the `grid-area` shorthand per D2 with the cascade-correct
/// margin-shorthand model (adjudicated 2026-10-22, PR #27 review): a lone
/// `<custom-ident>` writes ALL FOUR placement longhands as `GridLine::Named(name)`
/// — exactly as `margin: 0 auto` expands to four margin longhands, so a later
/// placement longhand wins its own slot (`grid-area: header` then
/// `grid-column-start: 2`). `grid-area: auto` is NOT a lone ident (`auto` is
/// rejected by `is_custom_ident`) and falls through to the grid-line path,
/// which writes `GridLine::Auto` to all four longhands. 1–4 `<grid-line>`
/// values write the four placement longhands with the standard defaulting
/// table (`row-start`, `column-start`, `row-end`, `column-end`).
fn expand_grid_area(value: &str, store: &mut DeclStore) -> Option<Vec<Declaration>> {
    // A lone <custom-ident> names the area: it expands to the four placement
    // longhands as `Named(name)` (not a separate area-name slot), so re-setting
    // the shorthand between forms clears the previous form's longhands. `auto`
    // and the CSS-wide keywords are rejected by `is_custom_ident`, so they fall
    // through to the grid-line path below (never a name).
    let tokens = split_ws_paren_aware(value);
    if let [single] = tokens.as_slice()
        && !single.contains('/')
        && is_custom_ident(single)
    {
        let named = GridLine::Named(store.intern(single));
        return Some(vec![
            declaration(
                PresentationProperty::GridRowStart,
                DeclValue::GridLine(named),
            ),
            declaration(
                PresentationProperty::GridColumnStart,
                DeclValue::GridLine(named),
            ),
            declaration(PresentationProperty::GridRowEnd, DeclValue::GridLine(named)),
            declaration(
                PresentationProperty::GridColumnEnd,
                DeclValue::GridLine(named),
            ),
        ]);
    }

    let mut parts = value.split('/');
    let first = parts.next()?.trim();
    let mut lines = ArrayVec::<GridLine, 4>::new();
    lines.push(parse_grid_line(first, store)?);
    for part in parts {
        if lines.len() >= 4 {
            return None; // more than four <grid-line> values
        }
        lines.push(parse_grid_line(part.trim(), store)?);
    }
    let auto = GridLine::Auto;
    let (row_start, column_start, row_end, column_end) = match lines.as_slice() {
        [a] => (*a, auto, auto, auto),
        [a, b] => (*a, *b, auto, auto),
        [a, b, c] => (*a, *b, *c, auto),
        [a, b, c, d] => (*a, *b, *c, *d),
        _ => return None,
    };
    Some(vec![
        declaration(
            PresentationProperty::GridRowStart,
            DeclValue::GridLine(row_start),
        ),
        declaration(
            PresentationProperty::GridColumnStart,
            DeclValue::GridLine(column_start),
        ),
        declaration(
            PresentationProperty::GridRowEnd,
            DeclValue::GridLine(row_end),
        ),
        declaration(
            PresentationProperty::GridColumnEnd,
            DeclValue::GridLine(column_end),
        ),
    ])
}

fn expand_transition(value: &str, store: &mut DeclStore) -> Option<Vec<Declaration>> {
    let authored = DomAttributeValue::String(value.to_owned());
    let longhands = transition_longhand_values_from_shorthand(Some(&authored)).ok()?;
    let property = transition_string(longhands.property.as_ref()?)?;
    let duration = transition_string(longhands.duration.as_ref()?)?;
    let timing = transition_string(longhands.timing_function.as_ref()?)?;
    let delay = transition_string(longhands.delay.as_ref()?)?;
    Some(vec![
        declaration(
            PresentationProperty::TransitionProperty,
            DeclValue::Text(store.intern(property)),
        ),
        declaration(
            PresentationProperty::TransitionDuration,
            DeclValue::Text(store.intern(duration)),
        ),
        declaration(
            PresentationProperty::TransitionTimingFunction,
            DeclValue::Text(store.intern(timing)),
        ),
        declaration(
            PresentationProperty::TransitionDelay,
            DeclValue::Text(store.intern(delay)),
        ),
    ])
}

fn transition_string(value: &DomAttributeValue) -> Option<&str> {
    match value {
        DomAttributeValue::String(s) => Some(s.as_str()),
        _ => None,
    }
}

fn expand_border_side(side: BorderSide, value: &str) -> Option<Vec<Declaration>> {
    let property = match side {
        BorderSide::Top => PresentationProperty::BorderTopWidth,
        BorderSide::Right => PresentationProperty::BorderRightWidth,
        BorderSide::Bottom => PresentationProperty::BorderBottomWidth,
        BorderSide::Left => PresentationProperty::BorderLeftWidth,
    };
    let tokens = split_ws_paren_aware(value);
    let [token] = tokens.as_slice() else {
        return None;
    };
    let length = parse_length_for(property, token)?;
    Some(vec![declaration(property, DeclValue::Length(length))])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(block: &DeclBlock, property: PresentationProperty) -> DeclValue {
        *block
            .get(property)
            .unwrap_or_else(|| panic!("expected {property:?} in block"))
    }

    fn text<'a>(
        block: &DeclBlock,
        store: &'a DeclStore,
        property: PresentationProperty,
    ) -> &'a str {
        let value = block
            .get(property)
            .unwrap_or_else(|| panic!("expected {property:?} in block"));
        match value {
            DeclValue::Text(key) => store
                .resolve(*key)
                .unwrap_or_else(|| panic!("text key {key:?} should resolve")),
            other => panic!("expected text value for {property:?}, got {other:?}"),
        }
    }

    fn parse_block(text: &str) -> (DeclBlock, DeclDropReport, DeclStore) {
        let mut store = DeclStore::default();
        let (block, report) = parse_declaration_block(text, &mut store);
        (block, report, store)
    }

    fn px(v: f32) -> DeclValue {
        DeclValue::Length(DeclLength::Px(v))
    }

    fn margin_quad(block: &DeclBlock) -> [DeclValue; 4] {
        [
            value(block, PresentationProperty::MarginTop),
            value(block, PresentationProperty::MarginRight),
            value(block, PresentationProperty::MarginBottom),
            value(block, PresentationProperty::MarginLeft),
        ]
    }

    fn padding_quad(block: &DeclBlock) -> [DeclValue; 4] {
        [
            value(block, PresentationProperty::PaddingTop),
            value(block, PresentationProperty::PaddingRight),
            value(block, PresentationProperty::PaddingBottom),
            value(block, PresentationProperty::PaddingLeft),
        ]
    }

    // — Grammar arms — //

    #[test]
    fn length_grammar_accepts_closed_vocabulary() {
        let mut store = DeclStore::default();
        assert_eq!(
            parse_value(DeclGrammar::Length, "0", &mut store),
            Some(DeclValue::Length(DeclLength::Px(0.0)))
        );
        assert_eq!(
            parse_value(DeclGrammar::Length, "10px", &mut store),
            Some(DeclValue::Length(DeclLength::Px(10.0)))
        );
        assert_eq!(
            parse_value(DeclGrammar::Length, "1.5em", &mut store),
            Some(DeclValue::Length(DeclLength::Em(1.5)))
        );
        assert_eq!(
            parse_value(DeclGrammar::Length, "unset", &mut store),
            Some(DeclValue::Length(DeclLength::Unset))
        );
        assert_eq!(
            parse_value(DeclGrammar::Length, "50%", &mut store),
            Some(DeclValue::Length(DeclLength::Percent(50.0)))
        );
        assert_eq!(
            parse_value(DeclGrammar::Length, "-10%", &mut store),
            Some(DeclValue::Length(DeclLength::Percent(-10.0)))
        );
    }

    #[test]
    fn length_grammar_rejects_unknown_units_and_non_finite() {
        let mut store = DeclStore::default();
        for invalid in [
            "10", "10vh", "NaNpx", "infpx", "-infpx", "10%%", "10.5%px", "",
        ] {
            assert_eq!(
                parse_value(DeclGrammar::Length, invalid, &mut store),
                None,
                "{invalid:?} should fail the Length grammar"
            );
        }
    }

    #[test]
    fn non_negative_length_grammar() {
        let mut store = DeclStore::default();
        assert_eq!(
            parse_value(DeclGrammar::NonNegativeLength, "unset", &mut store),
            Some(DeclValue::Length(DeclLength::Unset))
        );
        assert_eq!(
            parse_value(DeclGrammar::NonNegativeLength, "0", &mut store),
            Some(DeclValue::Length(DeclLength::Px(0.0)))
        );
        assert_eq!(
            parse_value(DeclGrammar::NonNegativeLength, "10px", &mut store),
            Some(DeclValue::Length(DeclLength::Px(10.0)))
        );
        assert_eq!(
            parse_value(DeclGrammar::NonNegativeLength, "1.5em", &mut store),
            Some(DeclValue::Length(DeclLength::Em(1.5)))
        );
        assert_eq!(
            parse_value(DeclGrammar::NonNegativeLength, "50%", &mut store),
            Some(DeclValue::Length(DeclLength::Percent(50.0)))
        );
        for invalid in ["-1px", "-0.5em", "-10%"] {
            assert_eq!(
                parse_value(DeclGrammar::NonNegativeLength, invalid, &mut store),
                None,
                "{invalid:?} should fail the NonNegativeLength grammar"
            );
        }
    }

    #[test]
    fn line_width_grammar_rejects_percentages() {
        let mut store = DeclStore::default();
        assert_eq!(
            parse_value(DeclGrammar::LineWidth, "unset", &mut store),
            Some(DeclValue::Length(DeclLength::Unset))
        );
        assert_eq!(
            parse_value(DeclGrammar::LineWidth, "0", &mut store),
            Some(DeclValue::Length(DeclLength::Px(0.0)))
        );
        assert_eq!(
            parse_value(DeclGrammar::LineWidth, "10px", &mut store),
            Some(DeclValue::Length(DeclLength::Px(10.0)))
        );
        assert_eq!(
            parse_value(DeclGrammar::LineWidth, "1.5em", &mut store),
            Some(DeclValue::Length(DeclLength::Em(1.5)))
        );
        for invalid in ["10%", "-10%", "-1px", "-0.5em"] {
            assert_eq!(
                parse_value(DeclGrammar::LineWidth, invalid, &mut store),
                None,
                "{invalid:?} should fail the LineWidth grammar"
            );
        }
    }

    #[test]
    fn signed_line_width_grammar_rejects_percentages() {
        let mut store = DeclStore::default();
        assert_eq!(
            parse_value(DeclGrammar::SignedLineWidth, "unset", &mut store),
            Some(DeclValue::Length(DeclLength::Unset))
        );
        assert_eq!(
            parse_value(DeclGrammar::SignedLineWidth, "0", &mut store),
            Some(DeclValue::Length(DeclLength::Px(0.0)))
        );
        assert_eq!(
            parse_value(DeclGrammar::SignedLineWidth, "-10px", &mut store),
            Some(DeclValue::Length(DeclLength::Px(-10.0)))
        );
        assert_eq!(
            parse_value(DeclGrammar::SignedLineWidth, "-0.5em", &mut store),
            Some(DeclValue::Length(DeclLength::Em(-0.5)))
        );
        for invalid in ["10%", "-10%", "50%"] {
            assert_eq!(
                parse_value(DeclGrammar::SignedLineWidth, invalid, &mut store),
                None,
                "{invalid:?} should fail the SignedLineWidth grammar"
            );
        }
    }

    #[test]
    fn gap_grammar_accepts_normal_and_non_negative_lengths() {
        let mut store = DeclStore::default();
        assert_eq!(
            parse_value(DeclGrammar::Gap, "normal", &mut store),
            Some(DeclValue::Length(DeclLength::Px(0.0)))
        );
        assert_eq!(
            parse_value(DeclGrammar::Gap, "unset", &mut store),
            Some(DeclValue::Length(DeclLength::Unset))
        );
        assert_eq!(
            parse_value(DeclGrammar::Gap, "0", &mut store),
            Some(DeclValue::Length(DeclLength::Px(0.0)))
        );
        assert_eq!(
            parse_value(DeclGrammar::Gap, "4px", &mut store),
            Some(DeclValue::Length(DeclLength::Px(4.0)))
        );
        assert_eq!(
            parse_value(DeclGrammar::Gap, "1.5em", &mut store),
            Some(DeclValue::Length(DeclLength::Em(1.5)))
        );
        for invalid in ["-1px", "-10%", "10%", "bogus"] {
            assert_eq!(
                parse_value(DeclGrammar::Gap, invalid, &mut store),
                None,
                "{invalid:?} should fail the Gap grammar"
            );
        }
    }

    #[test]
    fn auto_keyword_admission_is_closed_per_property() {
        let mut store = DeclStore::default();
        for admitted in [
            PresentationProperty::Width,
            PresentationProperty::Height,
            PresentationProperty::MinWidth,
            PresentationProperty::MaxWidth,
            PresentationProperty::Top,
            PresentationProperty::Left,
            PresentationProperty::MarginTop,
            PresentationProperty::MarginLeft,
        ] {
            assert_eq!(
                parse_property_value(admitted, "auto", &mut store),
                Some(DeclValue::Keyword(Keyword::Auto)),
                "{admitted:?} should admit auto"
            );
        }
        for rejected in [
            PresentationProperty::PaddingTop,
            PresentationProperty::PaddingLeft,
            PresentationProperty::BorderTopWidth,
            PresentationProperty::BorderTopLeftRadius,
            PresentationProperty::RowGap,
            PresentationProperty::OutlineWidth,
            PresentationProperty::FontSize,
        ] {
            assert_eq!(
                parse_property_value(rejected, "auto", &mut store),
                None,
                "{rejected:?} should reject auto"
            );
        }
    }

    #[test]
    fn line_height_accepts_normal_number_and_non_negative_lengths() {
        let mut store = DeclStore::default();
        assert_eq!(
            parse_value(DeclGrammar::LineHeight, "normal", &mut store),
            Some(DeclValue::Keyword(Keyword::Normal))
        );
        assert_eq!(
            parse_value(DeclGrammar::LineHeight, "1.5", &mut store),
            Some(DeclValue::Number(1.5))
        );
        assert_eq!(
            parse_value(DeclGrammar::LineHeight, "0", &mut store),
            Some(DeclValue::Number(0.0))
        );
        assert_eq!(
            parse_value(DeclGrammar::LineHeight, "20px", &mut store),
            Some(DeclValue::Length(DeclLength::Px(20.0)))
        );
        assert_eq!(
            parse_value(DeclGrammar::LineHeight, "1.25em", &mut store),
            Some(DeclValue::Length(DeclLength::Em(1.25)))
        );
        assert_eq!(
            parse_value(DeclGrammar::LineHeight, "150%", &mut store),
            Some(DeclValue::Length(DeclLength::Percent(150.0)))
        );
    }

    #[test]
    fn line_height_rejects_negatives_and_non_finite() {
        let mut store = DeclStore::default();
        for invalid in ["-120%", "-1", "-2px", "-0.5em", "NaN", "inf", "unset"] {
            assert_eq!(
                parse_value(DeclGrammar::LineHeight, invalid, &mut store),
                None,
                "{invalid:?} should fail the LineHeight grammar"
            );
        }
    }

    #[test]
    fn color_grammar_accepts_and_rejects() {
        let mut store = DeclStore::default();
        assert_eq!(
            parse_value(DeclGrammar::Color, "transparent", &mut store),
            Some(DeclValue::Color([0.0, 0.0, 0.0, 0.0]))
        );
        assert_eq!(
            parse_value(DeclGrammar::Color, "red", &mut store),
            Some(DeclValue::Color([1.0, 0.0, 0.0, 1.0]))
        );
        assert_eq!(
            parse_value(DeclGrammar::Color, "#112233", &mut store),
            Some(DeclValue::Color([
                17.0 / 255.0,
                34.0 / 255.0,
                51.0 / 255.0,
                1.0
            ]))
        );
        assert_eq!(
            parse_value(DeclGrammar::Color, "notacolor", &mut store),
            None
        );
    }

    #[test]
    fn opacity_clamps_to_unit_range() {
        let mut store = DeclStore::default();
        assert_eq!(
            parse_value(DeclGrammar::Opacity, "0.5", &mut store),
            Some(DeclValue::Number(0.5))
        );
        assert_eq!(
            parse_value(DeclGrammar::Opacity, "0", &mut store),
            Some(DeclValue::Number(0.0))
        );
        assert_eq!(
            parse_value(DeclGrammar::Opacity, "1", &mut store),
            Some(DeclValue::Number(1.0))
        );
        assert_eq!(
            parse_value(DeclGrammar::Opacity, "1.5", &mut store),
            Some(DeclValue::Number(1.0))
        );
        assert_eq!(
            parse_value(DeclGrammar::Opacity, "-0.5", &mut store),
            Some(DeclValue::Number(0.0))
        );
        for invalid in ["NaN", "inf", "-inf"] {
            assert_eq!(
                parse_value(DeclGrammar::Opacity, invalid, &mut store),
                None,
                "{invalid:?} should fail the Opacity grammar"
            );
        }
    }

    #[test]
    fn non_negative_number_and_integer_grammars() {
        let mut store = DeclStore::default();
        assert_eq!(
            parse_value(DeclGrammar::NonNegativeNumber, "2.5", &mut store),
            Some(DeclValue::Number(2.5))
        );
        assert_eq!(
            parse_value(DeclGrammar::NonNegativeNumber, "-1", &mut store),
            None
        );
        assert_eq!(
            parse_value(DeclGrammar::NonNegativeNumber, "inf", &mut store),
            None
        );
        assert_eq!(
            parse_value(DeclGrammar::Integer, "-3", &mut store),
            Some(DeclValue::Integer(-3))
        );
        assert_eq!(parse_value(DeclGrammar::Integer, "4.5", &mut store), None);
    }

    #[test]
    fn font_weight_keywords_and_step_constraints() {
        let mut store = DeclStore::default();
        assert_eq!(
            parse_value(DeclGrammar::FontWeight, "normal", &mut store),
            Some(DeclValue::Keyword(Keyword::Normal))
        );
        assert_eq!(
            parse_value(DeclGrammar::FontWeight, "bolder", &mut store),
            Some(DeclValue::Keyword(Keyword::Bolder))
        );
        assert_eq!(
            parse_value(DeclGrammar::FontWeight, "600", &mut store),
            Some(DeclValue::Integer(600))
        );
        assert_eq!(
            parse_value(DeclGrammar::FontWeight, "900", &mut store),
            Some(DeclValue::Integer(900))
        );
        for invalid in ["450", "950", "99", "semibold"] {
            assert_eq!(
                parse_value(DeclGrammar::FontWeight, invalid, &mut store),
                None,
                "{invalid:?} should fail the FontWeight grammar"
            );
        }
    }

    #[test]
    fn flex_basis_grammar_forms() {
        let mut store = DeclStore::default();
        assert_eq!(
            parse_value(DeclGrammar::FlexBasis, "auto", &mut store),
            Some(DeclValue::Keyword(Keyword::Auto))
        );
        assert_eq!(
            parse_value(DeclGrammar::FlexBasis, "content", &mut store),
            Some(DeclValue::Keyword(Keyword::Content))
        );
        assert_eq!(
            parse_value(DeclGrammar::FlexBasis, "0", &mut store),
            Some(DeclValue::Length(DeclLength::Px(0.0)))
        );
        assert_eq!(
            parse_value(DeclGrammar::FlexBasis, "20px", &mut store),
            Some(DeclValue::Length(DeclLength::Px(20.0)))
        );
        assert_eq!(
            parse_value(DeclGrammar::FlexBasis, "unset", &mut store),
            None
        );
    }

    #[test]
    fn flex_basis_rejects_negative_lengths() {
        let mut store = DeclStore::default();
        assert_eq!(
            parse_value(DeclGrammar::FlexBasis, "-10px", &mut store),
            None
        );
        for accepted in ["10px", "auto", "content", "0"] {
            assert!(
                parse_value(DeclGrammar::FlexBasis, accepted, &mut store).is_some(),
                "{accepted:?} should parse"
            );
        }
    }

    #[test]
    fn transform_grammar_accepts_none_and_closed_translate() {
        let mut store = DeclStore::default();
        assert_eq!(
            parse_value(DeclGrammar::Transform, "none", &mut store),
            Some(DeclValue::Keyword(Keyword::None))
        );
        for accepted in [
            "translateX(10px)",
            "translateY(5)",
            "translate(10px, 20px)",
            "translate(-3px, 4)",
        ] {
            let parsed = parse_value(DeclGrammar::Transform, accepted, &mut store)
                .unwrap_or_else(|| panic!("{accepted:?} should parse"));
            let DeclValue::Text(key) = parsed else {
                panic!("{accepted:?} should intern to text");
            };
            assert_eq!(store.resolve(key), Some(accepted));
        }
    }

    #[test]
    fn transform_grammar_rejects_trailing_garbage_and_bad_shapes() {
        let mut store = DeclStore::default();
        for invalid in [
            "translateX(10px 20px)",
            "translateX(10px) extra",
            "translate(10px)",
            "translate(10px, 20px, 30px)",
            "translateX()",
            "translateY(5em)",
            "none none",
            "scale(2)",
            "",
        ] {
            assert_eq!(
                parse_value(DeclGrammar::Transform, invalid, &mut store),
                None,
                "{invalid:?} should fail the Transform grammar"
            );
        }
    }

    #[test]
    fn text_grammar_interns_non_empty_and_rejects_empty() {
        let mut store = DeclStore::default();
        let parsed = parse_value(DeclGrammar::Text, "ui-sans, sans-serif", &mut store)
            .expect("font-family should parse");
        let DeclValue::Text(key) = parsed else {
            panic!("font-family should intern to text");
        };
        assert_eq!(store.resolve(key), Some("ui-sans, sans-serif"));
        assert_eq!(parse_value(DeclGrammar::Text, "   ", &mut store), None);
    }

    #[test]
    fn transition_property_list_grammar() {
        let mut store = DeclStore::default();
        let parsed = parse_value(
            DeclGrammar::TransitionPropertyList,
            "all, opacity",
            &mut store,
        )
        .expect("admitted list should parse");
        let DeclValue::Text(key) = parsed else {
            panic!("transition-property should intern to text");
        };
        assert_eq!(store.resolve(key), Some("all, opacity"));

        assert!(
            parse_value(
                DeclGrammar::TransitionPropertyList,
                "width, color",
                &mut store
            )
            .is_some()
        );
        for invalid in ["mystery", "all, mystery", "all,", ""] {
            assert_eq!(
                parse_value(DeclGrammar::TransitionPropertyList, invalid, &mut store),
                None,
                "{invalid:?} should fail"
            );
        }
    }

    #[test]
    fn transition_time_list_grammar() {
        let mut store = DeclStore::default();
        for accepted in ["80ms, 180ms", "0.25s", "0", "120ms"] {
            assert!(
                parse_value(DeclGrammar::TransitionTimeList, accepted, &mut store).is_some(),
                "{accepted:?} should parse"
            );
        }
        for invalid in ["-20ms", "80", "80ms,", "NaNms", ""] {
            assert_eq!(
                parse_value(DeclGrammar::TransitionTimeList, invalid, &mut store),
                None,
                "{invalid:?} should fail"
            );
        }
    }

    #[test]
    fn transition_signed_time_list_grammar() {
        let mut store = DeclStore::default();
        for accepted in ["-20ms", "-12.5ms", "0.25s", "0", "-20ms, 100ms"] {
            let parsed = parse_value(DeclGrammar::TransitionSignedTimeList, accepted, &mut store)
                .unwrap_or_else(|| panic!("{accepted:?} should parse"));
            let DeclValue::Text(key) = parsed else {
                panic!("{accepted:?} should intern to text");
            };
            assert_eq!(store.resolve(key), Some(accepted));
        }
        for invalid in ["-20", "80", "80ms,", "NaNms", "infms", "-infms", ""] {
            assert_eq!(
                parse_value(DeclGrammar::TransitionSignedTimeList, invalid, &mut store),
                None,
                "{invalid:?} should fail"
            );
        }
    }

    #[test]
    fn transition_timing_list_grammar() {
        let mut store = DeclStore::default();
        assert!(
            parse_value(
                DeclGrammar::TransitionTimingList,
                "ease, linear",
                &mut store
            )
            .is_some()
        );
        assert!(
            parse_value(DeclGrammar::TransitionTimingList, "ease-in-out", &mut store).is_some()
        );
        for invalid in ["cubic-bezier(0,0,1,1)", "ease,", "fast"] {
            assert_eq!(
                parse_value(DeclGrammar::TransitionTimingList, invalid, &mut store),
                None,
                "{invalid:?} should fail"
            );
        }
    }

    #[test]
    fn transition_list_grammars_reject_over_four_items() {
        // Four items are admitted; a fifth is malformed (PLAN.md S1.1:
        // over-capacity lists are rejected at admission, matching the
        // scheduler slot's capacity-4 bound).
        for accepted in [
            "left, width, opacity, transform",
            "1s, 2s, 3s, 4s",
            "ease, ease-in, ease-out, linear",
        ] {
            assert!(
                comma_split_trim(accepted).is_some(),
                "{accepted:?} should parse"
            );
        }
        for rejected in [
            "left, width, opacity, transform, color",
            "1s, 2s, 3s, 4s, 5s",
            "ease, ease-in, ease-out, linear, ease-in-out",
        ] {
            assert_eq!(
                comma_split_trim(rejected),
                None,
                "{rejected:?} should fail the capacity bound"
            );
        }
    }

    // — Shorthand expansions — //

    #[test]
    fn border_radius_expands_css_corner_mapping() {
        let (block, report, _store) = parse_block("border-radius: 10px 20px 30px");
        assert_eq!(report.applied, 1);
        assert_eq!(
            value(&block, PresentationProperty::BorderTopLeftRadius),
            DeclValue::Length(DeclLength::Px(10.0))
        );
        assert_eq!(
            value(&block, PresentationProperty::BorderTopRightRadius),
            DeclValue::Length(DeclLength::Px(20.0))
        );
        assert_eq!(
            value(&block, PresentationProperty::BorderBottomRightRadius),
            DeclValue::Length(DeclLength::Px(30.0))
        );
        assert_eq!(
            value(&block, PresentationProperty::BorderBottomLeftRadius),
            DeclValue::Length(DeclLength::Px(20.0))
        );
    }

    #[test]
    fn border_radius_two_value_and_unset_forms() {
        let (block, _, _) = parse_block("border-radius: 10px 20px");
        assert_eq!(
            value(&block, PresentationProperty::BorderTopLeftRadius),
            DeclValue::Length(DeclLength::Px(10.0))
        );
        assert_eq!(
            value(&block, PresentationProperty::BorderTopRightRadius),
            DeclValue::Length(DeclLength::Px(20.0))
        );
        assert_eq!(
            value(&block, PresentationProperty::BorderBottomRightRadius),
            DeclValue::Length(DeclLength::Px(10.0))
        );
        assert_eq!(
            value(&block, PresentationProperty::BorderBottomLeftRadius),
            DeclValue::Length(DeclLength::Px(20.0))
        );

        let (block, _, _) = parse_block("border-radius: unset");
        assert_eq!(
            value(&block, PresentationProperty::BorderTopLeftRadius),
            DeclValue::Length(DeclLength::Unset)
        );
    }

    #[test]
    fn border_radius_rejects_over_four_tokens() {
        let (_, report, _) = parse_block("border-radius: 1px 2px 3px 4px 5px");
        assert_eq!(report.applied, 0);
        assert_eq!(report.dropped, 1);
    }

    #[test]
    fn border_color_does_not_split_inside_rgb() {
        let (block, report, _store) = parse_block("border-color: rgb(255, 0, 0) rgb(0, 0, 255)");
        assert_eq!(report.applied, 1);
        assert_eq!(
            value(&block, PresentationProperty::BorderTopColor),
            DeclValue::Color([1.0, 0.0, 0.0, 1.0])
        );
        assert_eq!(
            value(&block, PresentationProperty::BorderRightColor),
            DeclValue::Color([0.0, 0.0, 1.0, 1.0])
        );
        assert_eq!(
            value(&block, PresentationProperty::BorderBottomColor),
            DeclValue::Color([1.0, 0.0, 0.0, 1.0])
        );
        assert_eq!(
            value(&block, PresentationProperty::BorderLeftColor),
            DeclValue::Color([0.0, 0.0, 1.0, 1.0])
        );
    }

    #[test]
    fn gap_expands_one_or_two_tokens_and_accepts_unset() {
        let (block, _, _) = parse_block("gap: 12px");
        assert_eq!(
            value(&block, PresentationProperty::RowGap),
            DeclValue::Length(DeclLength::Px(12.0))
        );
        assert_eq!(
            value(&block, PresentationProperty::ColumnGap),
            DeclValue::Length(DeclLength::Px(12.0))
        );

        let (block, _, _) = parse_block("gap: 8px 10px");
        assert_eq!(
            value(&block, PresentationProperty::RowGap),
            DeclValue::Length(DeclLength::Px(8.0))
        );
        assert_eq!(
            value(&block, PresentationProperty::ColumnGap),
            DeclValue::Length(DeclLength::Px(10.0))
        );

        let (block, _, _) = parse_block("gap: normal");
        assert_eq!(
            value(&block, PresentationProperty::RowGap),
            DeclValue::Length(DeclLength::Px(0.0))
        );

        let (block, report, _) = parse_block("gap: unset");
        assert_eq!(report.applied, 1);
        assert_eq!(
            value(&block, PresentationProperty::RowGap),
            DeclValue::Length(DeclLength::Unset)
        );
        assert_eq!(
            value(&block, PresentationProperty::ColumnGap),
            DeclValue::Length(DeclLength::Unset)
        );
    }

    #[test]
    fn flex_keyword_forms_expand_exactly() {
        let (block, _, _) = parse_block("flex: none");
        assert_eq!(
            value(&block, PresentationProperty::FlexGrow),
            DeclValue::Number(0.0)
        );
        assert_eq!(
            value(&block, PresentationProperty::FlexShrink),
            DeclValue::Number(0.0)
        );
        assert_eq!(
            value(&block, PresentationProperty::FlexBasis),
            DeclValue::Keyword(Keyword::Auto)
        );

        let (block, _, _) = parse_block("flex: auto");
        assert_eq!(
            value(&block, PresentationProperty::FlexGrow),
            DeclValue::Number(1.0)
        );
        assert_eq!(
            value(&block, PresentationProperty::FlexShrink),
            DeclValue::Number(1.0)
        );

        let (block, _, _) = parse_block("flex: initial");
        assert_eq!(
            value(&block, PresentationProperty::FlexGrow),
            DeclValue::Number(0.0)
        );
        assert_eq!(
            value(&block, PresentationProperty::FlexShrink),
            DeclValue::Number(1.0)
        );
    }

    #[test]
    fn flex_token_forms_expand_exactly() {
        let (block, _, _) = parse_block("flex: 2");
        assert_eq!(
            value(&block, PresentationProperty::FlexGrow),
            DeclValue::Number(2.0)
        );
        assert_eq!(
            value(&block, PresentationProperty::FlexShrink),
            DeclValue::Number(1.0)
        );
        assert_eq!(
            value(&block, PresentationProperty::FlexBasis),
            DeclValue::Length(DeclLength::Px(0.0))
        );

        let (block, _, _) = parse_block("flex: 2 3");
        assert_eq!(
            value(&block, PresentationProperty::FlexShrink),
            DeclValue::Number(3.0)
        );

        let (block, _, _) = parse_block("flex: 2 20px");
        assert_eq!(
            value(&block, PresentationProperty::FlexShrink),
            DeclValue::Number(1.0)
        );
        assert_eq!(
            value(&block, PresentationProperty::FlexBasis),
            DeclValue::Length(DeclLength::Px(20.0))
        );

        let (block, _, _) = parse_block("flex: 2 3 30px");
        assert_eq!(
            value(&block, PresentationProperty::FlexBasis),
            DeclValue::Length(DeclLength::Px(30.0))
        );

        let (_, report, _) = parse_block("flex: 1 2 3");
        assert_eq!(report.dropped, 1);
    }

    #[test]
    fn flex_flow_single_token_resets_the_other_longhand() {
        let (block, _, _) = parse_block("flex-flow: column");
        assert_eq!(
            value(&block, PresentationProperty::FlexDirection),
            DeclValue::Keyword(Keyword::Column)
        );
        assert_eq!(
            value(&block, PresentationProperty::FlexWrap),
            DeclValue::Keyword(Keyword::Nowrap)
        );

        let (block, _, _) = parse_block("flex-flow: wrap");
        assert_eq!(
            value(&block, PresentationProperty::FlexDirection),
            DeclValue::Keyword(Keyword::Row)
        );
        assert_eq!(
            value(&block, PresentationProperty::FlexWrap),
            DeclValue::Keyword(Keyword::Wrap)
        );

        let (block, _, _) = parse_block("flex-flow: row wrap");
        assert_eq!(
            value(&block, PresentationProperty::FlexDirection),
            DeclValue::Keyword(Keyword::Row)
        );
        assert_eq!(
            value(&block, PresentationProperty::FlexWrap),
            DeclValue::Keyword(Keyword::Wrap)
        );
    }

    #[test]
    fn flex_flow_rejects_two_from_one_set() {
        for invalid in ["row row", "wrap nowrap", "row column", "row wrap extra"] {
            let (_, report, _) = parse_block(&format!("flex-flow: {invalid}"));
            assert_eq!(report.dropped, 1, "{invalid:?} should be dropped");
        }
    }

    #[test]
    fn place_content_validates_both_halves() {
        let (block, _, _) = parse_block("place-content: center");
        assert_eq!(
            value(&block, PresentationProperty::AlignContent),
            DeclValue::Keyword(Keyword::Center)
        );
        assert_eq!(
            value(&block, PresentationProperty::JustifyContent),
            DeclValue::Keyword(Keyword::Center)
        );

        let (block, _, _) = parse_block("place-content: center start");
        assert_eq!(
            value(&block, PresentationProperty::JustifyContent),
            DeclValue::Keyword(Keyword::Start)
        );

        // "stretch" is in the align-content set but not the justify-content set.
        let (_, report, _) = parse_block("place-content: stretch");
        assert_eq!(report.dropped, 1);

        let (block, _, _) = parse_block("place-content: stretch center");
        assert_eq!(
            value(&block, PresentationProperty::AlignContent),
            DeclValue::Keyword(Keyword::Stretch)
        );
    }

    #[test]
    fn place_items_and_place_self_expand_to_both_sides() {
        let (block, _, _) = parse_block("place-items: center start");
        assert_eq!(
            value(&block, PresentationProperty::AlignItems),
            DeclValue::Keyword(Keyword::Center)
        );
        assert_eq!(
            value(&block, PresentationProperty::JustifyItems),
            DeclValue::Keyword(Keyword::Start)
        );
        assert_eq!(block.len(), 2);

        let (block, _, _) = parse_block("place-items: center");
        assert_eq!(
            value(&block, PresentationProperty::JustifyItems),
            DeclValue::Keyword(Keyword::Center)
        );

        let (block, _, _) = parse_block("place-self: end auto");
        assert_eq!(
            value(&block, PresentationProperty::AlignSelf),
            DeclValue::Keyword(Keyword::End)
        );
        assert_eq!(
            value(&block, PresentationProperty::JustifySelf),
            DeclValue::Keyword(Keyword::Auto)
        );
        assert_eq!(block.len(), 2);

        // `baseline` is not a justify-items keyword, so the whole declaration
        // drops.
        let (_, report, _) = parse_block("place-items: center baseline");
        assert_eq!(report.dropped, 1);
    }

    #[test]
    fn transition_shorthand_expands_to_normalized_longhands() {
        let (block, report, store) = parse_block("transition: width 120ms ease-in 40ms");
        assert_eq!(report.applied, 1);
        assert_eq!(
            text(&block, &store, PresentationProperty::TransitionProperty),
            "width"
        );
        assert_eq!(
            text(&block, &store, PresentationProperty::TransitionDuration),
            "0.12s"
        );
        assert_eq!(
            text(
                &block,
                &store,
                PresentationProperty::TransitionTimingFunction
            ),
            "ease-in"
        );
        assert_eq!(
            text(&block, &store, PresentationProperty::TransitionDelay),
            "0.04s"
        );
    }

    #[test]
    fn border_side_expands_single_length_and_rejects_multi_token() {
        let (block, _, _) = parse_block("border-top: 2px");
        assert_eq!(
            value(&block, PresentationProperty::BorderTopWidth),
            DeclValue::Length(DeclLength::Px(2.0))
        );

        let (_, report, _) = parse_block("border-left: 2px solid");
        assert_eq!(report.dropped, 1);
    }

    #[test]
    fn shorthand_expansion_rejects_negative_lengths() {
        for (css, name) in [
            ("border-radius: -4px", "border-radius"),
            ("gap: -4px", "gap"),
            ("border-top: -2px", "border-top"),
        ] {
            let (_, report, _) = parse_block(css);
            assert_eq!(report.dropped, 1, "{name} negative should drop");
            assert_eq!(report.applied, 0, "{name} negative should not apply");
        }

        let (block, report, _) = parse_block("border-radius: 4px");
        assert_eq!(report.applied, 1);
        assert_eq!(
            value(&block, PresentationProperty::BorderTopLeftRadius),
            DeclValue::Length(DeclLength::Px(4.0))
        );
    }

    #[test]
    fn margin_shorthand_expands_clockwise() {
        let (block, report, _) = parse_block("margin: 8px");
        assert_eq!(report.applied, 1);
        assert_eq!(margin_quad(&block), [px(8.0), px(8.0), px(8.0), px(8.0)]);

        let (block, _, _) = parse_block("margin: 1px 2px");
        assert_eq!(margin_quad(&block), [px(1.0), px(2.0), px(1.0), px(2.0)]);

        let (block, _, _) = parse_block("margin: 1px 2px 3px");
        assert_eq!(margin_quad(&block), [px(1.0), px(2.0), px(3.0), px(2.0)]);

        let (block, _, _) = parse_block("margin: 1px 2px 3px 4px");
        assert_eq!(margin_quad(&block), [px(1.0), px(2.0), px(3.0), px(4.0)]);

        let (_, report, _) = parse_block("margin: 1px 2px 3px 4px 5px");
        assert_eq!(report.applied, 0);
        assert_eq!(report.dropped, 1);
    }

    #[test]
    fn margin_shorthand_admits_negative_lengths() {
        let (block, report, _) = parse_block("margin: -10px 2px");
        assert_eq!(report.applied, 1);
        assert_eq!(
            margin_quad(&block),
            [px(-10.0), px(2.0), px(-10.0), px(2.0)]
        );
    }

    #[test]
    fn margin_shorthand_admits_auto() {
        let auto = DeclValue::Keyword(Keyword::Auto);

        let (block, report, _) = parse_block("margin: auto");
        assert_eq!(report.applied, 1);
        assert_eq!(margin_quad(&block), [auto, auto, auto, auto]);

        let (block, _, _) = parse_block("margin: 0 auto");
        assert_eq!(margin_quad(&block), [px(0.0), auto, px(0.0), auto]);

        let (block, _, _) = parse_block("margin: 10px auto 20px");
        assert_eq!(margin_quad(&block), [px(10.0), auto, px(20.0), auto]);

        let (_, report, _) = parse_block("margin: auto auto auto auto auto");
        assert_eq!(report.applied, 0);
        assert_eq!(report.dropped, 1);
    }

    #[test]
    fn padding_shorthand_rejects_auto() {
        let (_, report, _) = parse_block("padding: auto");
        assert_eq!(report.applied, 0);
        assert_eq!(report.dropped, 1);
    }

    #[test]
    fn padding_shorthand_expands_and_rejects_negatives() {
        let (block, report, _) = parse_block("padding: 4px");
        assert_eq!(report.applied, 1);
        assert_eq!(padding_quad(&block), [px(4.0), px(4.0), px(4.0), px(4.0)]);

        let (_, report, _) = parse_block("padding: -4px");
        assert_eq!(report.applied, 0);
        assert_eq!(report.dropped, 1);
    }

    // — Block-parse mechanics — //

    #[test]
    fn block_parse_rejects_negative_duration_but_accepts_negative_delay() {
        let (block, report, store) = parse_block(
            "transition-duration: -20ms; transition-delay: -20ms; \
             transition-delay: -20ms, 100ms",
        );
        assert_eq!(report.applied, 2);
        assert_eq!(report.dropped, 1);
        assert!(
            block
                .get(PresentationProperty::TransitionDuration)
                .is_none()
        );
        assert_eq!(
            text(&block, &store, PresentationProperty::TransitionDelay),
            "-20ms, 100ms"
        );
    }

    #[test]
    fn block_parse_counts_mixed_valid_and_invalid() {
        let (block, report, _) =
            parse_block("width: 10px; mystery: 1px; height: 20px; malformed; opacity: 0.5");
        assert_eq!(report.applied, 3);
        assert_eq!(report.dropped, 2);
        assert_eq!(block.len(), 3);
    }

    #[test]
    fn block_parse_uses_last_wins_within_a_block() {
        let (block, report, _) = parse_block("width: 10px; width: 20px");
        assert_eq!(report.applied, 2);
        assert_eq!(block.len(), 1);
        assert_eq!(
            value(&block, PresentationProperty::Width),
            DeclValue::Length(DeclLength::Px(20.0))
        );
    }

    #[test]
    fn block_parse_skips_empty_segments_without_counting() {
        let (block, report, _) = parse_block("width: 10px;;;height: 20px;");
        assert_eq!(report.applied, 2);
        assert_eq!(report.dropped, 0);
        assert_eq!(block.len(), 2);
    }

    #[test]
    fn block_parse_drops_unknown_and_content_attribute_names() {
        let (block, report, _) = parse_block("id: foo; src: bar; width: 10px");
        assert_eq!(report.applied, 1);
        assert_eq!(report.dropped, 2);
        assert_eq!(block.len(), 1);
        assert_eq!(
            value(&block, PresentationProperty::Width),
            DeclValue::Length(DeclLength::Px(10.0))
        );
    }

    #[test]
    fn block_parse_drops_missing_and_duplicate_colons() {
        let (_, report, _) = parse_block("width 10px; height: 10px: 20px; top: 5px");
        assert_eq!(report.applied, 1);
        assert_eq!(report.dropped, 2);
    }

    // — Strict parse — //

    #[test]
    fn strict_parse_ok_for_longhand() {
        let mut store = DeclStore::default();
        let declarations =
            parse_declaration_strict("width", "10px", &mut store).expect("longhand should parse");
        assert_eq!(declarations.len(), 1);
        assert_eq!(declarations[0].property, PresentationProperty::Width);
        assert_eq!(
            declarations[0].value,
            DeclValue::Length(DeclLength::Px(10.0))
        );
    }

    #[test]
    fn font_size_unset_is_admitted() {
        let mut store = DeclStore::default();
        let declarations = parse_declaration_strict("font-size", "unset", &mut store)
            .expect("font-size: unset should parse");
        assert_eq!(declarations.len(), 1);
        assert_eq!(declarations[0].property, PresentationProperty::FontSize);
        assert_eq!(declarations[0].value, DeclValue::Length(DeclLength::Unset));
    }

    #[test]
    fn strict_parse_ok_for_shorthand_in_expansion_order() {
        let mut store = DeclStore::default();
        let declarations =
            parse_declaration_strict("flex", "1", &mut store).expect("shorthand should parse");
        let properties: Vec<PresentationProperty> =
            declarations.iter().map(|d| d.property).collect();
        assert_eq!(
            properties,
            vec![
                PresentationProperty::FlexGrow,
                PresentationProperty::FlexShrink,
                PresentationProperty::FlexBasis,
            ]
        );
    }

    #[test]
    fn strict_parse_errors_on_unknown_malformed_and_empty() {
        let mut store = DeclStore::default();
        assert_eq!(
            parse_declaration_strict("mystery", "1px", &mut store),
            Err(DomError::InvalidAttributeValue)
        );
        assert_eq!(
            parse_declaration_strict("width", "notalength", &mut store),
            Err(DomError::InvalidAttributeValue)
        );
        assert_eq!(
            parse_declaration_strict("", "10px", &mut store),
            Err(DomError::InvalidAttributeValue)
        );
        assert_eq!(
            parse_declaration_strict("width", "", &mut store),
            Err(DomError::InvalidAttributeValue)
        );
    }

    #[test]
    fn strict_parse_rejects_negative_duration_but_accepts_negative_delay() {
        let mut store = DeclStore::default();
        assert_eq!(
            parse_declaration_strict("transition-duration", "-20ms", &mut store),
            Err(DomError::InvalidAttributeValue)
        );
        let declarations = parse_declaration_strict("transition-delay", "-20ms", &mut store)
            .expect("negative delay should parse");
        assert_eq!(declarations.len(), 1);
        assert_eq!(
            declarations[0].property,
            PresentationProperty::TransitionDelay
        );
        let DeclValue::Text(key) = declarations[0].value else {
            panic!("delay should intern to text");
        };
        assert_eq!(store.resolve(key), Some("-20ms"));
    }

    // — `!important` marker — //

    #[test]
    fn importance_marker_strips_from_strict_parse_and_carries_the_flag() {
        let mut store = DeclStore::default();
        let declarations = parse_declaration_strict("width", "10px !important", &mut store)
            .expect("important width should parse");
        assert_eq!(declarations.len(), 1);
        assert_eq!(declarations[0].property, PresentationProperty::Width);
        assert_eq!(
            declarations[0].value,
            DeclValue::Length(DeclLength::Px(10.0))
        );
        assert!(declarations[0].important);
    }

    #[test]
    fn importance_marker_accepts_case_and_whitespace_variants() {
        let mut store = DeclStore::default();
        for authored in [
            "10px !important",
            "10px!important",
            "10px ! IMPORTANT",
            "10px !  important",
        ] {
            let declarations = parse_declaration_strict("width", authored, &mut store)
                .unwrap_or_else(|_| panic!("{authored:?} should parse as important"));
            assert_eq!(declarations.len(), 1, "{authored:?}");
            assert_eq!(
                declarations[0].value,
                DeclValue::Length(DeclLength::Px(10.0)),
                "{authored:?}"
            );
            assert!(declarations[0].important, "{authored:?}");
        }
    }

    #[test]
    fn importance_marker_absent_keeps_normal_flag() {
        let mut store = DeclStore::default();
        let declarations = parse_declaration_strict("width", "10px", &mut store)
            .expect("plain width should parse");
        assert_eq!(declarations.len(), 1);
        assert!(!declarations[0].important);
    }

    #[test]
    fn importance_marker_is_not_recognized_mid_value() {
        let mut store = DeclStore::default();
        // `!important` is only a marker at the END of the value string; these
        // values leave the marker in place, so the grammar rejects them.
        assert_eq!(
            parse_declaration_strict("width", "10px !important 5px", &mut store),
            Err(DomError::InvalidAttributeValue)
        );
        assert_eq!(
            parse_declaration_strict("width", "!important 10px", &mut store),
            Err(DomError::InvalidAttributeValue)
        );
        // An empty remainder after the marker is malformed.
        assert_eq!(
            parse_declaration_strict("width", "!important", &mut store),
            Err(DomError::InvalidAttributeValue)
        );
    }

    #[test]
    fn block_parse_upserts_importance_and_keeps_the_normal_after() {
        let (block, report, _) = parse_block("width: 10px !important; height: 5px");
        assert_eq!(report.applied, 2);
        assert_eq!(report.dropped, 0);
        assert_eq!(
            value(&block, PresentationProperty::Width),
            DeclValue::Length(DeclLength::Px(10.0))
        );
        assert_eq!(
            value(&block, PresentationProperty::Height),
            DeclValue::Length(DeclLength::Px(5.0))
        );
        assert_eq!(block.importance(), &[true, false]);
    }

    #[test]
    fn importance_marker_strict_non_char_boundary_tail_is_typed_error() {
        // A post-`!` tail where byte index 9 is NOT a char boundary (multi-byte
        // UTF-8) is not a marker; the strict path must return a typed error,
        // never panic on `after[..9]`/`after[9..]`.
        let mut store = DeclStore::default();
        for authored in ["10px !ééééé", "10px !😀😀😀"] {
            assert_eq!(
                parse_declaration_strict("width", authored, &mut store),
                Err(DomError::InvalidAttributeValue),
                "{authored:?} should be a typed error, not a panic"
            );
        }
    }

    #[test]
    fn importance_marker_block_non_char_boundary_tail_drops_and_continues() {
        // The block path must drop the malformed declaration and keep parsing:
        // `height: 5px` survives, no panic on the non-boundary byte index.
        let (block, report, _) = parse_block("width: 10px !ééééé; height: 5px");
        assert_eq!(report.applied, 1);
        assert_eq!(report.dropped, 1);
        assert_eq!(
            value(&block, PresentationProperty::Height),
            DeclValue::Length(DeclLength::Px(5.0))
        );
    }

    #[test]
    fn importance_marker_block_trailing_non_ascii_after_valid_head_drops() {
        // Byte 9 IS a char boundary here (valid `important` head), but the
        // tail `é` is non-empty — not a marker, so the grammar rejects.
        let (_, report, _) = parse_block("width: 10px !importanté");
        assert_eq!(report.applied, 0);
        assert_eq!(report.dropped, 1);
    }

    #[test]
    fn importance_marker_block_char_boundary_on_boundary_is_not_a_marker() {
        // Byte 9 lands exactly ON a char boundary (`ééé` + `imp`), so `get`
        // succeeds and returns a non-`important` head — not a marker, drop.
        let (_, report, _) = parse_block("width: 10px !éééimportant");
        assert_eq!(report.applied, 0);
        assert_eq!(report.dropped, 1);
    }

    #[test]
    fn block_parse_drops_malformed_importance_mid_value_and_continues() {
        // The G rule (counted drop, parsing continues) for the malformed
        // mid-value `!important` shape specifically: `10px !important 5px` is
        // not a trailing marker (the `!important` head is valid but the tail
        // `5px` is non-empty), so the whole `width` declaration drops + counts,
        // and the following `height: 5px` still applies — no panic.
        let (block, report, _) = parse_block("width: 10px !important 5px; height: 5px");
        assert_eq!(report.applied, 1);
        assert_eq!(report.dropped, 1);
        assert_eq!(
            value(&block, PresentationProperty::Height),
            DeclValue::Length(DeclLength::Px(5.0))
        );
    }

    // — longhands_for_name — //

    #[test]
    fn longhands_for_name_resolves_longhands_to_themselves() {
        assert_eq!(
            longhands_for_name("width").map(SmallVec::into_vec),
            Some(vec![PresentationProperty::Width])
        );
        assert_eq!(
            longhands_for_name("opacity").map(SmallVec::into_vec),
            Some(vec![PresentationProperty::Opacity])
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)] // one assertion row per shorthand's expansion set
    fn longhands_for_name_resolves_shorthands_to_expansion_sets() {
        assert_eq!(
            longhands_for_name("border-radius").map(SmallVec::into_vec),
            Some(vec![
                PresentationProperty::BorderTopLeftRadius,
                PresentationProperty::BorderTopRightRadius,
                PresentationProperty::BorderBottomRightRadius,
                PresentationProperty::BorderBottomLeftRadius,
            ])
        );
        assert_eq!(
            longhands_for_name("border-color").map(SmallVec::into_vec),
            Some(vec![
                PresentationProperty::BorderTopColor,
                PresentationProperty::BorderRightColor,
                PresentationProperty::BorderBottomColor,
                PresentationProperty::BorderLeftColor,
            ])
        );
        assert_eq!(
            longhands_for_name("gap").map(SmallVec::into_vec),
            Some(vec![
                PresentationProperty::RowGap,
                PresentationProperty::ColumnGap,
            ])
        );
        assert_eq!(
            longhands_for_name("flex").map(SmallVec::into_vec),
            Some(vec![
                PresentationProperty::FlexGrow,
                PresentationProperty::FlexShrink,
                PresentationProperty::FlexBasis,
            ])
        );
        assert_eq!(
            longhands_for_name("flex-flow").map(SmallVec::into_vec),
            Some(vec![
                PresentationProperty::FlexDirection,
                PresentationProperty::FlexWrap,
            ])
        );
        assert_eq!(
            longhands_for_name("place-content").map(SmallVec::into_vec),
            Some(vec![
                PresentationProperty::AlignContent,
                PresentationProperty::JustifyContent,
            ])
        );
        assert_eq!(
            longhands_for_name("place-items").map(SmallVec::into_vec),
            Some(vec![
                PresentationProperty::AlignItems,
                PresentationProperty::JustifyItems,
            ])
        );
        assert_eq!(
            longhands_for_name("place-self").map(SmallVec::into_vec),
            Some(vec![
                PresentationProperty::AlignSelf,
                PresentationProperty::JustifySelf,
            ])
        );
        assert_eq!(
            longhands_for_name("grid-column").map(SmallVec::into_vec),
            Some(vec![
                PresentationProperty::GridColumnStart,
                PresentationProperty::GridColumnEnd,
            ])
        );
        assert_eq!(
            longhands_for_name("grid-row").map(SmallVec::into_vec),
            Some(vec![
                PresentationProperty::GridRowStart,
                PresentationProperty::GridRowEnd,
            ])
        );
        assert_eq!(
            longhands_for_name("transition").map(SmallVec::into_vec),
            Some(vec![
                PresentationProperty::TransitionProperty,
                PresentationProperty::TransitionDuration,
                PresentationProperty::TransitionTimingFunction,
                PresentationProperty::TransitionDelay,
            ])
        );
        assert_eq!(
            longhands_for_name("border-top").map(SmallVec::into_vec),
            Some(vec![PresentationProperty::BorderTopWidth])
        );
        assert_eq!(
            longhands_for_name("margin").map(SmallVec::into_vec),
            Some(vec![
                PresentationProperty::MarginTop,
                PresentationProperty::MarginRight,
                PresentationProperty::MarginBottom,
                PresentationProperty::MarginLeft,
            ])
        );
        assert_eq!(
            longhands_for_name("padding").map(SmallVec::into_vec),
            Some(vec![
                PresentationProperty::PaddingTop,
                PresentationProperty::PaddingRight,
                PresentationProperty::PaddingBottom,
                PresentationProperty::PaddingLeft,
            ])
        );
    }

    #[test]
    fn longhands_for_name_returns_none_for_unknown_names() {
        assert_eq!(longhands_for_name("frobnicate"), None);
        assert_eq!(longhands_for_name(""), None);
    }

    // — Grid family — //

    fn track_size(min: TrackBreadth, max: TrackMaxBreadth) -> GridTrackSize {
        GridTrackSize { min, max }
    }

    #[test]
    fn grid_line_parses_auto_integer_span() {
        let mut store = DeclStore::default();
        assert_eq!(parse_grid_line("auto", &mut store), Some(GridLine::Auto));
        assert_eq!(parse_grid_line("2", &mut store), Some(GridLine::Line(2)));
        assert_eq!(parse_grid_line("-3", &mut store), Some(GridLine::Line(-3)));
        assert_eq!(
            parse_grid_line("span 2", &mut store),
            Some(GridLine::Span(2))
        );
    }

    #[test]
    fn grid_line_parses_named_and_named_span_forms() {
        let mut store = DeclStore::default();
        // "main" dedups to the first interned key across the three forms.
        assert_eq!(
            parse_grid_line("main", &mut store),
            Some(GridLine::Named(TextKey(0)))
        );
        assert_eq!(
            parse_grid_line("span main", &mut store),
            Some(GridLine::NamedSpan(TextKey(0), 0))
        );
        assert_eq!(
            parse_grid_line("span main 3", &mut store),
            Some(GridLine::NamedSpan(TextKey(0), 3))
        );
    }

    #[test]
    fn grid_line_drops_zero_negative_span_and_bare_span() {
        let mut store = DeclStore::default();
        assert_eq!(parse_grid_line("0", &mut store), None);
        assert_eq!(parse_grid_line("span", &mut store), None);
        assert_eq!(parse_grid_line("span 0", &mut store), None);
        assert_eq!(parse_grid_line("span -1", &mut store), None);
        assert_eq!(parse_grid_line("span main 0", &mut store), None);
        assert_eq!(parse_grid_line("2 span", &mut store), None);
    }

    #[test]
    fn is_custom_ident_rejects_reserved_keywords() {
        // CSS Values 4 §3.2 excludes the CSS-wide keywords; CSS Grid §7.1
        // excludes `span`; `auto` is the shorthand's keyword, not a name. None
        // of these may become a name in area cells, `[name]` line names, or the
        // lone-ident `grid-area` branch.
        for reserved in [
            "span",
            "initial",
            "inherit",
            "unset",
            "revert",
            "revert-layer",
            "auto",
        ] {
            assert!(
                !is_custom_ident(reserved),
                "{reserved:?} must not be a <custom-ident>"
            );
        }
        // Signed integers are lines, not names.
        assert!(!is_custom_ident("1"));
        assert!(!is_custom_ident("-1"));
        // Genuine idents (including hyphen-prefixed) still pass.
        for valid in ["header", "main", "a1", "-start", "_x", "foo-bar"] {
            assert!(
                is_custom_ident(valid),
                "{valid:?} should be a <custom-ident>"
            );
        }
    }

    #[test]
    fn grid_column_and_row_shorthands_expand_per_d1() {
        // Lone span: start Auto, end Span.
        let (block, _, _) = parse_block("grid-column: span 2");
        assert_eq!(
            value(&block, PresentationProperty::GridColumnStart),
            DeclValue::GridLine(GridLine::Auto)
        );
        assert_eq!(
            value(&block, PresentationProperty::GridColumnEnd),
            DeclValue::GridLine(GridLine::Span(2))
        );

        // Lone non-span: start = value, end Auto.
        let (block, _, _) = parse_block("grid-column: 2");
        assert_eq!(
            value(&block, PresentationProperty::GridColumnStart),
            DeclValue::GridLine(GridLine::Line(2))
        );
        assert_eq!(
            value(&block, PresentationProperty::GridColumnEnd),
            DeclValue::GridLine(GridLine::Auto)
        );

        // Two values in order.
        let (block, _, _) = parse_block("grid-row: 1 / 3");
        assert_eq!(
            value(&block, PresentationProperty::GridRowStart),
            DeclValue::GridLine(GridLine::Line(1))
        );
        assert_eq!(
            value(&block, PresentationProperty::GridRowEnd),
            DeclValue::GridLine(GridLine::Line(3))
        );

        // Mixed definite + span.
        let (block, _, _) = parse_block("grid-column: 2 / span 3");
        assert_eq!(
            value(&block, PresentationProperty::GridColumnStart),
            DeclValue::GridLine(GridLine::Line(2))
        );
        assert_eq!(
            value(&block, PresentationProperty::GridColumnEnd),
            DeclValue::GridLine(GridLine::Span(3))
        );

        // Malformed (three '/' parts) drops.
        let (_, report, _) = parse_block("grid-column: 1 / 2 / 3");
        assert_eq!(report.dropped, 1);
    }

    #[test]
    fn grid_areas_none_parses_to_the_empty_sentinel() {
        // `grid-template-areas: none` is the CSS initial value: it admits to the
        // empty-map sentinel (the null key), never a dropped declaration.
        let mut store = DeclStore::default();
        assert_eq!(
            parse_grid_areas("none", &mut store),
            Some(GridAreasKey::default()),
            "`none` admits to the empty-map sentinel"
        );
        // The `<string>+` form still parses (and interns a real, non-null map).
        let key = parse_grid_areas("\"a a\"", &mut store).expect("string form parses");
        assert_ne!(
            key,
            GridAreasKey::default(),
            "a real map is not the sentinel"
        );
        assert_eq!(store.interned_grid_areas_count(), 1, "one interned map");
    }

    #[test]
    fn grid_track_size_leaf_parses_each_kind() {
        assert_eq!(
            parse_track_size("auto"),
            Some(track_size(TrackBreadth::Auto, TrackMaxBreadth::Auto))
        );
        assert_eq!(
            parse_track_size("100px"),
            Some(track_size(
                TrackBreadth::Px(100.0),
                TrackMaxBreadth::Px(100.0)
            ))
        );
        assert_eq!(
            parse_track_size("20%"),
            Some(track_size(
                TrackBreadth::Percent(20.0),
                TrackMaxBreadth::Percent(20.0)
            ))
        );
        assert_eq!(
            parse_track_size("1fr"),
            Some(track_size(TrackBreadth::Auto, TrackMaxBreadth::Fr(1.0)))
        );
        assert_eq!(
            parse_track_size("min-content"),
            Some(track_size(
                TrackBreadth::MinContent,
                TrackMaxBreadth::MinContent
            ))
        );
        assert_eq!(
            parse_track_size("max-content"),
            Some(track_size(
                TrackBreadth::MaxContent,
                TrackMaxBreadth::MaxContent
            ))
        );
        assert_eq!(
            parse_track_size("minmax(100px, 200px)"),
            Some(track_size(
                TrackBreadth::Px(100.0),
                TrackMaxBreadth::Px(200.0)
            ))
        );
        assert_eq!(
            parse_track_size("minmax(80px, max-content)"),
            Some(track_size(
                TrackBreadth::Px(80.0),
                TrackMaxBreadth::MaxContent
            ))
        );
        // Correction 2: `auto` is a valid minmax MAX bound.
        assert_eq!(
            parse_track_size("minmax(80px, auto)"),
            Some(track_size(TrackBreadth::Px(80.0), TrackMaxBreadth::Auto))
        );
        // Correction 3: percent minmax bounds keep their unit.
        assert_eq!(
            parse_track_size("minmax(10%, 200px)"),
            Some(track_size(
                TrackBreadth::Percent(10.0),
                TrackMaxBreadth::Px(200.0)
            ))
        );
        // `em` is not admitted (no parse-time font size for the Px breadth).
        assert_eq!(parse_track_size("2em"), None);
        assert_eq!(parse_track_size("bogus"), None);
    }

    #[test]
    fn grid_track_size_rejects_negative_fr_and_percent() {
        // CSS Grid §7.1: `<flex>` is non-negative and negative track sizes are
        // invalid. The four sign-check sites: the leaf `fr` arm, the leaf `%`
        // arm, and both minmax bound percent arms.
        assert_eq!(parse_track_size("-1fr"), None);
        assert_eq!(parse_track_size("-10%"), None);
        assert_eq!(parse_track_size("minmax(-5%, 100px)"), None);
        assert_eq!(parse_track_size("minmax(10%, -50%)"), None);
        // Zero is admitted per spec (`<flex>` is non-negative, zero inclusive).
        assert_eq!(
            parse_track_size("0fr"),
            Some(track_size(TrackBreadth::Auto, TrackMaxBreadth::Fr(0.0)))
        );
        assert_eq!(
            parse_track_size("0%"),
            Some(track_size(
                TrackBreadth::Percent(0.0),
                TrackMaxBreadth::Percent(0.0)
            ))
        );
    }

    #[test]
    fn grid_auto_flow_parses_the_four_combinations() {
        assert_eq!(parse_grid_auto_flow("row"), Some(GridAutoFlowKw::Row));
        assert_eq!(parse_grid_auto_flow("column"), Some(GridAutoFlowKw::Column));
        assert_eq!(
            parse_grid_auto_flow("row dense"),
            Some(GridAutoFlowKw::RowDense)
        );
        assert_eq!(
            parse_grid_auto_flow("dense column"),
            Some(GridAutoFlowKw::ColumnDense)
        );
        // `dense` alone defaults to row flow; duplicates and unknowns drop.
        assert_eq!(
            parse_grid_auto_flow("dense"),
            Some(GridAutoFlowKw::RowDense)
        );
        assert_eq!(parse_grid_auto_flow("row row"), None);
        assert_eq!(parse_grid_auto_flow("dense dense"), None);
        assert_eq!(parse_grid_auto_flow("row column"), None);
        assert_eq!(parse_grid_auto_flow("row extra"), None);
    }

    #[test]
    fn grid_track_list_interns_expanded_typed_sequences() {
        let mut store = DeclStore::default();
        let key = parse_track_list("repeat(3, 1fr) 100px", &mut store).expect("track list");
        let list = store.resolve_track_list(key).expect("resolve");
        assert_eq!(
            list.tracks.as_slice(),
            &[
                track_size(TrackBreadth::Auto, TrackMaxBreadth::Fr(1.0)),
                track_size(TrackBreadth::Auto, TrackMaxBreadth::Fr(1.0)),
                track_size(TrackBreadth::Auto, TrackMaxBreadth::Fr(1.0)),
                track_size(TrackBreadth::Px(100.0), TrackMaxBreadth::Px(100.0)),
            ]
        );
        assert!(list.line_names.is_empty());
    }

    #[test]
    fn grid_track_list_interns_line_names() {
        let mut store = DeclStore::default();
        let key = parse_track_list("[main] 1fr [aside]", &mut store).expect("track list");
        let list = store.resolve_track_list(key).expect("resolve");
        assert_eq!(list.tracks.len(), 1);
        // `main` labels line 0 (before the track), `aside` labels line 1 (after).
        assert_eq!(list.line_names.len(), 2);
        assert_eq!(
            store.resolve(list.line_names[0].0),
            Some("main"),
            "first registration is main"
        );
        assert_eq!(list.line_names[0].1, 0);
        assert_eq!(store.resolve(list.line_names[1].0), Some("aside"));
        assert_eq!(list.line_names[1].1, 1);
    }

    #[test]
    fn grid_track_list_gates_repeat_and_line_names() {
        let mut store = DeclStore::default();
        // Over capacity (> 12 expanded tracks).
        assert_eq!(parse_track_list("repeat(13, 1fr)", &mut store), None);
        // Zero / negative / non-integer repeat counts.
        assert_eq!(parse_track_list("repeat(0, 1fr)", &mut store), None);
        assert_eq!(parse_track_list("repeat(-1, 1fr)", &mut store), None);
        assert_eq!(parse_track_list("repeat(1.5, 1fr)", &mut store), None);
        // A nested repeat.
        assert_eq!(
            parse_track_list("repeat(2, repeat(3, 1fr))", &mut store),
            None
        );
        // A name labeling more than four lines drops + counts (the D6 gate).
        assert_eq!(
            parse_track_list("[m] 1fr [m] 1fr [m] 1fr [m] 1fr [m] 1fr", &mut store),
            None
        );
        // More than eight name registrations drops + counts.
        assert_eq!(
            parse_track_list(
                "[a] 1fr [b] 1fr [c] 1fr [d] 1fr [e] 1fr [f] 1fr [g] 1fr [h] 1fr [i] 1fr",
                &mut store
            ),
            None
        );
        // A valid two-track list still interns (the control).
        assert!(parse_track_list("1fr 1fr", &mut store).is_some());
    }

    #[test]
    fn grid_track_list_auto_repeat_stores_one_span_copy() {
        let mut store = DeclStore::default();
        let key = parse_track_list("repeat(auto-fit, minmax(240px, 1fr))", &mut store)
            .expect("auto-fit admits");
        let list = store.resolve_track_list(key).expect("resolve");
        // ONE copy of the inner track plus the span marker, not N expanded copies.
        assert_eq!(list.tracks.len(), 1);
        assert_eq!(
            list.tracks[0],
            track_size(TrackBreadth::Px(240.0), TrackMaxBreadth::Fr(1.0))
        );
        assert_eq!(list.auto_repeat_span, Some((0, 1, GridRepeat::AutoFit)));

        let key = parse_track_list("repeat(auto-fill, 100px 200px)", &mut store)
            .expect("auto-fill admits");
        let list = store.resolve_track_list(key).expect("resolve");
        assert_eq!(list.tracks.len(), 2);
        assert_eq!(list.auto_repeat_span, Some((0, 2, GridRepeat::AutoFill)));
    }

    #[test]
    fn grid_track_list_auto_repeat_stores_prefix_and_suffix() {
        let mut store = DeclStore::default();
        let key = parse_track_list("100px repeat(auto-fit, 200px) 300px", &mut store)
            .expect("prefix + span + suffix admits");
        let list = store.resolve_track_list(key).expect("resolve");
        assert_eq!(list.tracks.len(), 3);
        assert_eq!(list.auto_repeat_span, Some((1, 1, GridRepeat::AutoFit)));
    }

    #[test]
    fn grid_track_list_auto_repeat_named_lines_store_once() {
        let mut store = DeclStore::default();
        let key = parse_track_list("repeat(auto-fit, [main] minmax(240px, 1fr))", &mut store)
            .expect("auto-fit with names admits");
        let list = store.resolve_track_list(key).expect("resolve");
        assert_eq!(list.tracks.len(), 1);
        assert_eq!(list.auto_repeat_span, Some((0, 1, GridRepeat::AutoFit)));
        assert_eq!(list.line_names.len(), 1);
        assert_eq!(store.resolve(list.line_names[0].0), Some("main"));
        assert_eq!(list.line_names[0].1, 0);
    }

    #[test]
    fn grid_track_list_auto_repeat_bare_fr_drops() {
        let mut store = DeclStore::default();
        // A bare `fr` span track is the deferred content-based repetition basis
        // (the zero-basis case): drop + count at parse.
        assert_eq!(parse_track_list("repeat(auto-fit, 1fr)", &mut store), None);
        assert_eq!(parse_track_list("repeat(auto-fill, 1fr)", &mut store), None);
        assert_eq!(parse_track_list("repeat(auto-fit, 2fr)", &mut store), None);
        // A `fr` MAX inside minmax (the idiom) is admitted.
        assert!(parse_track_list("repeat(auto-fit, minmax(240px, 1fr))", &mut store).is_some());
        // A fixed repeat of a bare fr still expands (bare-fr gate is span-only).
        assert!(parse_track_list("repeat(3, 1fr)", &mut store).is_some());
    }

    #[test]
    fn grid_track_list_auto_repeat_nested_empty_and_second_span_drop() {
        let mut store = DeclStore::default();
        // Nested repeat inside the span.
        assert_eq!(
            parse_track_list("repeat(auto-fit, repeat(2, 1fr))", &mut store),
            None
        );
        // Missing / empty inner list.
        assert_eq!(parse_track_list("repeat(auto-fill)", &mut store), None);
        assert_eq!(parse_track_list("repeat(auto-fill, )", &mut store), None);
        // A second auto-repeat span has no field to carry it (CSS §7.2.3).
        assert_eq!(
            parse_track_list(
                "repeat(auto-fit, 100px) repeat(auto-fill, 200px)",
                &mut store
            ),
            None
        );
    }

    #[test]
    fn grid_track_list_auto_repeat_names_only_span_drops() {
        let mut store = DeclStore::default();
        // §7.2.3's `<auto-repeat>` requires ≥1 `<track-size>`: a names-only
        // inner list stores zero track sizes and would divide by zero at
        // layout — drop + count at parse (B1).
        assert_eq!(parse_track_list("repeat(auto-fill, [x])", &mut store), None);
        assert_eq!(parse_track_list("repeat(auto-fit, [x])", &mut store), None);
        assert_eq!(
            parse_track_list("repeat(auto-fill, [x] [y])", &mut store),
            None
        );
        // A name PLUS a track size still admits (the ≥1 track-size control).
        assert!(parse_track_list("repeat(auto-fill, [x] 100px)", &mut store).is_some());
    }

    #[test]
    fn grid_track_list_row_axis_content_basis_drops() {
        let mut store = DeclStore::default();
        // Row-axis content-based span tracks (min-content/max-content/auto) drop
        // + count: their repetition basis would be INLINE-axis contributions,
        // wrong for the block axis (D1/NB1).
        assert_eq!(
            parse_row_track_list("repeat(auto-fit, min-content)", &mut store),
            None
        );
        assert_eq!(
            parse_row_track_list("repeat(auto-fill, max-content)", &mut store),
            None
        );
        assert_eq!(
            parse_row_track_list("repeat(auto-fit, auto)", &mut store),
            None
        );
        // The column axis keeps content bases (correct axis): still admitted.
        assert!(parse_track_list("repeat(auto-fit, min-content)", &mut store).is_some());
        // A definite minmax on the row axis (min/max px) still admits.
        assert!(parse_row_track_list("repeat(auto-fit, minmax(40px, 1fr))", &mut store).is_some());
    }
}
