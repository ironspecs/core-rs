//! Transition inverse-projection and payload-validation leaves.
//!
//! What this file represents:
//!
//! - the pure, stateless transition helpers that read computed groups or a
//!   transition payload and project/validate without touching `RootDom`:
//!   `decl_value_for_groups` (computed-group field → `DeclValue`, the inverse
//!   of the resolver's field writes) plus its edge-mapper and keyword
//!   resolvers, `validate_transition_payload` and its grammar check, and
//!   `transition_elapsed_s_at`
//!
//! Ownership boundary:
//!
//! - these leaves read only their parameters (`ComputedGroups`,
//!   `PresentationProperty`, `DeclValue`, `DomTransition`) — no `RootDom`
//!   state — so they live here in `types/` rather than in `root_dom.rs`
//! - the transition TICK (`sync_transitions_at_current_time`) stays in
//!   `root_dom.rs`: its per-node effects are inline `&mut RootDom` mutations
//!   (dirty walks, layer queues, event dispatch with the popup transition-end
//!   hook) that do not factor into a journal without changing the marks-vs-
//!   events ordering (back-off doctrine verdict c; see `root_dom.rs`)
//!
//! Invariants and rules:
//!
//! - `decl_value_for_groups` reads the RAW groups, never the transition
//!   overlay; the scheduler composes the overlay separately
//! - the transition scheduler slots (`transition-property` etc.) and
//!   text-valued sources the group already resolved away (font-family's
//!   `TextKey`) project to `None` and are never transitionable
//! - `validate_transition_payload` rejects the transition longhands and any
//!   `from`/`to` whose kind does not match the property's grammar
use crate::domains::style::computed::project::{
    overflow_wrap, text_align, text_direction, white_space, word_break,
};
use crate::domains::style::computed::{BoxLength, ComputedGroups, FlexBasisKw};
use crate::domains::style::{DeclGrammar, DeclLength, DeclValue, Keyword, PresentationProperty};
use crate::types::DomError;
use crate::types::dom_transition::DomTransition;

#[cfg(test)]
use crate::types::dom_node::DomNodeSlotKey;
#[cfg(test)]
use crate::types::dom_transition::{DomTimingFunction, DomTransitionConfig};

