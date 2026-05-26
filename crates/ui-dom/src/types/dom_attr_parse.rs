use std::collections::BTreeMap;

use crate::types::{
    DomAttribute, DomAttributeValue, DomError, parse_authored_value_str, parse_text_align_keyword,
    parse_text_direction_keyword, parse_white_space_keyword,
};

const KNOWN_COLOR_ATTRIBUTE_NAMES: &[&str] = &[
    "background-color",
    "color",
    "border-color",
    "border-top-color",
    "border-right-color",
    "border-bottom-color",
    "border-left-color",
    "outline-color",
    "caret-color",
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum AuthoredLength {
    Unset,
    Px(f32),
    Em(f32),
}

pub(crate) fn validate_attribute_value(
    attribute: &DomAttribute,
    value: Option<&DomAttributeValue>,
) -> Result<(), DomError> {
    if is_layout_length_attribute(attribute) {
        return validate_direct_length_attr_with_unset(value);
    }

    match attribute {
        DomAttribute::Id
        | DomAttribute::Style
        | DomAttribute::Class
        | DomAttribute::Transition
        | DomAttribute::TransitionProperty
        | DomAttribute::TransitionDuration
        | DomAttribute::TransitionTimingFunction
        | DomAttribute::TransitionDelay
        | DomAttribute::FontFamily
        | DomAttribute::FontWeight
        | DomAttribute::FontStyle
        | DomAttribute::Label
        | DomAttribute::List
        | DomAttribute::Max
        | DomAttribute::MaxLength
        | DomAttribute::Min
        | DomAttribute::MinLength
        | DomAttribute::Overflow
        | DomAttribute::FlexDirection
        | DomAttribute::FlexWrap
        | DomAttribute::FlexFlow
        | DomAttribute::FlexGrow
        | DomAttribute::FlexShrink
        | DomAttribute::FlexBasis
        | DomAttribute::Flex
        | DomAttribute::Order
        | DomAttribute::JustifyContent
        | DomAttribute::AlignItems
        | DomAttribute::AlignSelf
        | DomAttribute::AlignContent
        | DomAttribute::Gap
        | DomAttribute::RowGap
        | DomAttribute::ColumnGap
        | DomAttribute::PlaceContent
        | DomAttribute::PlaceItems
        | DomAttribute::PlaceSelf
        | DomAttribute::Placeholder
        | DomAttribute::OverflowWrap
        | DomAttribute::WordBreak
        | DomAttribute::Lang
        | DomAttribute::PresentedOptionsRole
        | DomAttribute::PresentedOptionsState
        | DomAttribute::Step
        | DomAttribute::Transform
        | DomAttribute::Src
        | DomAttribute::Type
        | DomAttribute::Value => validate_string_attribute(value),
        DomAttribute::TextAlign => validate_text_align_attribute(value),
        DomAttribute::Dir | DomAttribute::Direction => validate_text_direction_attribute(value),
        DomAttribute::WhiteSpace => validate_white_space_attribute(value),
        DomAttribute::ReadOnly
        | DomAttribute::Disabled
        | DomAttribute::Required
        | DomAttribute::Checked
        | DomAttribute::Selected
        | DomAttribute::PresentedOptionsHighlighted
        | DomAttribute::Multiple => validate_boolean_attribute(value),
        DomAttribute::FontSize => validate_font_size_attribute(value),
        DomAttribute::LineHeight => validate_line_height_value(value),
        DomAttribute::TabIndex | DomAttribute::Size | DomAttribute::PresentedOptionsSourceIndex => {
            validate_integer_attribute(value)
        }
        DomAttribute::Display => validate_display_attribute(value),
        DomAttribute::Position => validate_position_attribute(value),
        DomAttribute::Isolate => validate_isolate_attribute(value),
        DomAttribute::Custom(name) if is_known_color_attribute_name(name) => {
            validate_color_attribute(value)
        }
        DomAttribute::Custom(name) if name == "opacity" => validate_opacity_attribute(value),
        DomAttribute::Custom(_) => Ok(()),
        _ => Err(DomError::InvalidAttributeValue),
    }
}

pub(crate) fn normalize_attribute_value(
    attribute: &DomAttribute,
    value: Option<DomAttributeValue>,
) -> Result<Option<DomAttributeValue>, DomError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if !is_known_color_attribute(attribute) {
        return Ok(Some(value));
    }
    match value {
        DomAttributeValue::String(value) => parse_css_color_rgba(&value)
            .map(DomAttributeValue::Color)
            .map(Some),
        DomAttributeValue::Color(rgba) => {
            validate_normalized_color_rgba(rgba)?;
            Ok(Some(DomAttributeValue::Color(rgba)))
        }
        DomAttributeValue::Bool(_)
        | DomAttributeValue::Integer(_)
        | DomAttributeValue::Float(_) => Err(DomError::InvalidAttributeValue),
    }
}

pub(crate) fn is_known_color_attribute(attribute: &DomAttribute) -> bool {
    matches!(attribute, DomAttribute::Custom(name) if is_known_color_attribute_name(name))
}

fn is_known_color_attribute_name(name: &str) -> bool {
    KNOWN_COLOR_ATTRIBUTE_NAMES.contains(&name)
}

fn validate_color_attribute(value: Option<&DomAttributeValue>) -> Result<(), DomError> {
    match value {
        None => Ok(()),
        Some(DomAttributeValue::Color(rgba)) => validate_normalized_color_rgba(*rgba),
        Some(
            DomAttributeValue::String(_)
            | DomAttributeValue::Bool(_)
            | DomAttributeValue::Integer(_)
            | DomAttributeValue::Float(_),
        ) => Err(DomError::InvalidAttributeValue),
    }
}

fn validate_normalized_color_rgba(rgba: [f32; 4]) -> Result<(), DomError> {
    if rgba
        .iter()
        .all(|channel| channel.is_finite() && (0.0..=1.0).contains(channel))
    {
        return Ok(());
    }
    Err(DomError::InvalidAttributeValue)
}

