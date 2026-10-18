//! The transition-longhand scheduler slot (PLAN.md S1.1).
//!
//! This file owns `ResolvedTransitionLonghands` and `TransitionLonghandProperty`:
//! the bounded, typed slot the style pass writes from cascade winners (spine
//! step 3) and the transition scheduler reads when a value change arrives. It
//! is **not** a computed group and **not** diff-classified: longhand changes
//! schedule nothing by themselves (today's longhand writes dirty nothing either
//! — verified: they appear in none of the invalidation classes). D8's
//! pseudo-state→transition chain depends on it — stylesheet rules, never just
//! the inline block, supply these longhands, so the cascaded result must
//! persist here.
//!
//! Invariants and rules:
//!
//! - **Bounded inline lists (capacity 4, ArrayVec-class).** The four longhand
//!   lists are fixed-capacity inline lists of typed entries, parsed once by the
//!   pass and never re-parsed per read. Over-capacity lists are rejected at
//!   declaration admission (`set`/`set_all` contracts + drop metric),
//!   revisitable via the admission policy (D6) if a consumer needs more.
//! - **The list-repeat rule is the consumer's concern, not this slot's.** CSS
//!   Transitions repeats the shortest of the four lists to the length of the
//!   longest; the scheduler applies that on read. This struct only stores the
//!   four as-authored lists.
//! - `timing_functions` holds exactly `Linear | Ease | EaseIn | EaseOut |
//!   EaseInOut` (`Keyword`).
//! - `TransitionLonghandProperty::None` is the `transition-property: none`
//!   keyword (nothing transitions), distinct from the empty-list default.

use arrayvec::ArrayVec;

use crate::domains::style::Keyword;
use crate::domains::style::PresentationProperty;

/// Fixed capacity for the four transition-longhand lists (the damage-rect
/// precedent, `ArrayVec`-class bound). Over-capacity lists are rejected at
/// declaration admission.
pub const TRANSITION_LONGHAND_CAPACITY: usize = 4;

/// One entry of the resolved `transition-property` list.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransitionLonghandProperty {
    /// The `all` keyword: every transitionable property.
    All,
    /// The `none` keyword: nothing transitions.
    None,
    /// One admitted presentation property name.
    Property(PresentationProperty),
}

/// The cascaded transition longhands, persisted on the node for the scheduler.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResolvedTransitionLonghands {
    pub properties: ArrayVec<TransitionLonghandProperty, TRANSITION_LONGHAND_CAPACITY>,
    pub durations_s: ArrayVec<f32, TRANSITION_LONGHAND_CAPACITY>,
    /// `Linear | Ease | EaseIn | EaseOut | EaseInOut`.
    pub timing_functions: ArrayVec<Keyword, TRANSITION_LONGHAND_CAPACITY>,
    pub delays_s: ArrayVec<f32, TRANSITION_LONGHAND_CAPACITY>,
}

impl ResolvedTransitionLonghands {
    /// True when no longhand lists are populated.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.properties.is_empty()
    }

    /// Number of entries in the property list (the leading list).
    #[must_use]
    pub fn len(&self) -> usize {
        self.properties.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_empty_with_zero_len() {
        let longhands = ResolvedTransitionLonghands::default();
        assert!(longhands.is_empty());
        assert_eq!(longhands.len(), 0);
    }

    #[test]
    fn property_list_drives_len_and_emptiness() {
        let mut longhands = ResolvedTransitionLonghands::default();
        longhands.properties.push(TransitionLonghandProperty::All);
        longhands.durations_s.push(0.2);
        longhands.timing_functions.push(Keyword::Ease);
        longhands.delays_s.push(0.0);

        assert!(!longhands.is_empty());
        assert_eq!(longhands.len(), 1);
    }

    #[test]
    fn property_variants_cover_all_none_and_specific() {
        assert_eq!(
            TransitionLonghandProperty::All,
            TransitionLonghandProperty::All
        );
        assert_eq!(
            TransitionLonghandProperty::None,
            TransitionLonghandProperty::None
        );
        assert_eq!(
            TransitionLonghandProperty::Property(PresentationProperty::FontSize),
            TransitionLonghandProperty::Property(PresentationProperty::FontSize)
        );
    }
}