/// Converts one computed-group field back to its `DeclValue` (the inverse of
/// the resolver's field writes). Reads the *raw* groups, never the transition
/// overlay: the scheduler composes the overlay separately — a re-targeted
/// transition starts from the active transition's presented value, a fresh one
/// from the raw old field. `None` means the field has no representable
/// transitionable value (text-valued properties whose source the group already
/// resolved away — font-family's `TextKey`) or the transition scheduler slots
/// themselves, so no transition is scheduled. `Transform` IS representable:
/// the group hands the scheduler its resolved `Translate` pair (or
/// `Keyword(None)` when no transform is present).
#[allow(clippy::too_many_lines)] // mechanical field-by-field inverse projection
pub(crate) fn decl_value_for_groups(
    groups: &ComputedGroups,
    property: PresentationProperty,
) -> Option<DeclValue> {
    match property {
        // Inherited — font group
        PresentationProperty::FontSize => {
            Some(DeclValue::Length(DeclLength::Px(groups.font.size_px)))
        }
        PresentationProperty::LineHeight => match groups.font.line_height_px {
            Some(px) => Some(DeclValue::Length(DeclLength::Px(px))),
            None => Some(DeclValue::Keyword(Keyword::Normal)),
        },
        PresentationProperty::FontWeight => Some(DeclValue::Integer(i64::from(groups.font.weight))),
        PresentationProperty::FontStyle => Some(DeclValue::Keyword(groups.font.style)),
        // Inherited — text group
        PresentationProperty::TextAlign => Some(DeclValue::Keyword(text_keyword(
            PresentationProperty::TextAlign,
            groups.text.align,
            text_align,
        )?)),
        PresentationProperty::Direction => Some(DeclValue::Keyword(text_keyword(
            PresentationProperty::Direction,
            groups.text.direction,
            text_direction,
        )?)),
        PresentationProperty::WhiteSpace => Some(DeclValue::Keyword(text_keyword(
            PresentationProperty::WhiteSpace,
            groups.text.white_space,
            white_space,
        )?)),
        PresentationProperty::OverflowWrap => Some(DeclValue::Keyword(text_keyword(
            PresentationProperty::OverflowWrap,
            groups.text.overflow_wrap,
            overflow_wrap,
        )?)),
        PresentationProperty::WordBreak => Some(DeclValue::Keyword(text_keyword(
            PresentationProperty::WordBreak,
            groups.text.word_break,
            word_break,
        )?)),
        // Inherited — color group
        PresentationProperty::Color => Some(DeclValue::Color(groups.color.color_rgba)),
        // Non-inherited — box group
        PresentationProperty::Display => Some(DeclValue::Keyword(keyword_from_name(
            PresentationProperty::Display,
            groups.box_.display.name(),
        )?)),
        PresentationProperty::Position => Some(DeclValue::Keyword(keyword_from_name(
            PresentationProperty::Position,
            groups.box_.position.name(),
        )?)),
        PresentationProperty::Overflow => Some(DeclValue::Keyword(keyword_from_name(
            PresentationProperty::Overflow,
            groups.box_.overflow.name(),
        )?)),
        PresentationProperty::Isolate => Some(DeclValue::Keyword(if groups.box_.isolate {
            Keyword::Isolate
        } else {
            Keyword::Auto
        })),
        PresentationProperty::BoxSizing => Some(DeclValue::Keyword(keyword_from_name(
            PresentationProperty::BoxSizing,
            groups.box_.box_sizing.name(),
        )?)),
        PresentationProperty::Width => Some(optional_length_decl_value(groups.box_.width)),
        PresentationProperty::Height => Some(optional_length_decl_value(groups.box_.height)),
        PresentationProperty::MinWidth => Some(optional_length_decl_value(groups.box_.min_width)),
        PresentationProperty::MinHeight => Some(optional_length_decl_value(groups.box_.min_height)),
        PresentationProperty::MaxWidth => Some(optional_length_decl_value(groups.box_.max_width)),
        PresentationProperty::MaxHeight => Some(optional_length_decl_value(groups.box_.max_height)),
        PresentationProperty::Top => Some(box_edge_decl_value(groups.box_.top)),
        PresentationProperty::Left => Some(box_edge_decl_value(groups.box_.left)),
        PresentationProperty::Right => Some(box_edge_decl_value(groups.box_.right)),
        PresentationProperty::Bottom => Some(box_edge_decl_value(groups.box_.bottom)),
        PresentationProperty::MarginTop => Some(margin_edge_decl_value(groups.box_.margin[0])),
        PresentationProperty::MarginRight => Some(margin_edge_decl_value(groups.box_.margin[1])),
        PresentationProperty::MarginBottom => Some(margin_edge_decl_value(groups.box_.margin[2])),
        PresentationProperty::MarginLeft => Some(margin_edge_decl_value(groups.box_.margin[3])),
        PresentationProperty::PaddingTop => Some(inset_edge_decl_value(groups.box_.padding[0])),
        PresentationProperty::PaddingRight => Some(inset_edge_decl_value(groups.box_.padding[1])),
        PresentationProperty::PaddingBottom => Some(inset_edge_decl_value(groups.box_.padding[2])),
        PresentationProperty::PaddingLeft => Some(inset_edge_decl_value(groups.box_.padding[3])),
        PresentationProperty::BorderTopWidth => Some(DeclValue::Length(DeclLength::Px(
            groups.box_.border_width[0],
        ))),
        PresentationProperty::BorderRightWidth => Some(DeclValue::Length(DeclLength::Px(
            groups.box_.border_width[1],
        ))),
        PresentationProperty::BorderBottomWidth => Some(DeclValue::Length(DeclLength::Px(
            groups.box_.border_width[2],
        ))),
        PresentationProperty::BorderLeftWidth => Some(DeclValue::Length(DeclLength::Px(
            groups.box_.border_width[3],
        ))),
        // Non-inherited — flex group
        PresentationProperty::FlexDirection => Some(DeclValue::Keyword(keyword_from_name(
            PresentationProperty::FlexDirection,
            groups.flex.direction.name(),
        )?)),
        PresentationProperty::FlexWrap => Some(DeclValue::Keyword(keyword_from_name(
            PresentationProperty::FlexWrap,
            groups.flex.wrap.name(),
        )?)),
        PresentationProperty::FlexGrow => Some(DeclValue::Number(groups.flex.grow)),
        PresentationProperty::FlexShrink => Some(DeclValue::Number(groups.flex.shrink)),
        PresentationProperty::FlexBasis => match groups.flex.basis {
            FlexBasisKw::Auto => Some(DeclValue::Keyword(Keyword::Auto)),
            FlexBasisKw::Content => Some(DeclValue::Keyword(Keyword::Content)),
            FlexBasisKw::Length(px) => Some(DeclValue::Length(DeclLength::Px(px))),
        },
        PresentationProperty::Order => Some(DeclValue::Integer(i64::from(groups.flex.order))),
        PresentationProperty::JustifyContent => Some(DeclValue::Keyword(keyword_from_name(
            PresentationProperty::JustifyContent,
            groups.flex.justify_content.name(),
        )?)),
        PresentationProperty::AlignItems => Some(DeclValue::Keyword(keyword_from_name(
            PresentationProperty::AlignItems,
            groups.flex.align_items.name(),
        )?)),
        PresentationProperty::AlignSelf => match groups.flex.align_self {
            Some(align) => Some(DeclValue::Keyword(keyword_from_name(
                PresentationProperty::AlignSelf,
                align.name(),
            )?)),
            None => Some(DeclValue::Keyword(Keyword::Auto)),
        },
        PresentationProperty::AlignContent => Some(DeclValue::Keyword(keyword_from_name(
            PresentationProperty::AlignContent,
            groups.flex.align_content.name(),
        )?)),
        PresentationProperty::RowGap => Some(DeclValue::Length(DeclLength::Px(groups.flex.gap[0]))),
        PresentationProperty::ColumnGap => {
            Some(DeclValue::Length(DeclLength::Px(groups.flex.gap[1])))
        }
        // Non-inherited — grid group
        PresentationProperty::GridTemplateColumns => {
            Some(DeclValue::TrackList(groups.grid.template_columns))
        }
        PresentationProperty::GridTemplateRows => {
            Some(DeclValue::TrackList(groups.grid.template_rows))
        }
        PresentationProperty::GridTemplateAreas => {
            Some(DeclValue::GridAreas(groups.grid.template_areas))
        }
        PresentationProperty::GridAutoFlow => Some(DeclValue::GridAutoFlow(groups.grid.auto_flow)),
        PresentationProperty::GridAutoColumns => {
            Some(DeclValue::TrackSize(groups.grid.auto_columns))
        }
        PresentationProperty::GridAutoRows => Some(DeclValue::TrackSize(groups.grid.auto_rows)),
        PresentationProperty::GridColumnStart => {
            Some(DeclValue::GridLine(groups.grid.column_start))
        }
        PresentationProperty::GridColumnEnd => Some(DeclValue::GridLine(groups.grid.column_end)),
        PresentationProperty::GridRowStart => Some(DeclValue::GridLine(groups.grid.row_start)),
        PresentationProperty::GridRowEnd => Some(DeclValue::GridLine(groups.grid.row_end)),
        PresentationProperty::JustifyItems => Some(DeclValue::Keyword(keyword_from_name(
            PresentationProperty::JustifyItems,
            groups.grid.justify_items.name(),
        )?)),
        PresentationProperty::JustifySelf => Some(DeclValue::Keyword(keyword_from_name(
            PresentationProperty::JustifySelf,
            groups.grid.justify_self.name(),
        )?)),
        // Non-inherited — visual group
        PresentationProperty::Opacity => Some(DeclValue::Number(groups.visual.opacity)),
        PresentationProperty::BackgroundColor => {
            Some(DeclValue::Color(groups.visual.background_rgba))
        }
        PresentationProperty::BorderTopColor => {
            Some(DeclValue::Color(groups.visual.border_rgba[0]))
        }
        PresentationProperty::BorderRightColor => {
            Some(DeclValue::Color(groups.visual.border_rgba[1]))
        }
        PresentationProperty::BorderBottomColor => {
            Some(DeclValue::Color(groups.visual.border_rgba[2]))
        }
        PresentationProperty::BorderLeftColor => {
            Some(DeclValue::Color(groups.visual.border_rgba[3]))
        }
        PresentationProperty::OutlineColor => Some(DeclValue::Color(groups.visual.outline_rgba)),
        PresentationProperty::OutlineWidth => Some(DeclValue::Length(DeclLength::Px(
            groups.visual.outline_width,
        ))),
        PresentationProperty::OutlineOffset => Some(DeclValue::Length(DeclLength::Px(
            groups.visual.outline_offset,
        ))),
        // `caret-color: auto` (None) has no representable color `from`.
        PresentationProperty::CaretColor => groups.visual.caret_rgba.map(DeclValue::Color),
        PresentationProperty::BorderTopLeftRadius => {
            Some(DeclValue::Length(DeclLength::Px(groups.visual.radius[0])))
        }
        PresentationProperty::BorderTopRightRadius => {
            Some(DeclValue::Length(DeclLength::Px(groups.visual.radius[1])))
        }
        PresentationProperty::BorderBottomRightRadius => {
            Some(DeclValue::Length(DeclLength::Px(groups.visual.radius[2])))
        }
        PresentationProperty::BorderBottomLeftRadius => {
            Some(DeclValue::Length(DeclLength::Px(groups.visual.radius[3])))
        }
        // Transform is schedulable through the typed translate pair (spine
        // 3.3e follow-up): a present transform hands the scheduler its resolved
        // `[x, y]` translate, and its absence hands `Keyword(None)`.
        PresentationProperty::Transform => {
            if groups.transform.has_transform {
                Some(DeclValue::Translate(groups.transform.translate_px))
            } else {
                Some(DeclValue::Keyword(Keyword::None))
            }
        }
        // Text-valued properties whose source the group already resolved away
        // (font-family's TextKey, the `GridArea` longhand that the `grid-area`
        // shorthand no longer produces) and the transition scheduler slots are
        // never transitionable.
        PresentationProperty::FontFamily
        | PresentationProperty::GridArea
        | PresentationProperty::TransitionProperty
        | PresentationProperty::TransitionDuration
        | PresentationProperty::TransitionTimingFunction
        | PresentationProperty::TransitionDelay => None,
    }
}