pub(crate) fn parse_style_attr_map(
    value: &str,
) -> Result<BTreeMap<DomAttribute, DomAttributeValue>, DomError> {
    let mut parsed = BTreeMap::new();
    for declaration in value.split(';') {
        let declaration = declaration.trim();
        if declaration.is_empty() {
            continue;
        }
        let Some((name, raw_value)) = declaration.split_once(':') else {
            return Err(DomError::InvalidAttributeValue);
        };
        if raw_value.contains(':') {
            return Err(DomError::InvalidAttributeValue);
        }
        let name = name.trim();
        let raw_value = raw_value.trim();
        if name.is_empty() || raw_value.is_empty() {
            return Err(DomError::InvalidAttributeValue);
        }
        let attribute = DomAttribute::try_from(name)?;
        if !style_attr_allows_property(&attribute) {
            return Err(DomError::InvalidAttributeValue);
        }
        let value = parse_authored_value_str(&attribute, raw_value)?;
        validate_style_attribute_value(&attribute, Some(&value))?;
        parsed.insert(attribute, value);
    }
    Ok(parsed)
}

pub(crate) fn parse_class_attr_list(value: &str) -> Vec<String> {
    value
        .split_ascii_whitespace()
        .map(ToOwned::to_owned)
        .collect()
}

pub(crate) fn layout_length_attr_uses_unset(attribute: &DomAttribute) -> bool {
    is_layout_length_attribute(attribute)
}

fn validate_display_attribute(value: Option<&DomAttributeValue>) -> Result<(), DomError> {
    match value {
        None => Ok(()),
        Some(DomAttributeValue::String(value)) if is_supported_display_value(value) => Ok(()),
        _ => Err(DomError::InvalidAttributeValue),
    }
}

fn validate_text_align_attribute(value: Option<&DomAttributeValue>) -> Result<(), DomError> {
    match value {
        None => Ok(()),
        Some(DomAttributeValue::String(value)) if parse_text_align_keyword(value).is_some() => {
            Ok(())
        }
        _ => Err(DomError::InvalidAttributeValue),
    }
}

fn validate_text_direction_attribute(value: Option<&DomAttributeValue>) -> Result<(), DomError> {
    match value {
        None => Ok(()),
        Some(DomAttributeValue::String(value)) if parse_text_direction_keyword(value).is_some() => {
            Ok(())
        }
        _ => Err(DomError::InvalidAttributeValue),
    }
}

fn is_supported_display_value(value: &str) -> bool {
    matches!(
        value,
        "block" | "inline" | "inline-block" | "none" | "flex" | "inline-flex"
    )
}

fn validate_string_attribute(value: Option<&DomAttributeValue>) -> Result<(), DomError> {
    match value {
        None | Some(DomAttributeValue::String(_)) => Ok(()),
        _ => Err(DomError::InvalidAttributeValue),
    }
}

fn validate_boolean_attribute(value: Option<&DomAttributeValue>) -> Result<(), DomError> {
    match value {
        None | Some(DomAttributeValue::Bool(_)) => Ok(()),
        Some(DomAttributeValue::String(value)) if matches!(value.as_str(), "true" | "false") => {
            Ok(())
        }
        _ => Err(DomError::InvalidAttributeValue),
    }
}

fn validate_integer_attribute(value: Option<&DomAttributeValue>) -> Result<(), DomError> {
    match value {
        None | Some(DomAttributeValue::Integer(_)) => Ok(()),
        _ => Err(DomError::InvalidAttributeValue),
    }
}

fn validate_isolate_attribute(value: Option<&DomAttributeValue>) -> Result<(), DomError> {
    match value {
        None | Some(DomAttributeValue::Bool(_) | DomAttributeValue::String(_)) => Ok(()),
        _ => Err(DomError::InvalidAttributeValue),
    }
}

fn validate_position_attribute(value: Option<&DomAttributeValue>) -> Result<(), DomError> {
    match value {
        None => Ok(()),
        Some(DomAttributeValue::String(value)) if is_supported_position_value(value) => Ok(()),
        _ => Err(DomError::InvalidAttributeValue),
    }
}

fn is_supported_position_value(value: &str) -> bool {
    matches!(value, "static" | "relative" | "absolute" | "fixed")
}

fn validate_direct_length_attr_with_unset(
    value: Option<&DomAttributeValue>,
) -> Result<(), DomError> {
    match value {
        Some(DomAttributeValue::String(value)) if is_supported_direct_length_attr_value(value) => {
            Ok(())
        }
        None => Ok(()),
        _ => Err(DomError::InvalidAttributeValue),
    }
}

fn validate_style_attribute_value(
    attribute: &DomAttribute,
    value: Option<&DomAttributeValue>,
) -> Result<(), DomError> {
    if is_layout_length_attribute(attribute) {
        return validate_style_length_attr_with_unset(value);
    }
    match attribute {
        DomAttribute::FontSize => validate_style_font_size_attribute(value),
        DomAttribute::LineHeight => validate_line_height_value(value),
        _ => validate_attribute_value(attribute, value),
    }
}

fn validate_style_length_attr_with_unset(
    value: Option<&DomAttributeValue>,
) -> Result<(), DomError> {
    match value {
        Some(DomAttributeValue::String(value)) if is_supported_style_length_attr_value(value) => {
            Ok(())
        }
        None => Ok(()),
        _ => Err(DomError::InvalidAttributeValue),
    }
}

fn validate_font_size_attribute(value: Option<&DomAttributeValue>) -> Result<(), DomError> {
    match value {
        Some(DomAttributeValue::String(value)) if is_supported_direct_font_size_value(value) => {
            Ok(())
        }
        None => Ok(()),
        _ => Err(DomError::InvalidAttributeValue),
    }
}

fn validate_style_font_size_attribute(value: Option<&DomAttributeValue>) -> Result<(), DomError> {
    match value {
        Some(DomAttributeValue::String(value)) if is_supported_style_font_size_value(value) => {
            Ok(())
        }
        None => Ok(()),
        _ => Err(DomError::InvalidAttributeValue),
    }
}

