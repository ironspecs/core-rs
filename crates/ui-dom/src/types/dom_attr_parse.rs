//! Authored content-attribute parsing for `ui-dom`.
//!
//! This file owns the boundary between authored attribute text and retained
//! attribute values for CONTENT attributes only (PLAN.md approach C, the
//! vocabulary split completing in spine 3.4): per-attribute validation
//! (`validate_attribute_value`), the supported length model (`AuthoredLength`,
//! `parse_authored_length`), the supported color subset (`parse_css_color_rgba`),
//! and the class-list parser (`parse_class_attr_list`). Presentation
//! declarations are parsed by the declaration store (`decl_parse`), not here;
//! their old validation/normalization arms are gone with the presentation
//! variants.
//!
//! Invariants and rules:
//!
//! - all grammars here are closed subsets: lengths are exactly `px`, `em`,
//!   `unset`, or bare `0`; colors are `transparent`, 6/8-digit hex,
//!   `rgb()`/`rgba()`, or the five named colors. Anything outside the subset is
//!   a typed `DomError`, never a panic: authored text is boundary data
//! - content attributes validate strictly; a non-content variant (the `Style`
//!   seam) reaching here is rejected
//! - the authored `line-height` resolution helpers are now `#[cfg(test)]`:
//!   the element intrinsic-measurement path (`intrinsic_size.rs`) reads the
//!   resolved line height from the style pass (spine 3.3g-2a), not the
//!   authored attr

use crate::types::{DomAttribute, DomAttributeValue, DomError, parse_text_direction_keyword};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum AuthoredLength {
    Unset,
    Px(f32),
    Em(f32),
}

/// Validates one content attribute value. The `Style` seam has no content
/// grammar (it routes into the inline declaration store before this runs), so
/// it is rejected here with the strict `InvalidAttributeValue` arm.
pub(crate) fn validate_attribute_value(
    attribute: &DomAttribute,
    value: Option<&DomAttributeValue>,
) -> Result<(), DomError> {
    match attribute {
        DomAttribute::Id
        | DomAttribute::Class
        | DomAttribute::Lang
        | DomAttribute::List
        | DomAttribute::Label
        | DomAttribute::Max
        | DomAttribute::MaxLength
        | DomAttribute::Min
        | DomAttribute::MinLength
        | DomAttribute::Placeholder
        | DomAttribute::Step
        | DomAttribute::Type
        | DomAttribute::Value
        | DomAttribute::Src
        | DomAttribute::PresentedOptionsRole
        | DomAttribute::PresentedOptionsState => validate_string_attribute(value),
        DomAttribute::Dir => validate_dir_attribute(value),
        DomAttribute::ReadOnly
        | DomAttribute::Disabled
        | DomAttribute::Required
        | DomAttribute::Checked
        | DomAttribute::Selected
        | DomAttribute::PresentedOptionsHighlighted
        | DomAttribute::Multiple => validate_boolean_attribute(value),
        DomAttribute::TabIndex | DomAttribute::Size | DomAttribute::PresentedOptionsSourceIndex => {
            validate_integer_attribute(value)
        }
        DomAttribute::Custom(_) => Ok(()),
        DomAttribute::Style => Err(DomError::InvalidAttributeValue),
    }
}

pub(crate) fn parse_class_attr_list(value: &str) -> Vec<String> {
    value
        .split_ascii_whitespace()
        .map(ToOwned::to_owned)
        .collect()
}

fn validate_dir_attribute(value: Option<&DomAttributeValue>) -> Result<(), DomError> {
    match value {
        None => Ok(()),
        Some(DomAttributeValue::String(value)) if parse_text_direction_keyword(value).is_some() => {
            Ok(())
        }
        _ => Err(DomError::InvalidAttributeValue),
    }
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
#[cfg(test)]
enum LineHeightValue {
    Normal,
    Unitless(f32),
    Px(f32),
    Em(f32),
}

#[cfg(test)]
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

#[cfg(test)]
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

#[cfg(test)]
fn parse_line_height_length(value: &str) -> Option<AuthoredLength> {
    match parse_authored_length(value)? {
        AuthoredLength::Px(px) if px >= 0.0 => Some(AuthoredLength::Px(px)),
        AuthoredLength::Em(em) if em >= 0.0 => Some(AuthoredLength::Em(em)),
        _ => None,
    }
}

#[cfg(test)]
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn authored_length_parser_recognizes_supported_units() {
        assert_eq!(parse_authored_length("0"), Some(AuthoredLength::Px(0.0)));
        assert_eq!(
            parse_authored_length("12px"),
            Some(AuthoredLength::Px(12.0))
        );
        assert_eq!(
            parse_authored_length("1.5em"),
            Some(AuthoredLength::Em(1.5))
        );
        assert_eq!(parse_authored_length("unset"), Some(AuthoredLength::Unset));
    }

    #[test]
    fn validator_accepts_content_string_values() {
        for attribute in [
            DomAttribute::Class,
            DomAttribute::Lang,
            DomAttribute::Src,
            DomAttribute::Label,
            DomAttribute::Placeholder,
            DomAttribute::Type,
            DomAttribute::Value,
        ] {
            validate_attribute_value(
                &attribute,
                Some(&DomAttributeValue::String("text".to_string())),
            )
            .expect("content string should validate");
        }
        validate_attribute_value(
            &DomAttribute::Dir,
            Some(&DomAttributeValue::String("rtl".into())),
        )
        .expect("dir should validate");
    }

    #[test]
    fn validator_accepts_boolean_and_integer_content_values() {
        for attribute in [
            DomAttribute::ReadOnly,
            DomAttribute::Disabled,
            DomAttribute::Required,
            DomAttribute::Checked,
            DomAttribute::Selected,
            DomAttribute::Multiple,
        ] {
            validate_attribute_value(&attribute, Some(&DomAttributeValue::Bool(true)))
                .expect("boolean should validate");
        }
        validate_attribute_value(
            &DomAttribute::TabIndex,
            Some(&DomAttributeValue::Integer(0)),
        )
        .expect("integer tab-index should validate");
    }

    #[test]
    fn validator_rejects_non_content_variants() {
        // The `Style` seam has no content grammar: it routes into the inline
        // declaration store before validation, so it must never validate here.
        let error = validate_attribute_value(
            &DomAttribute::Style,
            Some(&DomAttributeValue::String("width: 10px".to_string())),
        )
        .expect_err("the style seam should not validate through the content table");
        assert_eq!(error, DomError::InvalidAttributeValue);
    }
}