/// Maps a `BoxLength` optional-size field back to a `DeclValue`; `Auto` (unset/
/// `auto`) is the group's default `Length(Unset)`.
fn optional_length_decl_value(value: BoxLength) -> DeclValue {
    match value {
        BoxLength::Auto => DeclValue::Length(DeclLength::Unset),
        BoxLength::Px(px) => DeclValue::Length(DeclLength::Px(px)),
        BoxLength::Percent(pct) => DeclValue::Length(DeclLength::Percent(pct)),
    }
}

/// Maps a `BoxLength` box-edge offset field back to a `DeclValue`; a missing
/// edge defaults to zero (`attribute_uses_zero_when_missing` semantics).
fn box_edge_decl_value(value: BoxLength) -> DeclValue {
    match value {
        BoxLength::Auto => DeclValue::Length(DeclLength::Px(0.0)),
        BoxLength::Px(px) => DeclValue::Length(DeclLength::Px(px)),
        BoxLength::Percent(pct) => DeclValue::Length(DeclLength::Percent(pct)),
    }
}

/// Maps a `BoxLength` margin edge back to a `DeclValue`; `Auto` (authored
/// `margin: auto`) is `Keyword(Auto)`, distinct from the absent `Px(0.0)`.
fn margin_edge_decl_value(value: BoxLength) -> DeclValue {
    match value {
        BoxLength::Auto => DeclValue::Keyword(Keyword::Auto),
        BoxLength::Px(px) => DeclValue::Length(DeclLength::Px(px)),
        BoxLength::Percent(pct) => DeclValue::Length(DeclLength::Percent(pct)),
    }
}

