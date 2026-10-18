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
//! - transitions are keyed by `PresentationProperty` and interpolate typed
//!   `DeclValue`s (spine 3.3e); the `DomAttribute`/`DomAttributeValue` keying
//!   is gone from the retained objects
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

use arrayvec::ArrayVec;
use num_traits::ToPrimitive;

use crate::domains::style::computed::{
    ResolvedTransitionLonghands, TRANSITION_LONGHAND_CAPACITY, TransitionLonghandProperty,
};
use crate::domains::style::{DeclLength, DeclValue, Keyword, PresentationProperty};
use crate::types::DomAttributeValue;
use crate::types::dom_node::DomNodeSlotKey;

#[cfg(test)]
use crate::types::DomError;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DomTransitionState {
    pub active: BTreeMap<PresentationProperty, DomTransition>,
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
    pub property: PresentationProperty,
    pub from: DeclValue,
    pub to: DeclValue,
    pub presented: DeclValue,
    pub started_at_s: f64,
    pub duration_s: f32,
    pub delay_s: f32,
    pub timing: DomTimingFunction,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DomTransitionConfig {
    pub node_key: DomNodeSlotKey,
    pub property: PresentationProperty,
    pub from: DeclValue,
    pub to: DeclValue,
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
            property,
            from,
            to,
            started_at_s,
            duration_s,
            delay_s,
            timing,
        } = config;
        let presented = from;
        Self {
            node_key,
            property,
            from,
            to,
            presented,
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
        let previous_presented = self.presented;
        if now_s <= self.effective_start_s() {
            self.presented = self.from;
        } else if self.is_finished(now_s) {
            self.presented = self.to;
        } else {
            self.presented = interpolate_value(&self.from, &self.to, self.eased_progress(now_s));
        }
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

/// Resolves the transition spec for one property from the pass-written
/// `ResolvedTransitionLonghands` slot (spine 3.3g-1b). This replaces the
/// stringly `transition_spec_for_property` for pass-driven changes: the slot
/// stores the cascaded longhands as typed, bounded lists, so the scheduler
/// reads the slot instead of re-parsing the string longhand. Mirrors the CSS
/// list-repeat and last-matching-index semantics of the string form: an empty
/// property list defaults to `all` (CSS initial), an empty duration list to
/// `0s` (no transition), an empty timing list to `ease`, an empty delay list
/// to `0s`.
pub(crate) fn transition_spec_for_longhands(
    slot: &ResolvedTransitionLonghands,
    target: PresentationProperty,
) -> Option<DomTransitionSpec> {
    if transition_config_property(target) {
        return None;
    }
    let index = transition_property_index_in_slot(&slot.properties, target)?;
    let duration_s = repeated_slot_value(&slot.durations_s, index).unwrap_or(0.0);
    if duration_s == 0.0 {
        return None;
    }
    Some(DomTransitionSpec {
        duration_s,
        delay_s: repeated_slot_value(&slot.delays_s, index).unwrap_or(0.0),
        timing: repeated_slot_value(&slot.timing_functions, index)
            .and_then(timing_function_from_kw)
            .unwrap_or(DomTimingFunction::Ease),
    })
}

/// Finds the index of the last `transition-property` list entry that matches
/// the target, over the slot's typed list. An empty list defaults to `all`
/// (CSS `transition-property` initial); `None` entries never match (the whole
/// list being `none` therefore disables, matching the string form).
fn transition_property_index_in_slot(
    properties: &ArrayVec<TransitionLonghandProperty, TRANSITION_LONGHAND_CAPACITY>,
    target: PresentationProperty,
) -> Option<usize> {
    if properties.is_empty() {
        return Some(0);
    }
    let mut matched = None;
    for (index, entry) in properties.iter().enumerate() {
        match entry {
            TransitionLonghandProperty::All => matched = Some(index),
            TransitionLonghandProperty::Property(property) if *property == target => {
                matched = Some(index);
            }
            TransitionLonghandProperty::None | TransitionLonghandProperty::Property(_) => {}
        }
    }
    matched
}

/// Reads one entry of a bounded typed transition list with CSS list-repeat
/// (`index % len`); an empty list yields `None` (the callers apply the
/// longhand's CSS initial value).
fn repeated_slot_value<T: Copy>(
    list: &ArrayVec<T, TRANSITION_LONGHAND_CAPACITY>,
    index: usize,
) -> Option<T> {
    if list.is_empty() {
        return None;
    }
    Some(list[index % list.len()])
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

pub(crate) fn for_each_comma_separated_css_value(
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

/// The four transition longhands cannot themselves transition (the scheduler
/// config tier is excluded, matching the pre-re-key `transition_config_attribute`
/// rule).
fn transition_config_property(property: PresentationProperty) -> bool {
    matches!(
        property,
        PresentationProperty::TransitionProperty
            | PresentationProperty::TransitionDuration
            | PresentationProperty::TransitionTimingFunction
            | PresentationProperty::TransitionDelay
    )
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

const TIMING_KEYWORDS: &[Keyword] = &[
    Keyword::Linear,
    Keyword::Ease,
    Keyword::EaseIn,
    Keyword::EaseOut,
    Keyword::EaseInOut,
];

/// Maps the computed transition-longhand slot's timing `Keyword` to the public
/// closed `DomTimingFunction` enum (the 3.3g scheduler reads the slot, which
/// stores `Keyword`, instead of re-parsing the string longhand).
pub(crate) fn timing_function_from_kw(keyword: Keyword) -> Option<DomTimingFunction> {
    match keyword {
        Keyword::Linear => Some(DomTimingFunction::Linear),
        Keyword::Ease => Some(DomTimingFunction::Ease),
        Keyword::EaseIn => Some(DomTimingFunction::EaseIn),
        Keyword::EaseOut => Some(DomTimingFunction::EaseOut),
        Keyword::EaseInOut => Some(DomTimingFunction::EaseInOut),
        _ => None,
    }
}

pub(crate) fn parse_timing_function(token: &str) -> Option<DomTimingFunction> {
    Keyword::from_name_in(TIMING_KEYWORDS, token).and_then(timing_function_from_kw)
}

pub(crate) fn parse_transition_time_s(token: &str) -> Option<f32> {
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
fn interpolate_value(from: &DeclValue, to: &DeclValue, progress: f32) -> DeclValue {
    match (*from, *to) {
        (DeclValue::Length(DeclLength::Px(from)), DeclValue::Length(DeclLength::Px(to))) => {
            DeclValue::Length(DeclLength::Px(from + (to - from) * progress))
        }
        (DeclValue::Color(from), DeclValue::Color(to)) => DeclValue::Color([
            from[0] + (to[0] - from[0]) * progress,
            from[1] + (to[1] - from[1]) * progress,
            from[2] + (to[2] - from[2]) * progress,
            from[3] + (to[3] - from[3]) * progress,
        ]),
        (DeclValue::Number(from), DeclValue::Number(to)) => {
            DeclValue::Number(from + (to - from) * progress)
        }
        _ if progress >= 1.0 => *to,
        _ => *from,
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

    fn slot_from(
        properties: &[TransitionLonghandProperty],
        durations: &[f32],
        timings: &[Keyword],
        delays: &[f32],
    ) -> ResolvedTransitionLonghands {
        let mut slot = ResolvedTransitionLonghands::default();
        slot.properties.extend(properties.iter().copied());
        slot.durations_s.extend(durations.iter().copied());
        slot.timing_functions.extend(timings.iter().copied());
        slot.delays_s.extend(delays.iter().copied());
        slot
    }

    #[test]
    fn transition_shorthand_feeds_longhands() {
        let longhands = transition_longhand_values_from_shorthand(Some(
            &DomAttributeValue::String("width 120ms ease-in 40ms, transform 80ms".to_string()),
        ))
        .expect("transition shorthand should parse");

        assert_eq!(
            longhand_text(longhands.property.as_ref()),
            Some("width, transform")
        );
        assert_eq!(
            longhand_text(longhands.duration.as_ref()),
            Some("0.12s, 0.08s")
        );
        assert_eq!(
            longhand_text(longhands.timing_function.as_ref()),
            Some("ease-in, ease")
        );
        assert_eq!(longhand_text(longhands.delay.as_ref()), Some("0.04s, 0s"));
    }

    #[test]
    fn transition_spec_uses_matching_longhand_index() {
        let slot = slot_from(
            &[
                TransitionLonghandProperty::Property(PresentationProperty::Width),
                TransitionLonghandProperty::Property(PresentationProperty::Left),
            ],
            &[0.12, 0.08],
            &[Keyword::EaseIn, Keyword::EaseOut],
            &[0.04, -0.02],
        );

        assert_spec(
            transition_spec_for_longhands(&slot, PresentationProperty::Left),
            0.08,
            -0.02,
            DomTimingFunction::EaseOut,
        );
    }

    #[test]
    fn transition_spec_accepts_fractional_time_values() {
        let slot = slot_from(
            &[TransitionLonghandProperty::Property(
                PresentationProperty::Left,
            )],
            &[0.25],
            &[Keyword::Ease],
            &[-0.0125],
        );

        assert_spec(
            transition_spec_for_longhands(&slot, PresentationProperty::Left),
            0.25,
            -0.0125,
            DomTimingFunction::Ease,
        );
    }

    #[test]
    fn transition_spec_repeats_shorter_longhand_lists() {
        let slot = slot_from(
            &[
                TransitionLonghandProperty::Property(PresentationProperty::Top),
                TransitionLonghandProperty::Property(PresentationProperty::Left),
            ],
            &[0.08],
            &[Keyword::Linear],
            &[0.0],
        );

        assert_spec(
            transition_spec_for_longhands(&slot, PresentationProperty::Left),
            0.08,
            0.0,
            DomTimingFunction::Linear,
        );
    }

    #[test]
    fn transition_spec_uses_last_matching_property_entry() {
        let slot = slot_from(
            &[
                TransitionLonghandProperty::All,
                TransitionLonghandProperty::Property(PresentationProperty::Left),
            ],
            &[0.08, 0.18],
            &[Keyword::Ease],
            &[0.0],
        );

        assert_spec(
            transition_spec_for_longhands(&slot, PresentationProperty::Left),
            0.18,
            0.0,
            DomTimingFunction::Ease,
        );
    }

    #[test]
    fn transition_spec_matches_the_admitted_property_index() {
        // Unknown names never enter the slot (the projection drops them at
        // admission), so the property list is the admitted subset and its
        // indices align against that subset, not the raw authored list.
        let slot = slot_from(
            &[TransitionLonghandProperty::Property(
                PresentationProperty::Width,
            )],
            &[0.08, 0.18],
            &[Keyword::Ease],
            &[0.0],
        );

        assert_spec(
            transition_spec_for_longhands(&slot, PresentationProperty::Width),
            0.08,
            0.0,
            DomTimingFunction::Ease,
        );
    }

    #[test]
    fn transition_property_none_disables_transitions() {
        let slot = slot_from(
            &[TransitionLonghandProperty::None],
            &[0.18],
            &[Keyword::Ease],
            &[0.0],
        );

        assert_eq!(
            transition_spec_for_longhands(&slot, PresentationProperty::Width),
            None
        );
    }

    #[test]
    fn missing_transition_duration_uses_initial_zero_duration() {
        let slot = slot_from(
            &[TransitionLonghandProperty::Property(
                PresentationProperty::Width,
            )],
            &[],
            &[Keyword::Ease],
            &[0.0],
        );

        assert_eq!(
            transition_spec_for_longhands(&slot, PresentationProperty::Width),
            None
        );
    }

    #[test]
    fn empty_property_list_defaults_to_all() {
        // Absent `transition-property` defaults to `all` (CSS initial), so a
        // non-zero duration transitions any property.
        let slot = slot_from(&[], &[0.18], &[Keyword::Ease], &[0.0]);

        assert_spec(
            transition_spec_for_longhands(&slot, PresentationProperty::Width),
            0.18,
            0.0,
            DomTimingFunction::Ease,
        );
    }

    #[test]
    fn transition_config_properties_never_transition() {
        let slot = slot_from(
            &[TransitionLonghandProperty::All],
            &[0.18],
            &[Keyword::Ease],
            &[0.0],
        );
        for property in [
            PresentationProperty::TransitionProperty,
            PresentationProperty::TransitionDuration,
            PresentationProperty::TransitionTimingFunction,
            PresentationProperty::TransitionDelay,
        ] {
            assert_eq!(transition_spec_for_longhands(&slot, property), None);
        }
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

    #[test]
    fn interpolate_value_lerps_length_color_and_number_and_steps_other() {
        let from = DeclValue::Length(DeclLength::Px(0.0));
        let to = DeclValue::Length(DeclLength::Px(10.0));
        assert_eq!(
            interpolate_value(&from, &to, 0.5),
            DeclValue::Length(DeclLength::Px(5.0))
        );

        let from = DeclValue::Color([0.0, 0.0, 0.0, 1.0]);
        let to = DeclValue::Color([1.0, 0.0, 0.0, 1.0]);
        let mid = interpolate_value(&from, &to, 0.5);
        let DeclValue::Color(mid) = mid else {
            panic!("color interpolation should produce a color");
        };
        assert!((mid[0] - 0.5).abs() <= f32::EPSILON);

        let from = DeclValue::Number(0.0);
        let to = DeclValue::Number(2.0);
        assert_eq!(interpolate_value(&from, &to, 0.25), DeclValue::Number(0.5));

        // Keywords step: mid-flight keeps the `from` value; completion takes `to`.
        let from = DeclValue::Keyword(Keyword::Block);
        let to = DeclValue::Keyword(Keyword::Flex);
        assert_eq!(interpolate_value(&from, &to, 0.5), from);
        assert_eq!(interpolate_value(&from, &to, 1.0), to);
    }

    #[test]
    fn timing_function_from_kw_round_trips_the_closed_set() {
        for keyword in TIMING_KEYWORDS {
            let timing = timing_function_from_kw(*keyword)
                .expect("every timing keyword should map to a timing function");
            assert_eq!(parse_timing_function(keyword.name()), Some(timing));
        }
        assert_eq!(timing_function_from_kw(Keyword::Block), None);
    }

    fn longhand_text(value: Option<&DomAttributeValue>) -> Option<&str> {
        match value {
            Some(DomAttributeValue::String(s)) => Some(s.as_str()),
            _ => None,
        }
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
