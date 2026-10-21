//! A7's static group-diff classification table (PLAN.md §A7, S1.2 step 4).
//!
//! This file owns `GroupDirtyClassification`: the four dirty classes a changed
//! computed group implies, the static per-group table, and the `diff` helper
//! that unions classifications across the seven groups. It is the prime
//! unit-test target of the computed-store step.
//!
//! Invariants and rules:
//!
//! - **The table is exact** (PLAN.md §A7): box → layout+paint+
//!   layer-membership; flex → layout+paint; transform → paint+layer-membership;
//!   visual → paint; color → paint; font → layout+paint+`remeasure_text`; text →
//!   layout+paint+`remeasure_text`; transition longhands → none (S1.1: not
//!   `diff`-classified).
//! - **`diff` compares the seven `Copy` groups and unions** the classification
//!   for every changed group — one `!=` per group, zero allocation. It never
//!   sees `TransitionLonghands`: that slot lives on `NodePresentation` beside
//!   `ComputedGroups`, not inside it.
//! - **No children-marking row exists** (A7): the pass walks the full dirty
//!   subtree, so every visited descendant is re-seeded from its parent during
//!   the same pass (A5).
//! - `union` is the OR of each class; `NONE` is the identity.

use crate::domains::style::PresentationGroup;

use super::computed_groups::ComputedGroups;

#[cfg(test)]
use crate::domains::style::computed::BoxLength;
#[cfg(test)]
use crate::types::DomTextAlign;

/// The dirty classes a changed computed group implies.
///
/// Four `bool` fields are deliberate: they are the closed dirty-class
/// vocabulary A7 emits (layout, paint, text remeasure, layer-membership
/// re-evaluation) — a fixed, exact set, not an open bag of flags.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub struct GroupDirtyClassification {
    pub layout: bool,
    pub paint: bool,
    pub remeasure_text: bool,
    pub reevaluate_layer_membership: bool,
}

impl GroupDirtyClassification {
    /// The identity classification: no dirty class set.
    pub const NONE: Self = Self {
        layout: false,
        paint: false,
        remeasure_text: false,
        reevaluate_layer_membership: false,
    };

    /// A7's static table: the dirty classes a changed group implies.
    #[must_use]
    pub fn for_group(group: PresentationGroup) -> Self {
        match group {
            PresentationGroup::Box => Self {
                layout: true,
                paint: true,
                remeasure_text: false,
                reevaluate_layer_membership: true,
            },
            PresentationGroup::Flex | PresentationGroup::Grid => Self {
                layout: true,
                paint: true,
                remeasure_text: false,
                reevaluate_layer_membership: false,
            },
            PresentationGroup::Transform => Self {
                layout: false,
                paint: true,
                remeasure_text: false,
                reevaluate_layer_membership: true,
            },
            PresentationGroup::Visual | PresentationGroup::Color => Self {
                layout: false,
                paint: true,
                remeasure_text: false,
                reevaluate_layer_membership: false,
            },
            PresentationGroup::Font | PresentationGroup::Text => Self {
                layout: true,
                paint: true,
                remeasure_text: true,
                reevaluate_layer_membership: false,
            },
            PresentationGroup::TransitionLonghands => Self::NONE,
        }
    }