/// Maps a `BoxLength` padding edge back to a `DeclValue`; `Auto` never arrives
/// (the grammar rejects `padding: auto`), so it maps to the absent `Px(0.0)`.
fn inset_edge_decl_value(value: BoxLength) -> DeclValue {
    match value {
        BoxLength::Auto => DeclValue::Length(DeclLength::Px(0.0)),
        BoxLength::Px(px) => DeclValue::Length(DeclLength::Px(px)),
        BoxLength::Percent(pct) => DeclValue::Length(DeclLength::Percent(pct)),
    }
}

/// Resolves a keyword-grammar group field's canonical name back to its
/// `Keyword` within the property's admitted set.
fn keyword_from_name(property: PresentationProperty, name: &str) -> Option<Keyword> {
    let DeclGrammar::Keywords(set) = property.grammar() else {
        return None;
    };
    Keyword::from_name_in(set, name)
}

/// Resolves a text-group enum field back to its `Keyword` by scanning the
/// property's admitted keyword set for the member whose forward map equals the
/// field.
fn text_keyword<T: Copy + Eq>(
    property: PresentationProperty,
    field: T,
    forward: impl Fn(Keyword) -> Option<T>,
) -> Option<Keyword> {
    let DeclGrammar::Keywords(set) = property.grammar() else {
        return None;
    };
    set.iter()
        .copied()
        .find(|keyword| forward(*keyword) == Some(field))
}

