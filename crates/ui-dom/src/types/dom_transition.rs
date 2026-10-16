//! Shared transition state for authored DOM properties.
//!
//! This file represents the retained transition objects and scheduler-facing
//! transition stores used by `ui-dom`. Its purpose is to keep time-based
//! presented values explicit and reusable without pushing animation state into
//! app crates.
//!
//! Invariants and rules:
//!
//! - transition data here is crate-owned and deterministic
//! - these types describe interpolation state, not higher-level animation policy
//! - callers should not need to know internal scheduler details to use them
//!
//! Advice and code philosophy:
//!
//! - keep timing and interpolation shapes honest and small
//! - prefer explicit state over hidden animation side tables
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
};

use num_traits::ToPrimitive;

use crate::types::dom_attr_parse::style_attr_allows_property;
use crate::types::dom_node::DomNodeSlotKey;
use crate::types::{DomAttribute, DomAttributeValue};

#[cfg(test)]
use crate::types::DomError;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DomTransitionState {
    pub active: BTreeMap<DomAttribute, DomTransition>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DomTransitionStore {
    pub(crate) active_nodes: BTreeSet<DomNodeSlotKey>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum DomTimingFunction {
    #[default]
    Linear,
    Ease,
    EaseIn,
    EaseOut,
    EaseInOut,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DomTransition {
    pub node_key: DomNodeSlotKey,
    pub attribute: DomAttribute,
    pub from: DomAttributeValue,
    pub to: DomAttributeValue,
    pub presented: DomAttributeValue,
    pub started_at_s: f64,
    pub duration_s: f32,
    pub delay_s: f32,
    pub timing: DomTimingFunction,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DomTransitionConfig {
    pub node_key: DomNodeSlotKey,
    pub attribute: DomAttribute,
    pub from: DomAttributeValue,
    pub to: DomAttributeValue,
    pub started_at_s: f64,
    pub duration_s: f32,
    pub delay_s: f32,
    pub timing: DomTimingFunction,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DomTransitionSpec {
    pub duration_s: f32,
    pub delay_s: f32,
    pub timing: DomTimingFunction,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct TransitionLonghandValues {
    pub property: Option<DomAttributeValue>,
    pub duration: Option<DomAttributeValue>,
    pub timing_function: Option<DomAttributeValue>,
    pub delay: Option<DomAttributeValue>,
}

impl DomTransition {
    /// Creates one retained transition for a single node attribute.
    ///
    /// This captures the authored start/end values and timing parameters needed
    /// for root-owned transition scheduling. Callers use it to describe one
    /// animating property, not a whole animation system.
    #[must_use]
    pub fn new(config: DomTransitionConfig) -> Self {
        let DomTransitionConfig {
            node_key,
            attribute,
            from,
            to,
            started_at_s,
            duration_s,
            delay_s,
            timing,
        } = config;
        Self {
            node_key,
            attribute,
            presented: from.clone(),
            from,
            to,
            started_at_s,
            duration_s,
            delay_s,
            timing,
        }
    }

    /// Advances the transition to the provided timestamp.
    ///
    /// This mutates only the transition's presented value and internal timing
    /// state for the current moment. It does not decide whether the owning root
    /// should keep scheduling the node afterward.
    pub fn sync(&mut self, now_s: f64) -> bool {
        let previous_presented = self.presented.clone();
        if now_s <= self.effective_start_s() {
            self.presented = self.from.clone();
            return self.presented != previous_presented;
        }
        if self.is_finished(now_s) {
            self.presented = self.to.clone();
            return self.presented != previous_presented;
        }
        self.presented = interpolate_value(&self.from, &self.to, self.eased_progress(now_s));
        self.presented != previous_presented
    }

    /// Returns whether the transition has fully finished at the given time.
    ///
    /// This is the scheduler-facing completion check used to decide whether the
    /// owning root should keep the node in its active transition set. It does
    /// not mutate transition state.
    #[must_use]
    pub fn is_finished(&self, now_s: f64) -> bool {
        now_s >= self.effective_end_s()
    }

    fn eased_progress(&self, now_s: f64) -> f32 {
        if self.duration_s == 0.0 {
            return 1.0;
        }
        let elapsed_s = (now_s - self.effective_start_s()).max(0.0);
        let duration_s = f64::from(self.duration_s);
        let progress = (elapsed_s / duration_s)
            .clamp(0.0, 1.0)
            .to_f32()
            .expect("transition progress should stay representable as f32");
        apply_timing(progress, self.timing)
    }

    pub(crate) fn effective_start_s(&self) -> f64 {
        self.started_at_s + f64::from(self.delay_s)
    }

    pub(crate) fn effective_end_s(&self) -> f64 {
        self.effective_start_s() + f64::from(self.duration_s)
    }
}

pub(crate) fn transition_spec_for_attribute(
    transition_property: Option<&DomAttributeValue>,
    transition_duration: Option<&DomAttributeValue>,
    transition_timing_function: Option<&DomAttributeValue>,
    transition_delay: Option<&DomAttributeValue>,
    target_attribute: &DomAttribute,
) -> Result<Option<DomTransitionSpec>, crate::types::DomError> {
    if transition_config_attribute(target_attribute) {
        return Ok(None);
    }
    let Some(index) =
        transition_property_index_for_attribute(transition_property, target_attribute)?
    else {
        return Ok(None);
    };
    let duration_s = transition_duration_for_index(transition_duration, index)?;
    if duration_s == 0.0 {
        return Ok(None);
    }
    Ok(Some(DomTransitionSpec {
        duration_s,
        delay_s: transition_delay_for_index(transition_delay, index)?,
        timing: transition_timing_for_index(transition_timing_function, index)?,
    }))
}

pub(crate) fn transition_longhand_values_from_shorthand(
    authored_transition: Option<&DomAttributeValue>,
) -> Result<TransitionLonghandValues, crate::types::DomError> {
    let Some(value) = authored_transition else {
        return Ok(TransitionLonghandValues::default());
    };
    let DomAttributeValue::String(value) = value else {
        return Err(crate::types::DomError::InvalidAttributeValue);
    };
    let expanded = expand_transition_shorthand(value)?;
    Ok(TransitionLonghandValues {
        property: Some(DomAttributeValue::String(expanded.property)),
        duration: Some(DomAttributeValue::String(expanded.duration)),
        timing_function: Some(DomAttributeValue::String(expanded.timing_function)),
        delay: Some(DomAttributeValue::String(expanded.delay)),
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ParsedTransitionItem<'a> {
    property_name: &'a str,
    spec: DomTransitionSpec,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct ExpandedTransitionShorthand {
    property: String,
    duration: String,
    timing_function: String,
    delay: String,
}

fn expand_transition_shorthand(
    value: &str,
) -> Result<ExpandedTransitionShorthand, crate::types::DomError> {
    let mut expanded = ExpandedTransitionShorthand::default();
    let mut item_count = 0usize;
    let mut saw_none = false;
    for_each_comma_separated_css_value(value, |_, item| {
        let parsed = parse_transition_item(item)?;
        if parsed.property_name == "none" {
            saw_none = true;
        }
        push_transition_item_separator(&mut expanded, item_count);
        expanded.property.push_str(parsed.property_name);
        write_transition_time_s(&mut expanded.duration, parsed.spec.duration_s);
        expanded
            .timing_function
            .push_str(timing_function_name(parsed.spec.timing));
        write_transition_time_s(&mut expanded.delay, parsed.spec.delay_s);
        item_count += 1;
        Ok(())
    })?;
    if item_count > 1 && saw_none {
        return Err(crate::types::DomError::InvalidAttributeValue);
    }
    Ok(expanded)
}

fn push_transition_item_separator(expanded: &mut ExpandedTransitionShorthand, count: usize) {
    if count == 0 {
        return;
    }
    expanded.property.push_str(", ");
    expanded.duration.push_str(", ");
    expanded.timing_function.push_str(", ");
    expanded.delay.push_str(", ");
}

fn parse_transition_item(item: &str) -> Result<ParsedTransitionItem<'_>, crate::types::DomError> {
    let mut property_name = None;
    let mut duration_s = None;
    let mut delay_s = None;
    let mut timing = None;
    for token in item.split_ascii_whitespace() {
        if let Some(next_timing) = parse_timing_function(token) {
            if timing.is_some() {
                return Err(crate::types::DomError::InvalidAttributeValue);
            }
            timing = Some(next_timing);
            continue;
        }
        if let Some(parsed_time_s) = parse_transition_time_s(token) {
            if duration_s.is_none() {
                if parsed_time_s < 0.0 {
                    return Err(crate::types::DomError::InvalidAttributeValue);
                }
                duration_s = Some(parsed_time_s);
                continue;
            }
            if delay_s.is_none() {
                delay_s = Some(parsed_time_s);
                continue;
            }
            return Err(crate::types::DomError::InvalidAttributeValue);
        }
        if unsupported_transition_function_token(token)
            || property_name.is_some()
            || !valid_transition_property_name(token)
        {
            return Err(crate::types::DomError::InvalidAttributeValue);
        }
        property_name = Some(token);
    }
    let property_name = property_name.unwrap_or("all");
    Ok(ParsedTransitionItem {
        property_name,
        spec: DomTransitionSpec {
            duration_s: duration_s.unwrap_or(0.0),
            delay_s: delay_s.unwrap_or(0.0),
            timing: timing.unwrap_or(DomTimingFunction::Ease),
        },
    })
}

fn string_attr_value(
    value: Option<&DomAttributeValue>,
) -> Result<Option<&str>, crate::types::DomError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let DomAttributeValue::String(value) = value else {
        return Err(crate::types::DomError::InvalidAttributeValue);
    };
    Ok(Some(value))
}

fn for_each_comma_separated_css_value(
    value: &str,
    mut visit: impl FnMut(usize, &str) -> Result<(), crate::types::DomError>,
) -> Result<usize, crate::types::DomError> {
    let mut depth = 0usize;
    let mut start = 0usize;
    let mut count = 0usize;
    for (index, character) in value.char_indices() {
        match character {
            '(' => depth = depth.saturating_add(1),
            ')' => {
                depth = depth
                    .checked_sub(1)
                    .ok_or(crate::types::DomError::InvalidAttributeValue)?;
            }
            ',' if depth == 0 => {
                visit(count, trim_non_empty(&value[start..index])?)?;
                count += 1;
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    if depth != 0 {
        return Err(crate::types::DomError::InvalidAttributeValue);
    }
    visit(count, trim_non_empty(&value[start..])?)?;
    Ok(count + 1)
}

fn trim_non_empty(value: &str) -> Result<&str, crate::types::DomError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(crate::types::DomError::InvalidAttributeValue);
    }
    Ok(trimmed)
}

fn transition_property_index_for_attribute(
    transition_property: Option<&DomAttributeValue>,
    target_attribute: &DomAttribute,
) -> Result<Option<usize>, crate::types::DomError> {
    let property_value = string_attr_value(transition_property)?
        .unwrap_or("all")
        .trim();
    if property_value == "none" {
        return Ok(None);
    }
    let mut matched_index = None;
    for_each_comma_separated_css_value(property_value, |index, property_name| {
        if !valid_transition_property_name(property_name) || property_name == "none" {
            return Err(crate::types::DomError::InvalidAttributeValue);
        }
        if property_name == "all" {
            if transition_target_attribute_allowed(target_attribute) {
                matched_index = Some(index);
            }
            return Ok(());
        }
        if transition_property_name_matches_attribute(property_name, target_attribute)? {
            matched_index = Some(index);
        }
        Ok(())
    })?;
    Ok(matched_index)
}

fn transition_property_name_matches_attribute(
    property_name: &str,
    target_attribute: &DomAttribute,
) -> Result<bool, crate::types::DomError> {
    let attribute = DomAttribute::try_from(property_name)?;
    if !transition_target_attribute_allowed(&attribute) {
        return Ok(false);
    }
    Ok(attribute == *target_attribute)
}

fn transition_duration_for_index(
    transition_duration: Option<&DomAttributeValue>,
    index: usize,
) -> Result<f32, crate::types::DomError> {
    repeated_transition_list_value(transition_duration, index, "0ms", |value| {
        let duration_s = parse_transition_time_s(value)?;
        (duration_s >= 0.0).then_some(duration_s)
    })
}

fn transition_delay_for_index(
    transition_delay: Option<&DomAttributeValue>,
    index: usize,
) -> Result<f32, crate::types::DomError> {
    repeated_transition_list_value(transition_delay, index, "0ms", parse_transition_time_s)
}

fn transition_timing_for_index(
    transition_timing_function: Option<&DomAttributeValue>,
    index: usize,
) -> Result<DomTimingFunction, crate::types::DomError> {
    repeated_transition_list_value(
        transition_timing_function,
        index,
        "ease",
        parse_timing_function,
    )
}

fn repeated_transition_list_value<T: Copy>(
    value: Option<&DomAttributeValue>,
    index: usize,
    initial_value: &str,
    parse: impl Fn(&str) -> Option<T>,
) -> Result<T, crate::types::DomError> {
    let value = string_attr_value(value)?.unwrap_or(initial_value);
    let count = for_each_comma_separated_css_value(value, |_, item| {
        parse(item)
            .map(|_| ())
            .ok_or(crate::types::DomError::InvalidAttributeValue)
    })?;
    let target_index = index % count;
    let mut matched_value = None;
    for_each_comma_separated_css_value(value, |item_index, item| {
        if item_index == target_index {
            matched_value = Some(parse(item).ok_or(crate::types::DomError::InvalidAttributeValue)?);
        }
        Ok(())
    })?;
    matched_value.ok_or(crate::types::DomError::InvalidAttributeValue)
}

fn transition_config_attribute(attribute: &DomAttribute) -> bool {
    matches!(
        attribute,
        DomAttribute::Transition
            | DomAttribute::TransitionProperty
            | DomAttribute::TransitionDuration
            | DomAttribute::TransitionTimingFunction
            | DomAttribute::TransitionDelay
    )
}

fn transition_target_attribute_allowed(attribute: &DomAttribute) -> bool {
    style_attr_allows_property(attribute) && !transition_config_attribute(attribute)
}

fn valid_transition_property_name(name: &str) -> bool {
    !name.is_empty()
        && !name.chars().any(char::is_whitespace)
        && !matches!(
            name,
            "initial" | "inherit" | "unset" | "revert" | "revert-layer"
        )
}

fn unsupported_transition_function_token(token: &str) -> bool {
    token.contains('(') || token.contains(')')
}

fn timing_function_name(timing: DomTimingFunction) -> &'static str {
    match timing {
        DomTimingFunction::Linear => "linear",
        DomTimingFunction::Ease => "ease",
        DomTimingFunction::EaseIn => "ease-in",
        DomTimingFunction::EaseOut => "ease-out",
        DomTimingFunction::EaseInOut => "ease-in-out",
    }
}

fn parse_timing_function(token: &str) -> Option<DomTimingFunction> {
    match token {
        "linear" => Some(DomTimingFunction::Linear),
        "ease" => Some(DomTimingFunction::Ease),
        "ease-in" => Some(DomTimingFunction::EaseIn),
        "ease-out" => Some(DomTimingFunction::EaseOut),
        "ease-in-out" => Some(DomTimingFunction::EaseInOut),
        _ => None,
    }
}

fn parse_transition_time_s(token: &str) -> Option<f32> {
    if let Some(milliseconds) = token.strip_suffix("ms") {
        let value = milliseconds.parse::<f32>().ok()?;
        return value.is_finite().then_some(value / 1000.0);
    }
    let seconds = token.strip_suffix('s')?;
    let value = seconds.parse::<f32>().ok()?;
    value.is_finite().then_some(value)
}

fn write_transition_time_s(output: &mut String, seconds: f32) {
    write!(output, "{seconds}s").expect("writing transition time to a String should not fail");
}

#[must_use]
fn interpolate_value(
    from: &DomAttributeValue,
    to: &DomAttributeValue,
    progress: f32,
) -> DomAttributeValue {
    match (from, to) {
        (DomAttributeValue::Float(from), DomAttributeValue::Float(to)) => {
            DomAttributeValue::Float(from + (to - from) * progress)
        }
        (DomAttributeValue::Color(from), DomAttributeValue::Color(to)) => {
            DomAttributeValue::Color([
                from[0] + (to[0] - from[0]) * progress,
                from[1] + (to[1] - from[1]) * progress,
                from[2] + (to[2] - from[2]) * progress,
                from[3] + (to[3] - from[3]) * progress,
            ])
        }
        _ if progress >= 1.0 => to.clone(),
        _ => from.clone(),
    }
}

#[must_use]
fn apply_timing(progress: f32, timing: DomTimingFunction) -> f32 {
    match timing {
        DomTimingFunction::Linear => progress,
        DomTimingFunction::Ease => cubic_bezier_progress(progress, 0.25, 0.1, 0.25, 1.0),
        DomTimingFunction::EaseIn => cubic_bezier_progress(progress, 0.42, 0.0, 1.0, 1.0),
        DomTimingFunction::EaseOut => cubic_bezier_progress(progress, 0.0, 0.0, 0.58, 1.0),
        DomTimingFunction::EaseInOut => cubic_bezier_progress(progress, 0.42, 0.0, 0.58, 1.0),
    }
}

fn cubic_bezier_progress(progress: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    let t = cubic_bezier_t_for_x(progress, x1, x2);
    cubic_bezier_value(t, y1, y2)
}

fn cubic_bezier_t_for_x(x: f32, x1: f32, x2: f32) -> f32 {
    let mut low = 0.0;
    let mut high = 1.0;
    for _ in 0..12 {
        let mid = (low + high) * 0.5;
        if cubic_bezier_value(mid, x1, x2) < x {
            low = mid;
        } else {
            high = mid;
        }
    }
    (low + high) * 0.5
}

fn cubic_bezier_value(t: f32, point1: f32, point2: f32) -> f32 {
    let inverse_t = 1.0 - t;
    3.0 * inverse_t * inverse_t * t * point1 + 3.0 * inverse_t * t * t * point2 + t * t * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_shorthand_feeds_longhands() {
        let longhands = transition_longhand_values_from_shorthand(Some(
            &DomAttributeValue::String("width 120ms ease-in 40ms, transform 80ms".to_string()),
        ))
        .expect("transition shorthand should parse");
        let parsed = spec_from_longhands(&longhands, &DomAttribute::Width);

        assert_spec(parsed, 0.12, 0.04, DomTimingFunction::EaseIn);
    }

    #[test]
    fn transition_spec_uses_matching_longhand_index() {
        let parsed = transition_spec_for_attribute(
            Some(&DomAttributeValue::String("width, left".to_string())),
            Some(&DomAttributeValue::String("120ms, 80ms".to_string())),
            Some(&DomAttributeValue::String("ease-in, ease-out".to_string())),
            Some(&DomAttributeValue::String("40ms, -20ms".to_string())),
            &DomAttribute::Left,
        )
        .expect("transition longhands should parse");

        assert_spec(parsed, 0.08, -0.02, DomTimingFunction::EaseOut);
    }

    #[test]
    fn transition_spec_accepts_fractional_time_values() {
        let parsed = transition_spec_for_attribute(
            Some(&DomAttributeValue::String("left".to_string())),
            Some(&DomAttributeValue::String("0.25s".to_string())),
            None,
            Some(&DomAttributeValue::String("-12.5ms".to_string())),
            &DomAttribute::Left,
        )
        .expect("fractional transition times should parse");

        assert_spec(parsed, 0.25, -0.0125, DomTimingFunction::Ease);
    }

    #[test]
    fn transition_spec_repeats_shorter_longhand_lists() {
        let parsed = transition_spec_for_attribute(
            Some(&DomAttributeValue::String("top, left".to_string())),
            Some(&DomAttributeValue::String("80ms".to_string())),
            Some(&DomAttributeValue::String("linear".to_string())),
            Some(&DomAttributeValue::String("0ms".to_string())),
            &DomAttribute::Left,
        )
        .expect("transition longhands should parse");

        assert_spec(parsed, 0.08, 0.0, DomTimingFunction::Linear);
    }

    #[test]
    fn transition_spec_uses_last_matching_property_entry() {
        let parsed = transition_spec_for_attribute(
            Some(&DomAttributeValue::String("all, left".to_string())),
            Some(&DomAttributeValue::String("80ms, 180ms".to_string())),
            None,
            None,
            &DomAttribute::Left,
        )
        .expect("transition longhands should parse");

        assert_spec(parsed, 0.18, 0.0, DomTimingFunction::Ease);
    }

    #[test]
    fn transition_spec_preserves_unknown_property_indices() {
        let parsed = transition_spec_for_attribute(
            Some(&DomAttributeValue::String("mystery, width".to_string())),
            Some(&DomAttributeValue::String("80ms, 180ms".to_string())),
            None,
            None,
            &DomAttribute::Width,
        )
        .expect("unknown transition-property identifiers should be kept");

        assert_spec(parsed, 0.18, 0.0, DomTimingFunction::Ease);
    }

    #[test]
    fn transition_property_none_disables_transitions() {
        let parsed = transition_spec_for_attribute(
            Some(&DomAttributeValue::String("none".to_string())),
            Some(&DomAttributeValue::String("180ms".to_string())),
            None,
            None,
            &DomAttribute::Width,
        )
        .expect("transition-property none should parse");

        assert_eq!(parsed, None);
    }

    #[test]
    fn missing_transition_duration_uses_initial_zero_duration() {
        let parsed = transition_spec_for_attribute(
            Some(&DomAttributeValue::String("width".to_string())),
            None,
            None,
            None,
            &DomAttribute::Width,
        )
        .expect("initial transition-duration should parse");

        assert_eq!(parsed, None);
    }

    #[test]
    fn transition_shorthand_rejects_negative_duration() {
        let error = transition_longhand_values_from_shorthand(Some(&DomAttributeValue::String(
            "width -20ms".to_string(),
        )))
        .expect_err("negative duration should fail");

        assert_eq!(error, DomError::InvalidAttributeValue);
    }

    #[test]
    fn transition_shorthand_rejects_unsupported_function_token() {
        let error = transition_longhand_values_from_shorthand(Some(&DomAttributeValue::String(
            "cubic-bezier(.4,0,.2,1) 180ms".to_string(),
        )))
        .expect_err("unsupported timing function should not become a property name");

        assert_eq!(error, DomError::InvalidAttributeValue);
    }

    fn spec_from_longhands(
        longhands: &TransitionLonghandValues,
        target_attribute: &DomAttribute,
    ) -> Option<DomTransitionSpec> {
        transition_spec_for_attribute(
            longhands.property.as_ref(),
            longhands.duration.as_ref(),
            longhands.timing_function.as_ref(),
            longhands.delay.as_ref(),
            target_attribute,
        )
        .expect("longhand values should resolve")
    }

    fn assert_spec(
        actual: Option<DomTransitionSpec>,
        duration_s: f32,
        delay_s: f32,
        timing: DomTimingFunction,
    ) {
        let spec = actual.expect("transition spec should exist");
        assert!((spec.duration_s - duration_s).abs() <= f32::EPSILON);
        assert!((spec.delay_s - delay_s).abs() <= f32::EPSILON);
        assert_eq!(spec.timing, timing);
    }
}