    /// The OR of each class across two classifications.
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        Self {
            layout: self.layout || other.layout,
            paint: self.paint || other.paint,
            remeasure_text: self.remeasure_text || other.remeasure_text,
            reevaluate_layer_membership: self.reevaluate_layer_membership
                || other.reevaluate_layer_membership,
        }
    }

    /// Seven `Copy` compares → union of the classification for each changed
    /// group.
    #[must_use]
    pub fn diff(before: &ComputedGroups, after: &ComputedGroups) -> Self {
        let mut classification = Self::NONE;
        if before.font != after.font {
            classification = classification.union(Self::for_group(PresentationGroup::Font));
        }
        if before.text != after.text {
            classification = classification.union(Self::for_group(PresentationGroup::Text));
        }
        if before.color != after.color {
            classification = classification.union(Self::for_group(PresentationGroup::Color));
        }
        if before.box_ != after.box_ {
            classification = classification.union(Self::for_group(PresentationGroup::Box));
        }
        if before.flex != after.flex {
            classification = classification.union(Self::for_group(PresentationGroup::Flex));
        }
        if before.grid != after.grid {
            classification = classification.union(Self::for_group(PresentationGroup::Grid));
        }
        if before.visual != after.visual {
            classification = classification.union(Self::for_group(PresentationGroup::Visual));
        }
        if before.transform != after.transform {
            classification = classification.union(Self::for_group(PresentationGroup::Transform));
        }
        classification
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_is_the_zero_default() {
        assert_eq!(
            GroupDirtyClassification::NONE,
            GroupDirtyClassification::default()
        );
    }

    #[test]
    fn table_maps_every_group() {
        assert_eq!(
            GroupDirtyClassification::for_group(PresentationGroup::Box),
            GroupDirtyClassification {
                layout: true,
                paint: true,
                reevaluate_layer_membership: true,
                ..GroupDirtyClassification::NONE
            }
        );
        assert_eq!(
            GroupDirtyClassification::for_group(PresentationGroup::Flex),
            GroupDirtyClassification {
                layout: true,
                paint: true,
                ..GroupDirtyClassification::NONE
            }
        );
        assert_eq!(
            GroupDirtyClassification::for_group(PresentationGroup::Transform),
            GroupDirtyClassification {
                paint: true,
                reevaluate_layer_membership: true,
                ..GroupDirtyClassification::NONE
            }
        );
        assert_eq!(
            GroupDirtyClassification::for_group(PresentationGroup::Visual),
            GroupDirtyClassification {
                paint: true,
                ..GroupDirtyClassification::NONE
            }
        );
        assert_eq!(
            GroupDirtyClassification::for_group(PresentationGroup::Color),
            GroupDirtyClassification {
                paint: true,
                ..GroupDirtyClassification::NONE
            }
        );
        assert_eq!(
            GroupDirtyClassification::for_group(PresentationGroup::Font),
            GroupDirtyClassification {
                layout: true,
                paint: true,
                remeasure_text: true,
                ..GroupDirtyClassification::NONE
            }
        );
        assert_eq!(
            GroupDirtyClassification::for_group(PresentationGroup::Text),
            GroupDirtyClassification {
                layout: true,
                paint: true,
                remeasure_text: true,
                ..GroupDirtyClassification::NONE
            }
        );
        assert_eq!(
            GroupDirtyClassification::for_group(PresentationGroup::TransitionLonghands),
            GroupDirtyClassification::NONE
        );
    }

    #[test]
    fn union_is_identity_with_none_and_ors_otherwise() {
        let paint = GroupDirtyClassification::for_group(PresentationGroup::Color);
        assert_eq!(paint.union(GroupDirtyClassification::NONE), paint);
        assert_eq!(GroupDirtyClassification::NONE.union(paint), paint);

        let font = GroupDirtyClassification::for_group(PresentationGroup::Font);
        let box_ = GroupDirtyClassification::for_group(PresentationGroup::Box);
        let merged = font.union(box_);
        assert!(merged.layout);
        assert!(merged.paint);
        assert!(merged.remeasure_text);
        assert!(merged.reevaluate_layer_membership);
    }

    #[test]
    fn diff_detects_no_change_as_none() {
        let before = ComputedGroups::initial();
        let after = ComputedGroups::initial();
        assert_eq!(
            GroupDirtyClassification::diff(&before, &after),
            GroupDirtyClassification::NONE
        );
    }

    #[test]
    fn diff_detects_one_changed_field_per_group() {
        let before = ComputedGroups::initial();

        let mut after = before;
        after.font.size_px = before.font.size_px + 1.0;
        assert_eq!(
            GroupDirtyClassification::diff(&before, &after),
            GroupDirtyClassification::for_group(PresentationGroup::Font)
        );

        let mut after = before;
        after.text.align = DomTextAlign::End;
        assert_eq!(
            GroupDirtyClassification::diff(&before, &after),
            GroupDirtyClassification::for_group(PresentationGroup::Text)
        );

        let mut after = before;
        after.color.color_rgba[0] = 1.0;
        assert_eq!(
            GroupDirtyClassification::diff(&before, &after),
            GroupDirtyClassification::for_group(PresentationGroup::Color)
        );

        let mut after = before;
        after.box_.width = BoxLength::Px(100.0);
        assert_eq!(
            GroupDirtyClassification::diff(&before, &after),
            GroupDirtyClassification::for_group(PresentationGroup::Box)
        );

        let mut after = before;
        after.flex.grow = 1.0;
        assert_eq!(
            GroupDirtyClassification::diff(&before, &after),
            GroupDirtyClassification::for_group(PresentationGroup::Flex)
        );

        let mut after = before;
        after.visual.opacity = 0.5;
        assert_eq!(
            GroupDirtyClassification::diff(&before, &after),
            GroupDirtyClassification::for_group(PresentationGroup::Visual)
        );

        let mut after = before;
        after.transform.has_transform = true;
        assert_eq!(
            GroupDirtyClassification::diff(&before, &after),
            GroupDirtyClassification::for_group(PresentationGroup::Transform)
        );
    }

    #[test]
    fn diff_unions_multiple_changed_groups() {
        let before = ComputedGroups::initial();
        let mut after = before;
        after.font.size_px = before.font.size_px + 1.0;
        after.visual.opacity = 0.5;
        let merged = GroupDirtyClassification::diff(&before, &after);
        assert!(merged.layout);
        assert!(merged.paint);
        assert!(merged.remeasure_text);
        assert!(!merged.reevaluate_layer_membership);
    }
}