/// Validates one scheduled transition payload: the scheduler slots never
/// transition, and both endpoints must match the property's grammar.
pub(crate) fn validate_transition_payload(
    transition: DomTransition,
) -> Result<DomTransition, DomError> {
    let property = transition.property;
    if transition_longhand_property(property)
        || !decl_value_matches_grammar(property, &transition.from)
        || !decl_value_matches_grammar(property, &transition.to)
    {
        return Err(DomError::InvalidAttributeValue);
    }
    Ok(transition)
}

fn transition_longhand_property(property: PresentationProperty) -> bool {
    matches!(
        property,
        PresentationProperty::TransitionProperty
            | PresentationProperty::TransitionDuration
            | PresentationProperty::TransitionTimingFunction
            | PresentationProperty::TransitionDelay
    )
}

/// True when a `DeclValue`'s kind is valid for the property's grammar (the
/// property grammar tables admit a closed kind per grammar). Range constraints
/// are enforced at parse time, not here.
fn decl_value_matches_grammar(property: PresentationProperty, value: &DeclValue) -> bool {
    match property.grammar() {
        DeclGrammar::Length
        | DeclGrammar::NonNegativeLength
        | DeclGrammar::LineWidth
        | DeclGrammar::SignedLineWidth
        | DeclGrammar::Gap => {
            matches!(value, DeclValue::Length(_))
        }
        DeclGrammar::LineHeight => matches!(
            value,
            DeclValue::Keyword(Keyword::Normal) | DeclValue::Number(_) | DeclValue::Length(_)
        ),
        DeclGrammar::Color => matches!(value, DeclValue::Color(_)),
        DeclGrammar::Opacity | DeclGrammar::NonNegativeNumber => {
            matches!(value, DeclValue::Number(_))
        }
        DeclGrammar::Integer => matches!(value, DeclValue::Integer(_)),
        DeclGrammar::FontWeight => matches!(
            value,
            DeclValue::Integer(_)
                | DeclValue::Keyword(
                    Keyword::Normal | Keyword::Bold | Keyword::Bolder | Keyword::Lighter
                )
        ),
        DeclGrammar::FlexBasis => matches!(
            value,
            DeclValue::Keyword(Keyword::Auto | Keyword::Content) | DeclValue::Length(_)
        ),
        DeclGrammar::Transform => {
            matches!(
                value,
                DeclValue::Keyword(Keyword::None) | DeclValue::Text(_) | DeclValue::Translate(_)
            )
        }
        DeclGrammar::Text
        | DeclGrammar::TransitionPropertyList
        | DeclGrammar::TransitionTimeList
        | DeclGrammar::TransitionSignedTimeList
        | DeclGrammar::TransitionTimingList => matches!(value, DeclValue::Text(_)),
        DeclGrammar::Keywords(set) => match value {
            DeclValue::Keyword(keyword) => set.contains(keyword),
            _ => false,
        },
        DeclGrammar::TrackList => matches!(value, DeclValue::TrackList(_)),
        DeclGrammar::FamilyList => matches!(value, DeclValue::FamilyList(_)),
        DeclGrammar::GridAreas => matches!(value, DeclValue::GridAreas(_)),
        DeclGrammar::GridAutoFlow => matches!(value, DeclValue::GridAutoFlow(_)),
        DeclGrammar::TrackSize => matches!(value, DeclValue::TrackSize(_)),
        DeclGrammar::GridLine => matches!(value, DeclValue::GridLine(_)),
    }
}

