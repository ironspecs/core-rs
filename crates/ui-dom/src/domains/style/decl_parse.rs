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

use smallvec::SmallVec;

use super::decl_block::DeclBlock;
use super::decl_drop_report::DeclDropReport;
use super::decl_value::{DeclLength, DeclValue};
use super::declaration::Declaration;
use super::keyword::Keyword;
use super::parsed_declaration::ParsedDeclaration;
use super::presentation_property::{DeclGrammar, PresentationProperty};
use super::text_interner::TextInterner;
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
    interner: &mut TextInterner,
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
        let Some(declarations) = expand_shape(shape, value, interner) else {
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
    interner: &mut TextInterner,
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
    let declarations =
        expand_shape(shape, value, interner).ok_or(DomError::InvalidAttributeValue)?;
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
    if after.len() >= "important".len()
        && after[.."important".len()].eq_ignore_ascii_case("important")
        && after["important".len()..].trim().is_empty()
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
fn property_shape(name: &str) -> Option<PropertyShape> {
    if let Some(property) = PresentationProperty::from_name(name) {
        return Some(PropertyShape::Longhand(property));
    }
    let shorthand = match name {
        "border-radius" => Shorthand::BorderRadius,
        "border-color" => Shorthand::BorderColor,
        "gap" => Shorthand::Gap,
        "flex" => Shorthand::Flex,
        "flex-flow" => Shorthand::FlexFlow,
        "place-content" => Shorthand::PlaceContent,
        "place-items" => Shorthand::PlaceItems,
        "place-self" => Shorthand::PlaceSelf,
        "transition" => Shorthand::Transition,
        "border-top" => Shorthand::BorderSide(BorderSide::Top),
        "border-right" => Shorthand::BorderSide(BorderSide::Right),
        "border-bottom" => Shorthand::BorderSide(BorderSide::Bottom),
        "border-left" => Shorthand::BorderSide(BorderSide::Left),
        "margin" => Shorthand::Margin,
        "padding" => Shorthand::Padding,
        _ => return None,
    };
    Some(PropertyShape::Shorthand(shorthand))
}

/// Resolves a property name to the longhand set it writes (a longhand maps
/// to itself; a shorthand to its expansion set). Unknown names return `None`.
///
/// This is the `remove`-contract mirror of `expand_shape`: it names the
/// longhands a write touches without parsing a value.
pub(crate) fn longhands_for_name(name: &str) -> Option<SmallVec<[PresentationProperty; 4]>> {
    let shape = property_shape(name)?;
    Some(match shape {
        PropertyShape::Longhand(property) => SmallVec::from_slice(&[property]),
        PropertyShape::Shorthand(shorthand) => longhands_for_shorthand(shorthand),
    })
}

/// True when `name` is a presentation-property write: one of the 69 longhand
/// names or one of the 15 shorthand names (the `longhands_for_name` admission
/// surface). The D5 enforcement gate (`set_attr_str`) consults this BEFORE
/// `DomAttribute::try_from`, so a presentation name never reaches the content
/// vocabulary's `Custom` fallthrough; `set_attr` applies the same check to
/// `Custom(name)` typed writes.
pub(crate) fn name_is_presentation(name: &str) -> bool {
    longhands_for_name(name).is_some()
}

/// The longhand set a shorthand expands to, in expansion order. Names only;
/// the value grammar is `expand_shorthand`'s concern.
fn longhands_for_shorthand(shorthand: Shorthand) -> SmallVec<[PresentationProperty; 4]> {
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
        Shorthand::PlaceItems => &[PresentationProperty::AlignItems],
        Shorthand::PlaceSelf => &[PresentationProperty::AlignSelf],
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
    interner: &mut TextInterner,
) -> Option<Vec<Declaration>> {
    match shape {
        PropertyShape::Longhand(property) => {
            let parsed = parse_property_value(property, value, interner)?;
            Some(vec![declaration(property, parsed)])
        }
        PropertyShape::Shorthand(shorthand) => expand_shorthand(shorthand, value, interner),
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
    interner: &mut TextInterner,
) -> Option<DeclValue> {
    if value == "auto" && AUTO_ADMISSIBLE.contains(&property) {
        return Some(DeclValue::Keyword(Keyword::Auto));
    }
    parse_value(property.grammar(), value, interner)
}

fn expand_shorthand(
    shorthand: Shorthand,
    value: &str,
    interner: &mut TextInterner,
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
        Shorthand::Transition => expand_transition(value, interner),
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
fn parse_value(
    grammar: DeclGrammar,
    value: &str,
    interner: &mut TextInterner,
) -> Option<DeclValue> {
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
        DeclGrammar::Transform => parse_transform(value, interner),
        DeclGrammar::Text => parse_text(value, interner),
        DeclGrammar::Keywords(set) => Keyword::from_name_in(set, value).map(DeclValue::Keyword),
        DeclGrammar::TransitionPropertyList => parse_transition_property_list(value, interner),
        DeclGrammar::TransitionTimeList => parse_transition_time_list(value, interner),
        DeclGrammar::TransitionSignedTimeList => parse_transition_signed_time_list(value, interner),
        DeclGrammar::TransitionTimingList => parse_transition_timing_list(value, interner),
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

// — Text/transform family — //

fn parse_transform(value: &str, interner: &mut TextInterner) -> Option<DeclValue> {
    if value.trim() == "none" {
        return Some(DeclValue::Keyword(Keyword::None));
    }
    if parse_translate_text(value) {
        Some(DeclValue::Text(interner.intern(value)))
    } else {
        None
    }
}

fn parse_text(value: &str, interner: &mut TextInterner) -> Option<DeclValue> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(DeclValue::Text(interner.intern(trimmed)))
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

fn parse_transition_property_list(value: &str, interner: &mut TextInterner) -> Option<DeclValue> {
    let items = comma_split_trim(value)?;
    for &item in &items {
        if item == "all" || item == "none" {
            continue;
        }
        PresentationProperty::from_name(item)?;
    }
    Some(DeclValue::Text(interner.intern(value)))
}

fn parse_transition_time_list(value: &str, interner: &mut TextInterner) -> Option<DeclValue> {
    parse_transition_time_list_impl(value, interner, false)
}

fn parse_transition_signed_time_list(
    value: &str,
    interner: &mut TextInterner,
) -> Option<DeclValue> {
    parse_transition_time_list_impl(value, interner, true)
}

fn parse_transition_time_list_impl(
    value: &str,
    interner: &mut TextInterner,
    allow_negative: bool,
) -> Option<DeclValue> {
    let items = comma_split_trim(value)?;
    for item in &items {
        if !parse_time(item, allow_negative) {
            return None;
        }
    }
    Some(DeclValue::Text(interner.intern(value)))
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

fn parse_transition_timing_list(value: &str, interner: &mut TextInterner) -> Option<DeclValue> {
    let items = comma_split_trim(value)?;
    for item in &items {
        Keyword::from_name_in(TRANSITION_TIMING_KEYWORDS, item)?;
    }
    Some(DeclValue::Text(interner.intern(value)))
}

// — Shared shorthand helpers — //

/// The closed justify-items/justify-self vocabulary validated (and discarded)
/// for the second token of `place-items`/`place-self`. These properties are
/// not admitted longhands, so the value is checked and then dropped.
const JUSTIFY_ITEMS_SELF_KEYWORDS: &[Keyword] = &[
    Keyword::Auto,
    Keyword::Normal,
    Keyword::Stretch,
    Keyword::Start,
    Keyword::End,
    Keyword::FlexStart,
    Keyword::FlexEnd,
    Keyword::Center,
];

/// Reads a `Keywords` grammar off a property that is known to use one. A
/// self-owned structural invariant: these properties always declare a
/// `Keywords` grammar in the presentation registry.
fn keyword_set(property: PresentationProperty) -> &'static [Keyword] {
    match property.grammar() {
        DeclGrammar::Keywords(set) => set,
        _ => unreachable!("{property:?} must use the Keywords grammar"),
    }
}

/// Splits on whitespace only at paren depth 0, so `rgb(255, 0, 0)` stays one
/// token even when it contains spaces.
fn split_ws_paren_aware(value: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut depth = 0usize;
    let mut token_start = 0usize;
    let mut in_token = false;
    for (index, ch) in value.char_indices() {
        if ch == '(' {
            depth = depth.saturating_add(1);
            if !in_token {
                token_start = index;
                in_token = true;
            }
            continue;
        }
        if ch == ')' {
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
    let mut scratch = TextInterner::default();
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
    let align_token = match tokens.as_slice() {
        [align] => *align,
        [align, justify] => {
            Keyword::from_name_in(JUSTIFY_ITEMS_SELF_KEYWORDS, justify)?;
            *align
        }
        _ => return None,
    };
    let align = Keyword::from_name_in(align_set, align_token)?;
    Some(vec![declaration(
        PresentationProperty::AlignItems,
        DeclValue::Keyword(align),
    )])
}

fn expand_place_self(value: &str) -> Option<Vec<Declaration>> {
    let tokens = split_ws_paren_aware(value);
    let align_set = keyword_set(PresentationProperty::AlignSelf);
    let align_token = match tokens.as_slice() {
        [align] => *align,
        [align, justify] => {
            Keyword::from_name_in(JUSTIFY_ITEMS_SELF_KEYWORDS, justify)?;
            *align
        }
        _ => return None,
    };
    let align = Keyword::from_name_in(align_set, align_token)?;
    Some(vec![declaration(
        PresentationProperty::AlignSelf,
        DeclValue::Keyword(align),
    )])
}

fn expand_transition(value: &str, interner: &mut TextInterner) -> Option<Vec<Declaration>> {
    let authored = DomAttributeValue::String(value.to_owned());
    let longhands = transition_longhand_values_from_shorthand(Some(&authored)).ok()?;
    let property = transition_string(longhands.property.as_ref()?)?;
    let duration = transition_string(longhands.duration.as_ref()?)?;
    let timing = transition_string(longhands.timing_function.as_ref()?)?;
    let delay = transition_string(longhands.delay.as_ref()?)?;
    Some(vec![
        declaration(
            PresentationProperty::TransitionProperty,
            DeclValue::Text(interner.intern(property)),
        ),
        declaration(
            PresentationProperty::TransitionDuration,
            DeclValue::Text(interner.intern(duration)),
        ),
        declaration(
            PresentationProperty::TransitionTimingFunction,
            DeclValue::Text(interner.intern(timing)),
        ),
        declaration(
            PresentationProperty::TransitionDelay,
            DeclValue::Text(interner.intern(delay)),
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
        interner: &'a TextInterner,
        property: PresentationProperty,
    ) -> &'a str {
        let value = block
            .get(property)
            .unwrap_or_else(|| panic!("expected {property:?} in block"));
        match value {
            DeclValue::Text(key) => interner
                .resolve(*key)
                .unwrap_or_else(|| panic!("text key {key:?} should resolve")),
            other => panic!("expected text value for {property:?}, got {other:?}"),
        }
    }

    fn parse_block(text: &str) -> (DeclBlock, DeclDropReport, TextInterner) {
        let mut interner = TextInterner::default();
        let (block, report) = parse_declaration_block(text, &mut interner);
        (block, report, interner)
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
        let mut interner = TextInterner::default();
        assert_eq!(
            parse_value(DeclGrammar::Length, "0", &mut interner),
            Some(DeclValue::Length(DeclLength::Px(0.0)))
        );
        assert_eq!(
            parse_value(DeclGrammar::Length, "10px", &mut interner),
            Some(DeclValue::Length(DeclLength::Px(10.0)))
        );
        assert_eq!(
            parse_value(DeclGrammar::Length, "1.5em", &mut interner),
            Some(DeclValue::Length(DeclLength::Em(1.5)))
        );
        assert_eq!(
            parse_value(DeclGrammar::Length, "unset", &mut interner),
            Some(DeclValue::Length(DeclLength::Unset))
        );
        assert_eq!(
            parse_value(DeclGrammar::Length, "50%", &mut interner),
            Some(DeclValue::Length(DeclLength::Percent(50.0)))
        );
        assert_eq!(
            parse_value(DeclGrammar::Length, "-10%", &mut interner),
            Some(DeclValue::Length(DeclLength::Percent(-10.0)))
        );
    }

    #[test]
    fn length_grammar_rejects_unknown_units_and_non_finite() {
        let mut interner = TextInterner::default();
        for invalid in [
            "10", "10vh", "NaNpx", "infpx", "-infpx", "10%%", "10.5%px", "",
        ] {
            assert_eq!(
                parse_value(DeclGrammar::Length, invalid, &mut interner),
                None,
                "{invalid:?} should fail the Length grammar"
            );
        }
    }

    #[test]
    fn non_negative_length_grammar() {
        let mut interner = TextInterner::default();
        assert_eq!(
            parse_value(DeclGrammar::NonNegativeLength, "unset", &mut interner),
            Some(DeclValue::Length(DeclLength::Unset))
        );
        assert_eq!(
            parse_value(DeclGrammar::NonNegativeLength, "0", &mut interner),
            Some(DeclValue::Length(DeclLength::Px(0.0)))
        );
        assert_eq!(
            parse_value(DeclGrammar::NonNegativeLength, "10px", &mut interner),
            Some(DeclValue::Length(DeclLength::Px(10.0)))
        );
        assert_eq!(
            parse_value(DeclGrammar::NonNegativeLength, "1.5em", &mut interner),
            Some(DeclValue::Length(DeclLength::Em(1.5)))
        );
        assert_eq!(
            parse_value(DeclGrammar::NonNegativeLength, "50%", &mut interner),
            Some(DeclValue::Length(DeclLength::Percent(50.0)))
        );
        for invalid in ["-1px", "-0.5em", "-10%"] {
            assert_eq!(
                parse_value(DeclGrammar::NonNegativeLength, invalid, &mut interner),
                None,
                "{invalid:?} should fail the NonNegativeLength grammar"
            );
        }
    }

    #[test]
    fn line_width_grammar_rejects_percentages() {
        let mut interner = TextInterner::default();
        assert_eq!(
            parse_value(DeclGrammar::LineWidth, "unset", &mut interner),
            Some(DeclValue::Length(DeclLength::Unset))
        );
        assert_eq!(
            parse_value(DeclGrammar::LineWidth, "0", &mut interner),
            Some(DeclValue::Length(DeclLength::Px(0.0)))
        );
        assert_eq!(
            parse_value(DeclGrammar::LineWidth, "10px", &mut interner),
            Some(DeclValue::Length(DeclLength::Px(10.0)))
        );
        assert_eq!(
            parse_value(DeclGrammar::LineWidth, "1.5em", &mut interner),
            Some(DeclValue::Length(DeclLength::Em(1.5)))
        );
        for invalid in ["10%", "-10%", "-1px", "-0.5em"] {
            assert_eq!(
                parse_value(DeclGrammar::LineWidth, invalid, &mut interner),
                None,
                "{invalid:?} should fail the LineWidth grammar"
            );
        }
    }

    #[test]
    fn signed_line_width_grammar_rejects_percentages() {
        let mut interner = TextInterner::default();
        assert_eq!(
            parse_value(DeclGrammar::SignedLineWidth, "unset", &mut interner),
            Some(DeclValue::Length(DeclLength::Unset))
        );
        assert_eq!(
            parse_value(DeclGrammar::SignedLineWidth, "0", &mut interner),
            Some(DeclValue::Length(DeclLength::Px(0.0)))
        );
        assert_eq!(
            parse_value(DeclGrammar::SignedLineWidth, "-10px", &mut interner),
            Some(DeclValue::Length(DeclLength::Px(-10.0)))
        );
        assert_eq!(
            parse_value(DeclGrammar::SignedLineWidth, "-0.5em", &mut interner),
            Some(DeclValue::Length(DeclLength::Em(-0.5)))
        );
        for invalid in ["10%", "-10%", "50%"] {
            assert_eq!(
                parse_value(DeclGrammar::SignedLineWidth, invalid, &mut interner),
                None,
                "{invalid:?} should fail the SignedLineWidth grammar"
            );
        }
    }

    #[test]
    fn gap_grammar_accepts_normal_and_non_negative_lengths() {
        let mut interner = TextInterner::default();
        assert_eq!(
            parse_value(DeclGrammar::Gap, "normal", &mut interner),
            Some(DeclValue::Length(DeclLength::Px(0.0)))
        );
        assert_eq!(
            parse_value(DeclGrammar::Gap, "unset", &mut interner),
            Some(DeclValue::Length(DeclLength::Unset))
        );
        assert_eq!(
            parse_value(DeclGrammar::Gap, "0", &mut interner),
            Some(DeclValue::Length(DeclLength::Px(0.0)))
        );
        assert_eq!(
            parse_value(DeclGrammar::Gap, "4px", &mut interner),
            Some(DeclValue::Length(DeclLength::Px(4.0)))
        );
        assert_eq!(
            parse_value(DeclGrammar::Gap, "1.5em", &mut interner),
            Some(DeclValue::Length(DeclLength::Em(1.5)))
        );
        for invalid in ["-1px", "-10%", "10%", "bogus"] {
            assert_eq!(
                parse_value(DeclGrammar::Gap, invalid, &mut interner),
                None,
                "{invalid:?} should fail the Gap grammar"
            );
        }
    }

    #[test]
    fn auto_keyword_admission_is_closed_per_property() {
        let mut interner = TextInterner::default();
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
                parse_property_value(admitted, "auto", &mut interner),
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
                parse_property_value(rejected, "auto", &mut interner),
                None,
                "{rejected:?} should reject auto"
            );
        }
    }

    #[test]
    fn line_height_accepts_normal_number_and_non_negative_lengths() {
        let mut interner = TextInterner::default();
        assert_eq!(
            parse_value(DeclGrammar::LineHeight, "normal", &mut interner),
            Some(DeclValue::Keyword(Keyword::Normal))
        );
        assert_eq!(
            parse_value(DeclGrammar::LineHeight, "1.5", &mut interner),
            Some(DeclValue::Number(1.5))
        );
        assert_eq!(
            parse_value(DeclGrammar::LineHeight, "0", &mut interner),
            Some(DeclValue::Number(0.0))
        );
        assert_eq!(
            parse_value(DeclGrammar::LineHeight, "20px", &mut interner),
            Some(DeclValue::Length(DeclLength::Px(20.0)))
        );
        assert_eq!(
            parse_value(DeclGrammar::LineHeight, "1.25em", &mut interner),
            Some(DeclValue::Length(DeclLength::Em(1.25)))
        );
        assert_eq!(
            parse_value(DeclGrammar::LineHeight, "150%", &mut interner),
            Some(DeclValue::Length(DeclLength::Percent(150.0)))
        );
    }

    #[test]
    fn line_height_rejects_negatives_and_non_finite() {
        let mut interner = TextInterner::default();
        for invalid in ["-120%", "-1", "-2px", "-0.5em", "NaN", "inf", "unset"] {
            assert_eq!(
                parse_value(DeclGrammar::LineHeight, invalid, &mut interner),
                None,
                "{invalid:?} should fail the LineHeight grammar"
            );
        }
    }

    #[test]
    fn color_grammar_accepts_and_rejects() {
        let mut interner = TextInterner::default();
        assert_eq!(
            parse_value(DeclGrammar::Color, "transparent", &mut interner),
            Some(DeclValue::Color([0.0, 0.0, 0.0, 0.0]))
        );
        assert_eq!(
            parse_value(DeclGrammar::Color, "red", &mut interner),
            Some(DeclValue::Color([1.0, 0.0, 0.0, 1.0]))
        );
        assert_eq!(
            parse_value(DeclGrammar::Color, "#112233", &mut interner),
            Some(DeclValue::Color([
                17.0 / 255.0,
                34.0 / 255.0,
                51.0 / 255.0,
                1.0
            ]))
        );
        assert_eq!(
            parse_value(DeclGrammar::Color, "notacolor", &mut interner),
            None
        );
    }

    #[test]
    fn opacity_clamps_to_unit_range() {
        let mut interner = TextInterner::default();
        assert_eq!(
            parse_value(DeclGrammar::Opacity, "0.5", &mut interner),
            Some(DeclValue::Number(0.5))
        );
        assert_eq!(
            parse_value(DeclGrammar::Opacity, "0", &mut interner),
            Some(DeclValue::Number(0.0))
        );
        assert_eq!(
            parse_value(DeclGrammar::Opacity, "1", &mut interner),
            Some(DeclValue::Number(1.0))
        );
        assert_eq!(
            parse_value(DeclGrammar::Opacity, "1.5", &mut interner),
            Some(DeclValue::Number(1.0))
        );
        assert_eq!(
            parse_value(DeclGrammar::Opacity, "-0.5", &mut interner),
            Some(DeclValue::Number(0.0))
        );
        for invalid in ["NaN", "inf", "-inf"] {
            assert_eq!(
                parse_value(DeclGrammar::Opacity, invalid, &mut interner),
                None,
                "{invalid:?} should fail the Opacity grammar"
            );
        }
    }

    #[test]
    fn non_negative_number_and_integer_grammars() {
        let mut interner = TextInterner::default();
        assert_eq!(
            parse_value(DeclGrammar::NonNegativeNumber, "2.5", &mut interner),
            Some(DeclValue::Number(2.5))
        );
        assert_eq!(
            parse_value(DeclGrammar::NonNegativeNumber, "-1", &mut interner),
            None
        );
        assert_eq!(
            parse_value(DeclGrammar::NonNegativeNumber, "inf", &mut interner),
            None
        );
        assert_eq!(
            parse_value(DeclGrammar::Integer, "-3", &mut interner),
            Some(DeclValue::Integer(-3))
        );
        assert_eq!(
            parse_value(DeclGrammar::Integer, "4.5", &mut interner),
            None
        );
    }

    #[test]
    fn font_weight_keywords_and_step_constraints() {
        let mut interner = TextInterner::default();
        assert_eq!(
            parse_value(DeclGrammar::FontWeight, "normal", &mut interner),
            Some(DeclValue::Keyword(Keyword::Normal))
        );
        assert_eq!(
            parse_value(DeclGrammar::FontWeight, "bolder", &mut interner),
            Some(DeclValue::Keyword(Keyword::Bolder))
        );
        assert_eq!(
            parse_value(DeclGrammar::FontWeight, "600", &mut interner),
            Some(DeclValue::Integer(600))
        );
        assert_eq!(
            parse_value(DeclGrammar::FontWeight, "900", &mut interner),
            Some(DeclValue::Integer(900))
        );
        for invalid in ["450", "950", "99", "semibold"] {
            assert_eq!(
                parse_value(DeclGrammar::FontWeight, invalid, &mut interner),
                None,
                "{invalid:?} should fail the FontWeight grammar"
            );
        }
    }

    #[test]
    fn flex_basis_grammar_forms() {
        let mut interner = TextInterner::default();
        assert_eq!(
            parse_value(DeclGrammar::FlexBasis, "auto", &mut interner),
            Some(DeclValue::Keyword(Keyword::Auto))
        );
        assert_eq!(
            parse_value(DeclGrammar::FlexBasis, "content", &mut interner),
            Some(DeclValue::Keyword(Keyword::Content))
        );
        assert_eq!(
            parse_value(DeclGrammar::FlexBasis, "0", &mut interner),
            Some(DeclValue::Length(DeclLength::Px(0.0)))
        );
        assert_eq!(
            parse_value(DeclGrammar::FlexBasis, "20px", &mut interner),
            Some(DeclValue::Length(DeclLength::Px(20.0)))
        );
        assert_eq!(
            parse_value(DeclGrammar::FlexBasis, "unset", &mut interner),
            None
        );
    }

    #[test]
    fn flex_basis_rejects_negative_lengths() {
        let mut interner = TextInterner::default();
        assert_eq!(
            parse_value(DeclGrammar::FlexBasis, "-10px", &mut interner),
            None
        );
        for accepted in ["10px", "auto", "content", "0"] {
            assert!(
                parse_value(DeclGrammar::FlexBasis, accepted, &mut interner).is_some(),
                "{accepted:?} should parse"
            );
        }
    }

    #[test]
    fn transform_grammar_accepts_none_and_closed_translate() {
        let mut interner = TextInterner::default();
        assert_eq!(
            parse_value(DeclGrammar::Transform, "none", &mut interner),
            Some(DeclValue::Keyword(Keyword::None))
        );
        for accepted in [
            "translateX(10px)",
            "translateY(5)",
            "translate(10px, 20px)",
            "translate(-3px, 4)",
        ] {
            let parsed = parse_value(DeclGrammar::Transform, accepted, &mut interner)
                .unwrap_or_else(|| panic!("{accepted:?} should parse"));
            let DeclValue::Text(key) = parsed else {
                panic!("{accepted:?} should intern to text");
            };
            assert_eq!(interner.resolve(key), Some(accepted));
        }
    }

    #[test]
    fn transform_grammar_rejects_trailing_garbage_and_bad_shapes() {
        let mut interner = TextInterner::default();
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
                parse_value(DeclGrammar::Transform, invalid, &mut interner),
                None,
                "{invalid:?} should fail the Transform grammar"
            );
        }
    }

    #[test]
    fn text_grammar_interns_non_empty_and_rejects_empty() {
        let mut interner = TextInterner::default();
        let parsed = parse_value(DeclGrammar::Text, "ui-sans, sans-serif", &mut interner)
            .expect("font-family should parse");
        let DeclValue::Text(key) = parsed else {
            panic!("font-family should intern to text");
        };
        assert_eq!(interner.resolve(key), Some("ui-sans, sans-serif"));
        assert_eq!(parse_value(DeclGrammar::Text, "   ", &mut interner), None);
    }

    #[test]
    fn transition_property_list_grammar() {
        let mut interner = TextInterner::default();
        let parsed = parse_value(
            DeclGrammar::TransitionPropertyList,
            "all, opacity",
            &mut interner,
        )
        .expect("admitted list should parse");
        let DeclValue::Text(key) = parsed else {
            panic!("transition-property should intern to text");
        };
        assert_eq!(interner.resolve(key), Some("all, opacity"));

        assert!(
            parse_value(
                DeclGrammar::TransitionPropertyList,
                "width, color",
                &mut interner
            )
            .is_some()
        );
        for invalid in ["mystery", "all, mystery", "all,", ""] {
            assert_eq!(
                parse_value(DeclGrammar::TransitionPropertyList, invalid, &mut interner),
                None,
                "{invalid:?} should fail"
            );
        }
    }

    #[test]
    fn transition_time_list_grammar() {
        let mut interner = TextInterner::default();
        for accepted in ["80ms, 180ms", "0.25s", "0", "120ms"] {
            assert!(
                parse_value(DeclGrammar::TransitionTimeList, accepted, &mut interner).is_some(),
                "{accepted:?} should parse"
            );
        }
        for invalid in ["-20ms", "80", "80ms,", "NaNms", ""] {
            assert_eq!(
                parse_value(DeclGrammar::TransitionTimeList, invalid, &mut interner),
                None,
                "{invalid:?} should fail"
            );
        }
    }

    #[test]
    fn transition_signed_time_list_grammar() {
        let mut interner = TextInterner::default();
        for accepted in ["-20ms", "-12.5ms", "0.25s", "0", "-20ms, 100ms"] {
            let parsed = parse_value(
                DeclGrammar::TransitionSignedTimeList,
                accepted,
                &mut interner,
            )
            .unwrap_or_else(|| panic!("{accepted:?} should parse"));
            let DeclValue::Text(key) = parsed else {
                panic!("{accepted:?} should intern to text");
            };
            assert_eq!(interner.resolve(key), Some(accepted));
        }
        for invalid in ["-20", "80", "80ms,", "NaNms", "infms", "-infms", ""] {
            assert_eq!(
                parse_value(
                    DeclGrammar::TransitionSignedTimeList,
                    invalid,
                    &mut interner
                ),
                None,
                "{invalid:?} should fail"
            );
        }
    }

    #[test]
    fn transition_timing_list_grammar() {
        let mut interner = TextInterner::default();
        assert!(
            parse_value(
                DeclGrammar::TransitionTimingList,
                "ease, linear",
                &mut interner
            )
            .is_some()
        );
        assert!(
            parse_value(
                DeclGrammar::TransitionTimingList,
                "ease-in-out",
                &mut interner
            )
            .is_some()
        );
        for invalid in ["cubic-bezier(0,0,1,1)", "ease,", "fast"] {
            assert_eq!(
                parse_value(DeclGrammar::TransitionTimingList, invalid, &mut interner),
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
        let (block, report, _interner) = parse_block("border-radius: 10px 20px 30px");
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
        let (block, report, _interner) = parse_block("border-color: rgb(255, 0, 0) rgb(0, 0, 255)");
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
    fn place_items_and_place_self_discard_second_token() {
        let (block, _, _) = parse_block("place-items: center start");
        assert_eq!(
            value(&block, PresentationProperty::AlignItems),
            DeclValue::Keyword(Keyword::Center)
        );
        assert_eq!(block.len(), 1);

        let (block, _, _) = parse_block("place-self: end auto");
        assert_eq!(
            value(&block, PresentationProperty::AlignSelf),
            DeclValue::Keyword(Keyword::End)
        );
        assert_eq!(block.len(), 1);

        let (_, report, _) = parse_block("place-items: center baseline");
        assert_eq!(report.dropped, 1);
    }

    #[test]
    fn transition_shorthand_expands_to_normalized_longhands() {
        let (block, report, interner) = parse_block("transition: width 120ms ease-in 40ms");
        assert_eq!(report.applied, 1);
        assert_eq!(
            text(&block, &interner, PresentationProperty::TransitionProperty),
            "width"
        );
        assert_eq!(
            text(&block, &interner, PresentationProperty::TransitionDuration),
            "0.12s"
        );
        assert_eq!(
            text(
                &block,
                &interner,
                PresentationProperty::TransitionTimingFunction
            ),
            "ease-in"
        );
        assert_eq!(
            text(&block, &interner, PresentationProperty::TransitionDelay),
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
        let (block, report, interner) = parse_block(
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
            text(&block, &interner, PresentationProperty::TransitionDelay),
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
        let mut interner = TextInterner::default();
        let declarations = parse_declaration_strict("width", "10px", &mut interner)
            .expect("longhand should parse");
        assert_eq!(declarations.len(), 1);
        assert_eq!(declarations[0].property, PresentationProperty::Width);
        assert_eq!(
            declarations[0].value,
            DeclValue::Length(DeclLength::Px(10.0))
        );
    }

    #[test]
    fn font_size_unset_is_admitted() {
        let mut interner = TextInterner::default();
        let declarations = parse_declaration_strict("font-size", "unset", &mut interner)
            .expect("font-size: unset should parse");
        assert_eq!(declarations.len(), 1);
        assert_eq!(declarations[0].property, PresentationProperty::FontSize);
        assert_eq!(declarations[0].value, DeclValue::Length(DeclLength::Unset));
    }

    #[test]
    fn strict_parse_ok_for_shorthand_in_expansion_order() {
        let mut interner = TextInterner::default();
        let declarations =
            parse_declaration_strict("flex", "1", &mut interner).expect("shorthand should parse");
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
        let mut interner = TextInterner::default();
        assert_eq!(
            parse_declaration_strict("mystery", "1px", &mut interner),
            Err(DomError::InvalidAttributeValue)
        );
        assert_eq!(
            parse_declaration_strict("width", "notalength", &mut interner),
            Err(DomError::InvalidAttributeValue)
        );
        assert_eq!(
            parse_declaration_strict("", "10px", &mut interner),
            Err(DomError::InvalidAttributeValue)
        );
        assert_eq!(
            parse_declaration_strict("width", "", &mut interner),
            Err(DomError::InvalidAttributeValue)
        );
    }

    #[test]
    fn strict_parse_rejects_negative_duration_but_accepts_negative_delay() {
        let mut interner = TextInterner::default();
        assert_eq!(
            parse_declaration_strict("transition-duration", "-20ms", &mut interner),
            Err(DomError::InvalidAttributeValue)
        );
        let declarations = parse_declaration_strict("transition-delay", "-20ms", &mut interner)
            .expect("negative delay should parse");
        assert_eq!(declarations.len(), 1);
        assert_eq!(
            declarations[0].property,
            PresentationProperty::TransitionDelay
        );
        let DeclValue::Text(key) = declarations[0].value else {
            panic!("delay should intern to text");
        };
        assert_eq!(interner.resolve(key), Some("-20ms"));
    }

    // — `!important` marker — //

    #[test]
    fn importance_marker_strips_from_strict_parse_and_carries_the_flag() {
        let mut interner = TextInterner::default();
        let declarations = parse_declaration_strict("width", "10px !important", &mut interner)
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
        let mut interner = TextInterner::default();
        for authored in [
            "10px !important",
            "10px!important",
            "10px ! IMPORTANT",
            "10px !  important",
        ] {
            let declarations = parse_declaration_strict("width", authored, &mut interner)
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
        let mut interner = TextInterner::default();
        let declarations = parse_declaration_strict("width", "10px", &mut interner)
            .expect("plain width should parse");
        assert_eq!(declarations.len(), 1);
        assert!(!declarations[0].important);
    }

    #[test]
    fn importance_marker_is_not_recognized_mid_value() {
        let mut interner = TextInterner::default();
        // `!important` is only a marker at the END of the value string; these
        // values leave the marker in place, so the grammar rejects them.
        assert_eq!(
            parse_declaration_strict("width", "10px !important 5px", &mut interner),
            Err(DomError::InvalidAttributeValue)
        );
        assert_eq!(
            parse_declaration_strict("width", "!important 10px", &mut interner),
            Err(DomError::InvalidAttributeValue)
        );
        // An empty remainder after the marker is malformed.
        assert_eq!(
            parse_declaration_strict("width", "!important", &mut interner),
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
            Some(vec![PresentationProperty::AlignItems])
        );
        assert_eq!(
            longhands_for_name("place-self").map(SmallVec::into_vec),
            Some(vec![PresentationProperty::AlignSelf])
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
}
