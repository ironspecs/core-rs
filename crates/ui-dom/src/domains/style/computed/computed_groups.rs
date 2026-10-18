//! The seven computed groups held together (PLAN.md S1.1).
//!
//! This file owns the `ComputedGroups` struct: the container that carries the
//! seven inheritance-grouped computed groups as one inline `Copy` value. It is
//! plain `Copy` data plus `initial()` and `seed_for_child()`; the resolver owns
//! the per-group resolution, never this file.
//!
//! Invariants and rules:
//!
//! - **The field is named `box_`, not `box`.** `box` is a reserved keyword, so
//!   the field carries a trailing underscore (the established Rust idiom).
//! - **`initial()` is the no-parent seed**: all seven groups at their own
//!   `initial()`. The style pass seeds inherited groups from the parent
//!   instead; `initial()` exists for the ancestor/root seed and as the empty
//!   starting point.
//! - **`seed_for_child` is the A5 defaulting rule** (CSS Cascade §7, Servo
//!   `StyleBuilder` shape): the three inherited groups (font, text, color) are
//!   copied from `self`, the four non-inherited groups (box, flex, visual,
//!   transform) are reset to their `initial()`.

use super::computed_box_group::ComputedBoxGroup;
use super::computed_color_group::ComputedColorGroup;
use super::computed_flex_group::ComputedFlexGroup;
use super::computed_font_group::ComputedFontGroup;
use super::computed_text_group::ComputedTextGroup;
use super::computed_transform_group::ComputedTransformGroup;
use super::computed_visual_group::ComputedVisualGroup;

#[cfg(test)]
use super::BoxLength;

#[cfg(test)]
use crate::types::DomTextAlign;

/// The seven computed groups as one inline `Copy` value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ComputedGroups {
    pub font: ComputedFontGroup,
    pub text: ComputedTextGroup,
    pub color: ComputedColorGroup,
    /// Named `box_` because `box` is a reserved Rust keyword.
    pub box_: ComputedBoxGroup,
    pub flex: ComputedFlexGroup,
    pub visual: ComputedVisualGroup,
    pub transform: ComputedTransformGroup,
}

impl ComputedGroups {
    /// The no-parent seed: every group at its own `initial()`.
    #[must_use]
    pub fn initial() -> Self {
        Self {
            font: ComputedFontGroup::initial(),
            text: ComputedTextGroup::initial(),
            color: ComputedColorGroup::initial(),
            box_: ComputedBoxGroup::initial(),
            flex: ComputedFlexGroup::initial(),
            visual: ComputedVisualGroup::initial(),
            transform: ComputedTransformGroup::initial(),
        }
    }

    /// The child seed (A5): inherited groups copied, non-inherited reset.
    #[must_use]
    pub fn seed_for_child(&self) -> Self {
        Self {
            font: self.font,
            text: self.text,
            color: self.color,
            box_: ComputedBoxGroup::initial(),
            flex: ComputedFlexGroup::initial(),
            visual: ComputedVisualGroup::initial(),
            transform: ComputedTransformGroup::initial(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_matches_each_group_initial() {
        let groups = ComputedGroups::initial();
        assert_eq!(groups.font, ComputedFontGroup::initial());
        assert_eq!(groups.text, ComputedTextGroup::initial());
        assert_eq!(groups.color, ComputedColorGroup::initial());
        assert_eq!(groups.box_, ComputedBoxGroup::initial());
        assert_eq!(groups.flex, ComputedFlexGroup::initial());
        assert_eq!(groups.visual, ComputedVisualGroup::initial());
        assert_eq!(groups.transform, ComputedTransformGroup::initial());
    }

    #[test]
    fn seed_for_child_copies_inherited_and_resets_non_inherited() {
        let mut parent = ComputedGroups::initial();
        parent.font.size_px = 24.0;
        parent.text.align = DomTextAlign::End;
        parent.color.color_rgba = [1.0, 0.0, 0.0, 1.0];
        parent.box_.width = BoxLength::Px(100.0);
        parent.flex.grow = 3.0;
        parent.visual.opacity = 0.5;
        parent.transform.has_transform = true;

        let child = parent.seed_for_child();

        // Inherited groups copy verbatim.
        assert_eq!(child.font, parent.font);
        assert_eq!(child.text, parent.text);
        assert_eq!(child.color, parent.color);

        // Non-inherited groups reset to initial.
        assert_eq!(child.box_, ComputedBoxGroup::initial());
        assert_eq!(child.flex, ComputedFlexGroup::initial());
        assert_eq!(child.visual, ComputedVisualGroup::initial());
        assert_eq!(child.transform, ComputedTransformGroup::initial());
    }
}