fn validate_line_height_value(value: Option<&DomAttributeValue>) -> Result<(), DomError> {
    match value {
        Some(DomAttributeValue::String(value)) if is_supported_line_height_value(value) => Ok(()),
        None => Ok(()),
        _ => Err(DomError::InvalidAttributeValue),
    }
}

fn validate_white_space_attribute(value: Option<&DomAttributeValue>) -> Result<(), DomError> {
    match value {
        Some(DomAttributeValue::String(value)) if parse_white_space_keyword(value).is_some() => {
            Ok(())
        }
        None => Ok(()),
        _ => Err(DomError::InvalidAttributeValue),
    }
}

fn is_supported_direct_length_attr_value(value: &str) -> bool {
    matches!(
        parse_authored_length(value),
        Some(AuthoredLength::Unset | AuthoredLength::Px(_) | AuthoredLength::Em(_))
    )
}

fn is_supported_style_length_attr_value(value: &str) -> bool {
    matches!(
        parse_authored_length(value),
        Some(AuthoredLength::Unset | AuthoredLength::Px(_) | AuthoredLength::Em(_))
    )
}

fn is_supported_direct_font_size_value(value: &str) -> bool {
    matches!(
        parse_authored_length(value),
        Some(AuthoredLength::Px(_) | AuthoredLength::Em(_))
    )
}

fn is_supported_style_font_size_value(value: &str) -> bool {
    matches!(
        parse_authored_length(value),
        Some(AuthoredLength::Px(_) | AuthoredLength::Em(_))
    )
}

fn is_supported_line_height_value(value: &str) -> bool {
    parse_line_height_value(value).is_some()
}

pub(crate) fn parse_authored_length(value: &str) -> Option<AuthoredLength> {
    if value == "unset" {
        return Some(AuthoredLength::Unset);
    }
    if value == "0" {
        return Some(AuthoredLength::Px(0.0));
    }
    let (number, unit) = split_length_number_and_unit(value)?;
    let scalar = parse_finite_length_scalar(number)?;
    match unit {
        "px" => Some(AuthoredLength::Px(scalar)),
        "em" => Some(AuthoredLength::Em(scalar)),
        _ => None,
    }
}

/// Parses the supported CSS color subset into normalized `[red, green, blue, alpha]` channels.
///
/// Supported inputs are `transparent`, six- or eight-digit hex colors, `rgb(...)`, `rgba(...)`,
/// and the named colors `white`, `black`, `red`, `green`, and `blue`.
pub fn parse_css_color_rgba(value: &str) -> Result<[f32; 4], DomError> {
    let trimmed = value.trim();
    if trimmed.eq_ignore_ascii_case("transparent") {
        return Ok([0.0, 0.0, 0.0, 0.0]);
    }
    if let Some(hex) = trimmed.strip_prefix('#') {
        return parse_hex_color_rgba(hex);
    }
    if let Some(args) = color_function_args(trimmed, "rgb") {
        return parse_rgb_function_args(args, false);
    }
    if let Some(args) = color_function_args(trimmed, "rgba") {
        return parse_rgb_function_args(args, true);
    }
    parse_named_color_rgba(trimmed).ok_or(DomError::InvalidAttributeValue)
}

fn parse_named_color_rgba(value: &str) -> Option<[f32; 4]> {
    if value.eq_ignore_ascii_case("white") {
        return Some([1.0, 1.0, 1.0, 1.0]);
    }
    if value.eq_ignore_ascii_case("black") {
        return Some([0.0, 0.0, 0.0, 1.0]);
    }
    if value.eq_ignore_ascii_case("red") {
        return Some([1.0, 0.0, 0.0, 1.0]);
    }
    if value.eq_ignore_ascii_case("green") {
        return Some([0.0, 1.0, 0.0, 1.0]);
    }
    if value.eq_ignore_ascii_case("blue") {
        return Some([0.0, 0.0, 1.0, 1.0]);
    }
    None
}

fn color_function_args<'a>(value: &'a str, function_name: &str) -> Option<&'a str> {
    let head = value.get(..function_name.len())?;
    if !head.eq_ignore_ascii_case(function_name) {
        return None;
    }
    let args = value.get(function_name.len()..)?.strip_prefix('(')?;
    args.strip_suffix(')')
}

fn parse_rgb_function_args(args: &str, alpha_required: bool) -> Result<[f32; 4], DomError> {
    let tokens = rgb_function_tokens(args);
    let required_len = if alpha_required { 4 } else { 3 };
    if tokens.len() != required_len {
        return Err(DomError::InvalidAttributeValue);
    }
    Ok([
        parse_rgb_channel(tokens[0])?,
        parse_rgb_channel(tokens[1])?,
        parse_rgb_channel(tokens[2])?,
        if alpha_required {
            parse_alpha_channel(tokens[3])?
        } else {
            1.0
        },
    ])
}

fn rgb_function_tokens(args: &str) -> Vec<&str> {
    if args.contains(',') {
        return args.split(',').map(str::trim).collect();
    }
    args.split_ascii_whitespace().collect()
}

fn parse_rgb_channel(value: &str) -> Result<f32, DomError> {
    if let Some(percent) = value.strip_suffix('%') {
        return parse_percent_channel(percent);
    }
    let parsed = value
        .parse::<f32>()
        .map_err(|_| DomError::InvalidAttributeValue)?;
    if parsed.is_finite() && (0.0..=255.0).contains(&parsed) {
        return Ok(parsed / 255.0);
    }
    Err(DomError::InvalidAttributeValue)
}

fn parse_alpha_channel(value: &str) -> Result<f32, DomError> {
    if let Some(percent) = value.strip_suffix('%') {
        return parse_percent_channel(percent);
    }
    let parsed = value
        .parse::<f32>()
        .map_err(|_| DomError::InvalidAttributeValue)?;
    if parsed.is_finite() && (0.0..=1.0).contains(&parsed) {
        return Ok(parsed);
    }
    Err(DomError::InvalidAttributeValue)
}

