//! Shared inherited-style context for retained visitors in `ui-dom`.
//!
//! This file owns the `InheritedStyleContext` value that style, layout, and
//! paint visitors carry down the DFS. Since spine step 3.3d it is a thin,
//! `Copy` wrapper over the three *inherited* computed groups (font, text,
//! color) — the A5 group-copy seed — instead of the legacy eight-`Option`
//! record. The `effective_*` accessor names and return shapes are kept where
//! callers are built around them; their source is now the group fields.
//!
//! Invariants and rules:
//!
//! - this file owns only the `InheritedStyleContext` struct and its `impl` block
//! - the carried value is exactly the three inherited groups: font, text, color
//! - every field is always resolved (a group carries a concrete computed value,
//!   seeded from the parent or from `initial()`); there is no "not inherited
//!   yet" state to distinguish — the `Default` is the no-parent seed
//! - effective accessors read the group fields directly, never error

use crate::domains::style::computed::{
    ComputedColorGroup, ComputedFontGroup, ComputedGroups, ComputedTextGroup,
};
use crate::types::{DomOverflowWrap, DomTextAlign, DomTextDirection, DomWhiteSpace, DomWordBreak};

#[cfg(test)]
use crate::domains::style::BoxLength;

/// The carried inherited style for one DFS frame: the three inherited groups.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct InheritedStyleContext {
    pub(crate) font: ComputedFontGroup,
    pub(crate) text: ComputedTextGroup,
    pub(crate) color: ComputedColorGroup,
}

impl InheritedStyleContext {
    /// Builds the carried context from a full seven-group value, keeping only
    /// the three inherited groups (the `ComputedGroups`-derived seed). Retained
    /// for the 3.3g typed winner fold; spine 3.3e derives the context from the
    /// overlay-merged groups instead (see `inherited_style.rs`).
    #[must_use]
    #[allow(clippy::large_types_passed_by_value)] // Copy data; the by-value seed is deliberate
    #[allow(dead_code)]
    pub(crate) fn from_groups(groups: ComputedGroups) -> Self {
        Self {
            font: groups.font,
            text: groups.text,
            color: groups.color,
        }
    }

    pub(crate) fn effective_font_size_px(&self) -> f32 {
        self.font.size_px
    }

    pub(crate) fn effective_white_space(&self) -> DomWhiteSpace {
        self.text.white_space
    }

    pub(crate) fn effective_overflow_wrap(&self) -> DomOverflowWrap {
        self.text.overflow_wrap
    }

    pub(crate) fn effective_word_break(&self) -> DomWordBreak {
        self.text.word_break
    }

    /// The effective line height. `None` means `normal` — the exact legacy
    /// contract (`self.line_height_px` passthrough); the downstream text layout
    /// resolves `normal` to `font_size_px` (see
    /// `text_layout::effective_line_height_px`), never here.
    pub(crate) fn effective_line_height_px(&self) -> Option<f32> {
        self.font.line_height_px
    }

    pub(crate) fn effective_text_align(&self) -> DomTextAlign {
        self.text.align
    }

    pub(crate) fn effective_text_direction(&self) -> DomTextDirection {
        self.text.direction
    }

    pub(crate) fn effective_color(&self) -> [f32; 4] {
        self.color.color_rgba
    }
}

impl Default for InheritedStyleContext {
    fn default() -> Self {
        Self {
            font: ComputedFontGroup::initial(),
            text: ComputedTextGroup::initial(),
            color: ComputedColorGroup::initial(),
        }
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn effective_methods_read_the_inherited_group_fields() {
        let mut font = ComputedFontGroup::initial();
        font.size_px = 20.0;
        font.line_height_px = Some(28.0);
        let mut text = ComputedTextGroup::initial();
        text.align = DomTextAlign::End;
        text.direction = DomTextDirection::Rtl;
        text.white_space = DomWhiteSpace::PreWrap;
        text.overflow_wrap = DomOverflowWrap::Anywhere;
        text.word_break = DomWordBreak::KeepAll;
        let mut color = ComputedColorGroup::initial();
        color.color_rgba = [1.0, 0.0, 0.0, 1.0];

        let context = InheritedStyleContext { font, text, color };

        assert_eq!(context.effective_font_size_px(), 20.0);
        assert_eq!(context.effective_line_height_px(), Some(28.0));
        assert_eq!(context.effective_text_align(), DomTextAlign::End);
        assert_eq!(context.effective_text_direction(), DomTextDirection::Rtl);
        assert_eq!(context.effective_white_space(), DomWhiteSpace::PreWrap);
        assert_eq!(context.effective_overflow_wrap(), DomOverflowWrap::Anywhere);
        assert_eq!(context.effective_word_break(), DomWordBreak::KeepAll);
        assert_eq!(context.effective_color(), [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn default_mirrors_the_no_parent_group_seed() {
        let context = InheritedStyleContext::default();
        assert_eq!(context.effective_font_size_px(), 14.0);
        assert_eq!(context.effective_line_height_px(), None);
        assert_eq!(context.effective_text_align(), DomTextAlign::Start);
        assert_eq!(context.effective_text_direction(), DomTextDirection::Ltr);
        assert_eq!(
            context.effective_color(),
            [240.0 / 255.0, 240.0 / 255.0, 240.0 / 255.0, 1.0]
        );
    }

    #[test]
    fn from_groups_keeps_only_the_three_inherited_groups() {
        let mut groups = ComputedGroups::initial();
        groups.font.size_px = 24.0;
        groups.text.align = DomTextAlign::End;
        groups.color.color_rgba = [1.0, 0.0, 0.0, 1.0];
        // Non-inherited groups must not leak into the carried context.
        groups.box_.width = BoxLength::Px(100.0);

        let context = InheritedStyleContext::from_groups(groups);
        assert_eq!(context.font.size_px, 24.0);
        assert_eq!(context.text.align, DomTextAlign::End);
        assert_eq!(context.color.color_rgba, [1.0, 0.0, 0.0, 1.0]);
    }
}