/// Returns the elapsed seconds of `transition` at `now_s` (used by the
/// scheduler to report cancellation elapsed time). Only the upper bound is
/// clamped to `effective_end_s()`; a `now_s` before `effective_start_s()`
/// yields a negative elapsed time (the lower bound is not clamped).
pub(crate) fn transition_elapsed_s_at(transition: &DomTransition, now_s: f64) -> f64 {
    now_s.min(transition.effective_end_s()) - transition.effective_start_s()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transition_for(
        property: PresentationProperty,
        from: DeclValue,
        to: DeclValue,
        delay_s: f32,
    ) -> DomTransition {
        DomTransition::new(DomTransitionConfig {
            node_key: DomNodeSlotKey::default(),
            property,
            from,
            to,
            started_at_s: 10.0,
            duration_s: 2.0,
            delay_s,
            timing: DomTimingFunction::Linear,
        })
    }

    #[test]
    fn decl_value_projects_default_font_size_and_rejects_scheduler_slots() {
        let groups = ComputedGroups::initial();
        assert_eq!(
            decl_value_for_groups(&groups, PresentationProperty::FontSize),
            Some(DeclValue::Length(DeclLength::Px(14.0)))
        );
        for property in [
            PresentationProperty::FontFamily,
            PresentationProperty::GridArea,
            PresentationProperty::TransitionProperty,
            PresentationProperty::TransitionDuration,
            PresentationProperty::TransitionTimingFunction,
            PresentationProperty::TransitionDelay,
        ] {
            assert_eq!(
                decl_value_for_groups(&groups, property),
                None,
                "{property:?} should be non-transitionable"
            );
        }
    }

    #[test]
    fn validate_transition_rejects_scheduler_longhands() {
        let transition = transition_for(
            PresentationProperty::TransitionProperty,
            DeclValue::Keyword(Keyword::All),
            DeclValue::Keyword(Keyword::All),
            0.0,
        );
        assert_eq!(
            validate_transition_payload(transition),
            Err(DomError::InvalidAttributeValue)
        );
    }

    #[test]
    fn validate_transition_rejects_endpoints_outside_the_grammar() {
        // `Width` admits only a length, so a color endpoint is rejected.
        let transition = transition_for(
            PresentationProperty::Width,
            DeclValue::Color([0.0, 0.0, 0.0, 1.0]),
            DeclValue::Length(DeclLength::Px(10.0)),
            0.0,
        );
        assert_eq!(
            validate_transition_payload(transition),
            Err(DomError::InvalidAttributeValue)
        );
    }

    #[test]
    fn validate_transition_accepts_a_grammar_matching_payload() {
        let transition = transition_for(
            PresentationProperty::Width,
            DeclValue::Length(DeclLength::Px(0.0)),
            DeclValue::Length(DeclLength::Px(10.0)),
            0.0,
        );
        assert_eq!(
            validate_transition_payload(transition.clone()),
            Ok(transition)
        );
    }

    #[test]
    fn transition_longhand_property_names_the_scheduler_slots() {
        for property in [
            PresentationProperty::TransitionProperty,
            PresentationProperty::TransitionDuration,
            PresentationProperty::TransitionTimingFunction,
            PresentationProperty::TransitionDelay,
        ] {
            assert!(transition_longhand_property(property));
        }
        assert!(!transition_longhand_property(PresentationProperty::Width));
    }

    #[test]
    fn decl_value_matches_grammar_admits_only_the_closed_kind() {
        assert!(decl_value_matches_grammar(
            PresentationProperty::Width,
            &DeclValue::Length(DeclLength::Px(1.0))
        ));
        assert!(!decl_value_matches_grammar(
            PresentationProperty::Width,
            &DeclValue::Color([0.0, 0.0, 0.0, 1.0])
        ));
        assert!(decl_value_matches_grammar(
            PresentationProperty::Order,
            &DeclValue::Integer(2)
        ));
        assert!(!decl_value_matches_grammar(
            PresentationProperty::Order,
            &DeclValue::Number(2.0)
        ));
    }

    #[test]
    fn transition_elapsed_clamps_to_the_effective_window() {
        // started_at 10 + delay 1 → effective [11, 13].
        let transition = transition_for(
            PresentationProperty::Width,
            DeclValue::Length(DeclLength::Px(0.0)),
            DeclValue::Length(DeclLength::Px(10.0)),
            1.0,
        );

        assert!((transition_elapsed_s_at(&transition, 10.5) - (-0.5)).abs() <= f64::EPSILON);
        assert!((transition_elapsed_s_at(&transition, 12.0) - 1.0).abs() <= f64::EPSILON);
        assert!((transition_elapsed_s_at(&transition, 14.0) - 2.0).abs() <= f64::EPSILON);
    }
}