fn parse_percent_channel(percent: &str) -> Result<f32, DomError> {
    let parsed = percent
        .parse::<f32>()
        .map_err(|_| DomError::InvalidAttributeValue)?;
    if parsed.is_finite() && (0.0..=100.0).contains(&parsed) {
        return Ok(parsed / 100.0);
    }
    Err(DomError::InvalidAttributeValue)
}

fn parse_hex_color_rgba(hex: &str) -> Result<[f32; 4], DomError> {
    match hex.len() {
        6 => Ok([
            parse_hex_channel(&hex[0..2])?,
            parse_hex_channel(&hex[2..4])?,
            parse_hex_channel(&hex[4..6])?,
            1.0,
        ]),
        8 => Ok([
            parse_hex_channel(&hex[0..2])?,
            parse_hex_channel(&hex[2..4])?,
            parse_hex_channel(&hex[4..6])?,
            parse_hex_channel(&hex[6..8])?,
        ]),
        _ => Err(DomError::InvalidAttributeValue),
    }
}

fn parse_hex_channel(value: &str) -> Result<f32, DomError> {
    let parsed = u32::from_str_radix(value, 16).map_err(|_| DomError::InvalidAttributeValue)?;
    Ok(crate::numeric::u32_to_f32(parsed) / 255.0)
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum LineHeightValue {
    Normal,
    Unitless(f32),
    Px(f32),
    Em(f32),
}

pub(crate) fn resolve_line_height_px(value: &DomAttributeValue, font_size_px: f32) -> Option<f32> {
    let DomAttributeValue::String(value) = value else {
        return match value {
            DomAttributeValue::Float(value) => Some((*value).max(1.0)),
            DomAttributeValue::Integer(value) => Some(crate::numeric::i64_to_f32(*value).max(1.0)),
            DomAttributeValue::Bool(_) | DomAttributeValue::Color(_) => None,
            DomAttributeValue::String(_) => unreachable!("string handled above"),
        };
    };
    match parse_line_height_value(value)? {
        LineHeightValue::Normal => None,
        LineHeightValue::Unitless(scale) | LineHeightValue::Em(scale) => {
            Some((font_size_px * scale).max(1.0))
        }
        LineHeightValue::Px(value_px) => Some(value_px.max(1.0)),
    }
}

fn parse_line_height_value(value: &str) -> Option<LineHeightValue> {
    if value == "normal" {
        return Some(LineHeightValue::Normal);
    }

    if let Some(number) = parse_non_negative_line_height_number(value) {
        return Some(LineHeightValue::Unitless(number));
    }

    match parse_line_height_length(value) {
        Some(AuthoredLength::Px(px)) => return Some(LineHeightValue::Px(px)),
        Some(AuthoredLength::Em(em)) => return Some(LineHeightValue::Em(em)),
        Some(AuthoredLength::Unset) | None => {}
    }
    None
}

fn parse_line_height_length(value: &str) -> Option<AuthoredLength> {
    match parse_authored_length(value)? {
        AuthoredLength::Px(px) if px >= 0.0 => Some(AuthoredLength::Px(px)),
        AuthoredLength::Em(em) if em >= 0.0 => Some(AuthoredLength::Em(em)),
        _ => None,
    }
}

fn parse_non_negative_line_height_number(value: &str) -> Option<f32> {
    let parsed = value.parse::<f32>().ok()?;
    if parsed.is_finite() && parsed >= 0.0 {
        return Some(parsed);
    }
    None
}

fn split_length_number_and_unit(value: &str) -> Option<(&str, &str)> {
    if let Some(number) = value.strip_suffix("px") {
        return Some((number, "px"));
    }
    if let Some(number) = value.strip_suffix("em") {
        return Some((number, "em"));
    }
    None
}

fn parse_finite_length_scalar(number: &str) -> Option<f32> {
    if number.is_empty() {
        return None;
    }
    number.parse::<f32>().ok().filter(|value| value.is_finite())
}

pub(crate) fn style_attr_allows_property(attribute: &DomAttribute) -> bool {
    match attribute {
        DomAttribute::Id
        | DomAttribute::Style
        | DomAttribute::Class
        | DomAttribute::Transition
        | DomAttribute::ReadOnly
        | DomAttribute::Disabled
        | DomAttribute::Required
        | DomAttribute::Checked
        | DomAttribute::TabIndex
        | DomAttribute::Src
        | DomAttribute::Max
        | DomAttribute::MaxLength
        | DomAttribute::Min
        | DomAttribute::MinLength
        | DomAttribute::Placeholder
        | DomAttribute::Step
        | DomAttribute::Type
        | DomAttribute::Value
        | DomAttribute::Lang
        | DomAttribute::Dir => false,
        DomAttribute::Custom(name) => style_attr_allows_custom_property(name),
        _ => true,
    }
}

fn style_attr_allows_custom_property(name: &str) -> bool {
    matches!(
        name,
        "background-color"
            | "color"
            | "border-color"
            | "border-top-color"
            | "border-right-color"
            | "border-bottom-color"
            | "border-left-color"
            | "outline-color"
            | "outline-width"
            | "outline-offset"
            | "caret-color"
            | "border-radius"
            | "opacity"
    )
}

fn validate_opacity_attribute(value: Option<&DomAttributeValue>) -> Result<(), DomError> {
    match value {
        None => Ok(()),
        Some(DomAttributeValue::Float(value)) if is_supported_opacity_number(*value) => Ok(()),
        Some(DomAttributeValue::String(value)) => value
            .parse::<f32>()
            .ok()
            .filter(|value| is_supported_opacity_number(*value))
            .map(|_| ())
            .ok_or(DomError::InvalidAttributeValue),
        _ => Err(DomError::InvalidAttributeValue),
    }
}

fn is_supported_opacity_number(value: f32) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

fn is_layout_length_attribute(attribute: &DomAttribute) -> bool {
    matches!(
        attribute,
        DomAttribute::Width
            | DomAttribute::Height
            | DomAttribute::MinWidth
            | DomAttribute::MinHeight
            | DomAttribute::MaxWidth
            | DomAttribute::MaxHeight
            | DomAttribute::Top
            | DomAttribute::Left
            | DomAttribute::Right
            | DomAttribute::Bottom
            | DomAttribute::MarginTop
            | DomAttribute::MarginLeft
            | DomAttribute::MarginRight
            | DomAttribute::MarginBottom
            | DomAttribute::PaddingTop
            | DomAttribute::PaddingLeft
            | DomAttribute::PaddingRight
            | DomAttribute::PaddingBottom
            | DomAttribute::BorderTop
            | DomAttribute::BorderLeft
            | DomAttribute::BorderRight
            | DomAttribute::BorderBottom
    )
}

#[cfg(test)]
mod tests {
    use crate::types::{DomAttribute, DomAttributeValue, DomError};

    use super::{
        layout_length_attr_uses_unset, parse_authored_length, parse_class_attr_list,
        parse_css_color_rgba, parse_style_attr_map, resolve_line_height_px,
        validate_attribute_value,
    };

    fn color(value: &str) -> DomAttributeValue {
        DomAttributeValue::Color(parse_css_color_rgba(value).expect("test color should parse"))
    }

    #[test]
    fn class_parser_preserves_order_and_duplicates() {
        assert_eq!(
            parse_class_attr_list(" hero   primary hero "),
            vec![
                "hero".to_string(),
                "primary".to_string(),
                "hero".to_string()
            ]
        );
    }

    #[test]
    fn style_parser_uses_last_declaration_wins() {
        let parsed = parse_style_attr_map("width: 12px; background-color: #112233; width: 14px")
            .expect("style should parse");

        assert_eq!(
            parsed.get(&DomAttribute::Width),
            Some(&DomAttributeValue::String("14px".to_string()))
        );
        assert_eq!(
            parsed.get(&DomAttribute::Custom("background-color".to_string())),
            Some(&color("#112233"))
        );
    }

    #[test]
    fn css_color_parser_accepts_supported_color_subset() {
        assert_eq!(
            parse_css_color_rgba("transparent"),
            Ok([0.0, 0.0, 0.0, 0.0])
        );
        assert_eq!(
            parse_css_color_rgba("#11223344"),
            Ok([17.0 / 255.0, 34.0 / 255.0, 51.0 / 255.0, 68.0 / 255.0])
        );
        assert_eq!(parse_css_color_rgba("BLUE"), Ok([0.0, 0.0, 1.0, 1.0]));
        assert_eq!(
            parse_css_color_rgba("RGB(255, 128, 0)"),
            Ok([1.0, 128.0 / 255.0, 0.0, 1.0])
        );
        assert_eq!(
            parse_css_color_rgba("rgba(100%, 50%, 0%, 25%)"),
            Ok([1.0, 0.5, 0.0, 0.25])
        );
    }

    #[test]
    fn css_color_parser_rejects_unsupported_or_out_of_range_values() {
        for value in [
            "",
            "#12345",
            "#gg0000",
            "rgb(256, 0, 0)",
            "rgb(0, 0)",
            "rgba(0, 0, 0, 1.5)",
            "rgba(0, 0, 0)",
            "lab(1 2 3)",
        ] {
            assert_eq!(
                parse_css_color_rgba(value),
                Err(DomError::InvalidAttributeValue),
                "{value} should fail",
            );
        }
    }

    #[test]
    fn validator_accepts_only_normalized_color_values_for_known_color_attributes() {
        validate_attribute_value(
            &DomAttribute::Custom("color".to_string()),
            Some(&color("#336699")),
        )
        .expect("normalized color should validate");

        let error = validate_attribute_value(
            &DomAttribute::Custom("color".to_string()),
            Some(&DomAttributeValue::String("#336699".to_string())),
        )
        .expect_err("raw color string should fail validation after normalization boundary");
        assert_eq!(error, DomError::InvalidAttributeValue);

        let error = validate_attribute_value(
            &DomAttribute::Custom("color".to_string()),
            Some(&DomAttributeValue::Color([1.0, 0.0, f32::NAN, 1.0])),
        )
        .expect_err("non-finite color should fail validation");
        assert_eq!(error, DomError::InvalidAttributeValue);
    }

    #[test]
    fn style_parser_accepts_per_side_border_colors() {
        let parsed = parse_style_attr_map(
            "border-top-color: #ff0000; border-right-color: #00ff00; border-bottom-color: #0000ff; border-left-color: #ffff00",
        )
        .expect("style should parse");

        assert_eq!(
            parsed.get(&DomAttribute::Custom("border-top-color".to_string())),
            Some(&color("#ff0000"))
        );
        assert_eq!(
            parsed.get(&DomAttribute::Custom("border-right-color".to_string())),
            Some(&color("#00ff00"))
        );
        assert_eq!(
            parsed.get(&DomAttribute::Custom("border-bottom-color".to_string())),
            Some(&color("#0000ff"))
        );
        assert_eq!(
            parsed.get(&DomAttribute::Custom("border-left-color".to_string())),
            Some(&color("#ffff00"))
        );
    }

    #[test]
    fn style_parser_accepts_focus_chrome_properties() {
        let parsed = parse_style_attr_map(
            "outline-color: #7aa2ff; outline-width: 2px; outline-offset: 3px; caret-color: #ffffff",
        )
        .expect("style should parse");

        assert_eq!(
            parsed.get(&DomAttribute::Custom("outline-color".to_string())),
            Some(&color("#7aa2ff"))
        );
        assert_eq!(
            parsed.get(&DomAttribute::Custom("outline-width".to_string())),
            Some(&DomAttributeValue::String("2px".to_string()))
        );
        assert_eq!(
            parsed.get(&DomAttribute::Custom("outline-offset".to_string())),
            Some(&DomAttributeValue::String("3px".to_string()))
        );
        assert_eq!(
            parsed.get(&DomAttribute::Custom("caret-color".to_string())),
            Some(&color("#ffffff"))
        );
    }

    #[test]
    fn style_parser_accepts_transition_longhand_properties() {
        let parsed = parse_style_attr_map(
            "transition-property: all, opacity; transition-duration: 80ms, 180ms; transition-timing-function: ease, linear; transition-delay: 0ms, -20ms",
        )
        .expect("transition longhand style should parse");

        assert_eq!(
            parsed.get(&DomAttribute::TransitionProperty),
            Some(&DomAttributeValue::String("all, opacity".to_string()))
        );
        assert_eq!(
            parsed.get(&DomAttribute::TransitionDuration),
            Some(&DomAttributeValue::String("80ms, 180ms".to_string()))
        );
        assert_eq!(
            parsed.get(&DomAttribute::TransitionTimingFunction),
            Some(&DomAttributeValue::String("ease, linear".to_string()))
        );
        assert_eq!(
            parsed.get(&DomAttribute::TransitionDelay),
            Some(&DomAttributeValue::String("0ms, -20ms".to_string()))
        );
    }

    #[test]
    fn resolve_line_height_px_supports_all_valid_forms() {
        assert_eq!(
            resolve_line_height_px(&DomAttributeValue::String("normal".to_string()), 14.0),
            None
        );
        assert_eq!(
            resolve_line_height_px(&DomAttributeValue::String("1.5".to_string()), 14.0),
            Some(21.0)
        );
        assert_eq!(
            resolve_line_height_px(&DomAttributeValue::String("20px".to_string()), 14.0),
            Some(20.0)
        );
        assert_eq!(
            resolve_line_height_px(&DomAttributeValue::String("1.25em".to_string()), 16.0),
            Some(20.0)
        );
    }

    #[test]
    fn style_parser_rejects_non_style_properties() {
        let error = parse_style_attr_map("slot: panel").expect_err("slot should fail");
        assert_eq!(error, DomError::InvalidAttributeValue);
        let error = parse_style_attr_map("tab-index: 0").expect_err("tab-index should fail");
        assert_eq!(error, DomError::InvalidAttributeValue);
    }

    #[test]
    fn style_parser_accepts_font_size_property_in_step_2() {
        let parsed =
            parse_style_attr_map("font-size: 14px; color: #ffffff").expect("style should parse");

        assert_eq!(
            parsed.get(&DomAttribute::FontSize),
            Some(&DomAttributeValue::String("14px".to_string()))
        );
        assert_eq!(
            parsed.get(&DomAttribute::Custom("color".to_string())),
            Some(&color("#ffffff"))
        );
    }

    #[test]
    fn style_parser_accepts_new_text_style_properties() {
        let parsed = parse_style_attr_map(
            "font-family: ui-sans, sans-serif; font-weight: 600; font-style: italic; line-height: 1.4; text-align: center; white-space: pre-wrap; overflow-wrap: anywhere; word-break: break-word",
        )
        .expect("text style properties should parse");

        let expected = [
            (DomAttribute::FontFamily, "ui-sans, sans-serif"),
            (DomAttribute::FontWeight, "600"),
            (DomAttribute::FontStyle, "italic"),
            (DomAttribute::LineHeight, "1.4"),
            (DomAttribute::TextAlign, "center"),
            (DomAttribute::WhiteSpace, "pre-wrap"),
            (DomAttribute::OverflowWrap, "anywhere"),
            (DomAttribute::WordBreak, "break-word"),
        ];
        for (attribute, value) in expected {
            assert_eq!(
                parsed.get(&attribute),
                Some(&DomAttributeValue::String(value.to_string()))
            );
        }
    }

    #[test]
    fn style_parser_accepts_flex_surface_properties() {
        let parsed = parse_style_attr_map(
            "flex-direction: row; flex-wrap: wrap; flex-flow: column wrap; flex-grow: 2; flex-shrink: 3; flex-basis: 20px; flex: 1 0 auto; order: 4; justify-content: space-between; align-items: stretch; align-self: center; align-content: space-around; gap: 12px; row-gap: 8px; column-gap: 10px; place-content: center start; place-items: stretch center; place-self: end auto",
        )
        .expect("flex surface properties should parse");

        let expected = [
            (DomAttribute::FlexDirection, "row"),
            (DomAttribute::FlexWrap, "wrap"),
            (DomAttribute::FlexFlow, "column wrap"),
            (DomAttribute::FlexGrow, "2"),
            (DomAttribute::FlexShrink, "3"),
            (DomAttribute::FlexBasis, "20px"),
            (DomAttribute::Flex, "1 0 auto"),
            (DomAttribute::Order, "4"),
            (DomAttribute::JustifyContent, "space-between"),
            (DomAttribute::AlignItems, "stretch"),
            (DomAttribute::AlignSelf, "center"),
            (DomAttribute::AlignContent, "space-around"),
            (DomAttribute::Gap, "12px"),
            (DomAttribute::RowGap, "8px"),
            (DomAttribute::ColumnGap, "10px"),
            (DomAttribute::PlaceContent, "center start"),
            (DomAttribute::PlaceItems, "stretch center"),
            (DomAttribute::PlaceSelf, "end auto"),
        ];
        for (attribute, value) in expected {
            assert_eq!(
                parsed.get(&attribute),
                Some(&DomAttributeValue::String(value.to_string()))
            );
        }
    }

    #[test]
    fn validator_accepts_string_style_and_class_values() {
        validate_attribute_value(
            &DomAttribute::Style,
            Some(&DomAttributeValue::String("width: 12px".to_string())),
        )
        .expect("style string should validate");
        validate_attribute_value(
            &DomAttribute::Class,
            Some(&DomAttributeValue::String("hero".to_string())),
        )
        .expect("class string should validate");
    }

    #[test]
    fn validator_accepts_integer_tab_index_values() {
        validate_attribute_value(
            &DomAttribute::TabIndex,
            Some(&DomAttributeValue::Integer(0)),
        )
        .expect("integer tab-index should validate");

        let error = validate_attribute_value(
            &DomAttribute::TabIndex,
            Some(&DomAttributeValue::String("0".to_string())),
        )
        .expect_err("string tab-index should fail");

        assert_eq!(error, DomError::InvalidAttributeValue);
    }

    #[test]
    fn validator_accepts_new_form_attribute_values() {
        validate_attribute_value(
            &DomAttribute::ReadOnly,
            Some(&DomAttributeValue::Bool(true)),
        )
        .expect("readonly bool should validate");
        validate_attribute_value(
            &DomAttribute::Disabled,
            Some(&DomAttributeValue::String("true".to_string())),
        )
        .expect("disabled string should validate");
        validate_attribute_value(
            &DomAttribute::MaxLength,
            Some(&DomAttributeValue::String("64".to_string())),
        )
        .expect("maxlength string should validate");
        validate_attribute_value(
            &DomAttribute::Placeholder,
            Some(&DomAttributeValue::String("email".to_string())),
        )
        .expect("placeholder string should validate");
        validate_attribute_value(
            &DomAttribute::FontFamily,
            Some(&DomAttributeValue::String(
                "ui-sans, sans-serif".to_string(),
            )),
        )
        .expect("font-family should validate");
        validate_attribute_value(
            &DomAttribute::FontWeight,
            Some(&DomAttributeValue::String("600".to_string())),
        )
        .expect("font-weight should validate");
        validate_attribute_value(
            &DomAttribute::FontStyle,
            Some(&DomAttributeValue::String("italic".to_string())),
        )
        .expect("font-style should validate");
        validate_attribute_value(
            &DomAttribute::TextAlign,
            Some(&DomAttributeValue::String("center".to_string())),
        )
        .expect("text-align should validate");
        validate_attribute_value(
            &DomAttribute::WhiteSpace,
            Some(&DomAttributeValue::String("pre-wrap".to_string())),
        )
        .expect("white-space should validate");
        validate_attribute_value(
            &DomAttribute::OverflowWrap,
            Some(&DomAttributeValue::String("anywhere".to_string())),
        )
        .expect("overflow-wrap should validate");
        validate_attribute_value(
            &DomAttribute::WordBreak,
            Some(&DomAttributeValue::String("break-word".to_string())),
        )
        .expect("word-break should validate");
        validate_attribute_value(
            &DomAttribute::Lang,
            Some(&DomAttributeValue::String("ja".to_string())),
        )
        .expect("lang should validate");
        validate_attribute_value(
            &DomAttribute::Dir,
            Some(&DomAttributeValue::String("rtl".to_string())),
        )
        .expect("dir should validate");
        validate_attribute_value(
            &DomAttribute::Direction,
            Some(&DomAttributeValue::String("rtl".to_string())),
        )
        .expect("direction should validate");
    }

    #[test]
    fn validator_accepts_flex_surface_string_values() {
        let expected = [
            (DomAttribute::FlexDirection, "row"),
            (DomAttribute::FlexWrap, "wrap"),
            (DomAttribute::FlexFlow, "column wrap"),
            (DomAttribute::FlexGrow, "2"),
            (DomAttribute::FlexShrink, "3"),
            (DomAttribute::FlexBasis, "20px"),
            (DomAttribute::Flex, "1 0 auto"),
            (DomAttribute::Order, "4"),
            (DomAttribute::JustifyContent, "space-between"),
            (DomAttribute::AlignItems, "stretch"),
            (DomAttribute::AlignSelf, "center"),
            (DomAttribute::AlignContent, "space-around"),
            (DomAttribute::Gap, "12px"),
            (DomAttribute::RowGap, "8px"),
            (DomAttribute::ColumnGap, "10px"),
            (DomAttribute::PlaceContent, "center start"),
            (DomAttribute::PlaceItems, "stretch center"),
            (DomAttribute::PlaceSelf, "end auto"),
        ];

        for (attribute, value) in expected {
            validate_attribute_value(
                &attribute,
                Some(&DomAttributeValue::String(value.to_string())),
            )
            .expect("flex surface string should validate");
        }
    }

    #[test]
    fn style_parser_rejects_new_non_style_form_properties() {
        let error = parse_style_attr_map("readonly: true").expect_err("readonly should fail");
        assert_eq!(error, DomError::InvalidAttributeValue);

        let error = parse_style_attr_map("placeholder: name").expect_err("placeholder should fail");
        assert_eq!(error, DomError::InvalidAttributeValue);

        let error = parse_style_attr_map("lang: ja").expect_err("lang should fail");
        assert_eq!(error, DomError::InvalidAttributeValue);

        let error = parse_style_attr_map("dir: rtl").expect_err("dir should fail");
        assert_eq!(error, DomError::InvalidAttributeValue);

        parse_style_attr_map("direction: rtl").expect("direction should parse");
    }

    #[test]
    fn style_parser_maps_direction_property_to_direction_attribute() {
        let map = parse_style_attr_map("direction: rtl").expect("direction should parse");

        assert_eq!(
            map.get(&DomAttribute::Direction),
            Some(&DomAttributeValue::String("rtl".to_string()))
        );
    }

    #[test]
    fn validator_accepts_direct_em_and_px_font_size_values() {
        validate_attribute_value(
            &DomAttribute::FontSize,
            Some(&DomAttributeValue::String("0".to_string())),
        )
        .expect("zero font-size should validate");
        validate_attribute_value(
            &DomAttribute::FontSize,
            Some(&DomAttributeValue::String("14px".to_string())),
        )
        .expect("pixel font-size should validate");
        validate_attribute_value(
            &DomAttribute::FontSize,
            Some(&DomAttributeValue::String("1.5em".to_string())),
        )
        .expect("em font-size should validate");
        validate_attribute_value(&DomAttribute::FontSize, None)
            .expect("missing font-size should validate");
    }

    #[test]
    fn validator_rejects_invalid_font_size_values() {
        for invalid in ["14", "auto", "unset", "NaNem", "infpx"] {
            let error = validate_attribute_value(
                &DomAttribute::FontSize,
                Some(&DomAttributeValue::String(invalid.to_string())),
            )
            .expect_err("non-pixel font-size should fail");
            assert_eq!(error, DomError::InvalidAttributeValue);
        }
    }

    #[test]
    fn validator_accepts_supported_line_height_values() {
        for value in ["normal", "0", "1.4", "18px", "1.2em"] {
            validate_attribute_value(
                &DomAttribute::LineHeight,
                Some(&DomAttributeValue::String(value.to_string())),
            )
            .expect("supported line-height should validate");
        }
        validate_attribute_value(&DomAttribute::LineHeight, None)
            .expect("missing line-height should validate");
    }

    #[test]
    fn validator_accepts_only_supported_text_align_values() {
        for value in ["left", "start", "center", "right", "justify", "end"] {
            validate_attribute_value(
                &DomAttribute::TextAlign,
                Some(&DomAttributeValue::String(value.to_string())),
            )
            .expect("supported text-align should validate");
        }
        validate_attribute_value(&DomAttribute::TextAlign, None)
            .expect("missing text-align should validate");

        for invalid in ["", "middle", "top", "stretch"] {
            let error = validate_attribute_value(
                &DomAttribute::TextAlign,
                Some(&DomAttributeValue::String(invalid.to_string())),
            )
            .expect_err("unsupported text-align should fail");
            assert_eq!(error, DomError::InvalidAttributeValue);
        }
    }

    #[test]
    fn validator_accepts_only_supported_white_space_values() {
        for value in [
            "normal",
            "nowrap",
            "pre",
            "pre-wrap",
            "pre-line",
            "break-spaces",
        ] {
            validate_attribute_value(
                &DomAttribute::WhiteSpace,
                Some(&DomAttributeValue::String(value.to_string())),
            )
            .expect("supported white-space should validate");
        }
        validate_attribute_value(&DomAttribute::WhiteSpace, None)
            .expect("missing white-space should validate");

        for invalid in ["", "wrap", "preserve nowrap", "break-all"] {
            let error = validate_attribute_value(
                &DomAttribute::WhiteSpace,
                Some(&DomAttributeValue::String(invalid.to_string())),
            )
            .expect_err("unsupported white-space should fail");
            assert_eq!(error, DomError::InvalidAttributeValue);
        }
    }

    #[test]
    fn validator_rejects_unsupported_line_height_values() {
        for invalid in [
            "120%", "unset", "auto", "-1", "-2px", "-0.5em", "NaN", "inf",
        ] {
            let error = validate_attribute_value(
                &DomAttribute::LineHeight,
                Some(&DomAttributeValue::String(invalid.to_string())),
            )
            .expect_err("unsupported line-height should fail");
            assert_eq!(error, DomError::InvalidAttributeValue);
        }
    }

    #[test]
    fn validator_accepts_direct_em_and_px_length_values() {
        validate_attribute_value(
            &DomAttribute::Width,
            Some(&DomAttributeValue::String("unset".to_string())),
        )
        .expect("unset width should validate");
        validate_attribute_value(
            &DomAttribute::Width,
            Some(&DomAttributeValue::String("2em".to_string())),
        )
        .expect("em width should validate");
    }

    #[test]
    fn style_parser_accepts_em_lengths_in_step_4() {
        let parsed = parse_style_attr_map("width: 2em; font-size: 1.5em").expect("stylesheet em");

        assert_eq!(
            parsed.get(&DomAttribute::Width),
            Some(&DomAttributeValue::String("2em".to_string()))
        );
        assert_eq!(
            parsed.get(&DomAttribute::FontSize),
            Some(&DomAttributeValue::String("1.5em".to_string()))
        );
    }

    #[test]
    fn style_parser_accepts_supported_line_height_values() {
        let parsed = parse_style_attr_map("line-height: normal").expect("normal should parse");
        assert_eq!(
            parsed.get(&DomAttribute::LineHeight),
            Some(&DomAttributeValue::String("normal".to_string()))
        );

        let parsed = parse_style_attr_map("line-height: 1.25").expect("unitless should parse");
        assert_eq!(
            parsed.get(&DomAttribute::LineHeight),
            Some(&DomAttributeValue::String("1.25".to_string()))
        );

        let parsed = parse_style_attr_map("line-height: 20px").expect("px should parse");
        assert_eq!(
            parsed.get(&DomAttribute::LineHeight),
            Some(&DomAttributeValue::String("20px".to_string()))
        );

        let parsed = parse_style_attr_map("line-height: 1.4em").expect("em should parse");
        assert_eq!(
            parsed.get(&DomAttribute::LineHeight),
            Some(&DomAttributeValue::String("1.4em".to_string()))
        );
    }

    #[test]
    fn style_parser_rejects_unsupported_line_height_values() {
        for invalid in ["120%", "unset", "auto", "-1", "-3px", "-0.5em"] {
            let error = parse_style_attr_map(&format!("line-height: {invalid}"))
                .expect_err("unsupported line-height should fail");
            assert_eq!(error, DomError::InvalidAttributeValue);
        }
    }

    #[test]
    fn style_parser_rejects_lang_and_dir_even_next_to_valid_line_height() {
        let error = parse_style_attr_map("line-height: 1.2; lang: ja")
            .expect_err("lang should still fail next to valid line-height");
        assert_eq!(error, DomError::InvalidAttributeValue);

        let error = parse_style_attr_map("line-height: 1.2; dir: rtl")
            .expect_err("dir should still fail next to valid line-height");
        assert_eq!(error, DomError::InvalidAttributeValue);

        parse_style_attr_map("line-height: 1.2; direction: rtl")
            .expect("direction should still parse next to valid line-height");
    }

    #[test]
    fn authored_length_parser_recognizes_supported_units() {
        assert_eq!(
            parse_authored_length("0"),
            Some(super::AuthoredLength::Px(0.0))
        );
        assert_eq!(
            parse_authored_length("12px"),
            Some(super::AuthoredLength::Px(12.0))
        );
        assert_eq!(
            parse_authored_length("1.5em"),
            Some(super::AuthoredLength::Em(1.5))
        );
        assert_eq!(
            parse_authored_length("unset"),
            Some(super::AuthoredLength::Unset)
        );
    }

    #[test]
    fn layout_length_attrs_still_use_unset_removal() {
        assert!(layout_length_attr_uses_unset(&DomAttribute::Width));
        assert!(!layout_length_attr_uses_unset(&DomAttribute::FontSize));
        assert!(!layout_length_attr_uses_unset(&DomAttribute::LineHeight));
    }
}
