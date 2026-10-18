//! Closed presentation-property registry for the `ui-dom` style system.
//!
//! This file owns the `PresentationProperty` enum, per-property metadata
//! (canonical name, cascade group, inherited flag, declaration grammar), and
//! name lookup. It is the single admission table for the typed declaration
//! family: the 68 properties, their groups, and the grammar each property's
//! declared value must satisfy.
//!
//! Invariants and rules:
//!
//! - `COUNT` and `ALL` are exact; `ALL` lists every variant in enum order and
//!   `index` maps each variant to its table row
//! - `METADATA` is one fixed `static` table indexed by variant; `name`, `group`,
//!   and `grammar` are O(1) reads, never a scan
//! - canonical names are unique and kebab-case; `Justified` serializes to the
//!   crate's materialized form `"justify"`

use super::keyword::Keyword;

/// The closed presentation vocabulary: 68 properties across the inherited
/// font/text/color groups and the non-inherited box/flex/visual/transform
/// groups, plus the four transition scheduler slots.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PresentationProperty {
    // Inherited — font group
    FontSize,
    LineHeight,
    FontFamily,
    FontWeight,
    FontStyle,
    // Inherited — text group
    TextAlign,
    Direction,
    WhiteSpace,
    OverflowWrap,
    WordBreak,
    // Inherited — color group
    Color,
    // Non-inherited — box group
    Display,
    Position,
    Overflow,
    Isolate,
    Width,
    Height,
    MinWidth,
    MinHeight,
    MaxWidth,
    MaxHeight,
    Top,
    Left,
    Right,
    Bottom,
    MarginTop,
    MarginRight,
    MarginBottom,
    MarginLeft,
    PaddingTop,
    PaddingRight,
    PaddingBottom,
    PaddingLeft,
    BorderTopWidth,
    BorderRightWidth,
    BorderBottomWidth,
    BorderLeftWidth,
    // Non-inherited — flex group
    FlexDirection,
    FlexWrap,
    FlexGrow,
    FlexShrink,
    FlexBasis,
    Order,
    JustifyContent,
    AlignItems,
    AlignSelf,
    AlignContent,
    RowGap,
    ColumnGap,
    // Non-inherited — visual group
    Opacity,
    BackgroundColor,
    BorderTopColor,
    BorderRightColor,
    BorderBottomColor,
    BorderLeftColor,
    OutlineColor,
    OutlineWidth,
    OutlineOffset,
    CaretColor,
    BorderTopLeftRadius,
    BorderTopRightRadius,
    BorderBottomRightRadius,
    BorderBottomLeftRadius,
    // Non-inherited — transform group
    Transform,
    // Transition scheduler slot (not a computed group)
    TransitionProperty,
    TransitionDuration,
    TransitionTimingFunction,
    TransitionDelay,
}

/// Cascade inheritance group for a presentation property.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PresentationGroup {
    Font,
    Text,
    Color,
    Box,
    Flex,
    Visual,
    Transform,
    TransitionLonghands,
}

/// Per-property declaration grammar (the admission table's grammar column).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DeclGrammar {
    /// `unset | 0 | <px> | <em>`.
    Length,
    /// `0 | <px> | <em>` (font-size: no unset).
    LengthNoUnset,
    /// `normal | <unitless >= 0> | <px> | <em>` (no percentages, no negatives).
    LineHeight,
    /// `transparent | #rrggbb | #rrggbbaa | rgb()/rgba() | white|black|red|green|blue`.
    Color,
    /// `<f32>` clamped to the 0.0..=1.0 range (opacity).
    Opacity,
    /// `<f32 >= 0>` (flex-grow, flex-shrink).
    NonNegativeNumber,
    /// `<i64>` (order).
    Integer,
    /// `normal | bold | bolder | lighter | <integer 100..=900 in steps of 100>`.
    FontWeight,
    /// `auto | content | 0 | <px> | <em>` (flex-basis; no unset).
    FlexBasis,
    /// `none | translateX(<px>) | translateY(<px>) | translate(<px>, <px>)` stored as interned text.
    Transform,
    /// Raw non-empty text, interned (font-family).
    Text,
    /// One of a closed keyword set.
    Keywords(&'static [Keyword]),
    /// Comma list of admitted presentation property names, `all`, or `none`; interned raw.
    TransitionPropertyList,
    /// Comma list of `<f32> >= 0` with "ms"/"s" units or bare "0"; interned raw
    /// (transition-duration; non-negative only per CSS Transitions 1 §2.4).
    TransitionTimeList,
    /// Comma list of any finite `<f32>` with "ms"/"s" units or bare "0";
    /// interned raw (transition-delay; negatives admitted per CSS Transitions 1
    /// §2.4).
    TransitionSignedTimeList,
    /// Comma list of timing keywords (linear|ease|ease-in|ease-out|ease-in-out); interned raw.
    TransitionTimingList,
}

const DISPLAY_KEYWORDS: &[Keyword] = &[
    Keyword::Block,
    Keyword::Inline,
    Keyword::InlineBlock,
    Keyword::None,
    Keyword::Flex,
    Keyword::InlineFlex,
];

const POSITION_KEYWORDS: &[Keyword] = &[
    Keyword::Static,
    Keyword::Relative,
    Keyword::Absolute,
    Keyword::Fixed,
];

const OVERFLOW_KEYWORDS: &[Keyword] = &[
    Keyword::Visible,
    Keyword::Scroll,
    Keyword::Auto,
    Keyword::Hidden,
    Keyword::Clip,
];

const ISOLATE_KEYWORDS: &[Keyword] = &[Keyword::Auto, Keyword::Isolate];

const TEXT_ALIGN_KEYWORDS: &[Keyword] = &[
    Keyword::Start,
    Keyword::Left,
    Keyword::Center,
    Keyword::Right,
    Keyword::Justified,
    Keyword::End,
];

const DIRECTION_KEYWORDS: &[Keyword] = &[Keyword::Ltr, Keyword::Rtl];

const WHITE_SPACE_KEYWORDS: &[Keyword] = &[
    Keyword::Normal,
    Keyword::Nowrap,
    Keyword::Pre,
    Keyword::PreWrap,
    Keyword::PreLine,
    Keyword::BreakSpaces,
];

const OVERFLOW_WRAP_KEYWORDS: &[Keyword] =
    &[Keyword::Normal, Keyword::BreakWord, Keyword::Anywhere];

const WORD_BREAK_KEYWORDS: &[Keyword] = &[
    Keyword::Normal,
    Keyword::BreakAll,
    Keyword::BreakWord,
    Keyword::KeepAll,
];

const FONT_STYLE_KEYWORDS: &[Keyword] = &[Keyword::Normal, Keyword::Italic];

const FLEX_DIRECTION_KEYWORDS: &[Keyword] = &[
    Keyword::Row,
    Keyword::RowReverse,
    Keyword::Column,
    Keyword::ColumnReverse,
];

const FLEX_WRAP_KEYWORDS: &[Keyword] = &[Keyword::Nowrap, Keyword::Wrap, Keyword::WrapReverse];

const JUSTIFY_CONTENT_KEYWORDS: &[Keyword] = &[
    Keyword::Start,
    Keyword::End,
    Keyword::FlexStart,
    Keyword::FlexEnd,
    Keyword::Center,
    Keyword::SpaceBetween,
    Keyword::SpaceAround,
    Keyword::SpaceEvenly,
];

const ALIGN_ITEMS_KEYWORDS: &[Keyword] = &[
    Keyword::Start,
    Keyword::End,
    Keyword::FlexStart,
    Keyword::FlexEnd,
    Keyword::Center,
    Keyword::Stretch,
];

const ALIGN_SELF_KEYWORDS: &[Keyword] = &[
    Keyword::Auto,
    Keyword::Start,
    Keyword::End,
    Keyword::FlexStart,
    Keyword::FlexEnd,
    Keyword::Center,
    Keyword::Stretch,
];

const ALIGN_CONTENT_KEYWORDS: &[Keyword] = &[
    Keyword::Start,
    Keyword::End,
    Keyword::FlexStart,
    Keyword::FlexEnd,
    Keyword::Center,
    Keyword::Stretch,
    Keyword::SpaceBetween,
    Keyword::SpaceAround,
    Keyword::SpaceEvenly,
];

type PropertyMetadata = (&'static str, PresentationGroup, DeclGrammar);

static METADATA: [PropertyMetadata; PresentationProperty::COUNT] = [
    (
        "font-size",
        PresentationGroup::Font,
        DeclGrammar::LengthNoUnset,
    ),
    (
        "line-height",
        PresentationGroup::Font,
        DeclGrammar::LineHeight,
    ),
    ("font-family", PresentationGroup::Font, DeclGrammar::Text),
    (
        "font-weight",
        PresentationGroup::Font,
        DeclGrammar::FontWeight,
    ),
    (
        "font-style",
        PresentationGroup::Font,
        DeclGrammar::Keywords(FONT_STYLE_KEYWORDS),
    ),
    (
        "text-align",
        PresentationGroup::Text,
        DeclGrammar::Keywords(TEXT_ALIGN_KEYWORDS),
    ),
    (
        "direction",
        PresentationGroup::Text,
        DeclGrammar::Keywords(DIRECTION_KEYWORDS),
    ),
    (
        "white-space",
        PresentationGroup::Text,
        DeclGrammar::Keywords(WHITE_SPACE_KEYWORDS),
    ),
    (
        "overflow-wrap",
        PresentationGroup::Text,
        DeclGrammar::Keywords(OVERFLOW_WRAP_KEYWORDS),
    ),
    (
        "word-break",
        PresentationGroup::Text,
        DeclGrammar::Keywords(WORD_BREAK_KEYWORDS),
    ),
    ("color", PresentationGroup::Color, DeclGrammar::Color),
    (
        "display",
        PresentationGroup::Box,
        DeclGrammar::Keywords(DISPLAY_KEYWORDS),
    ),
    (
        "position",
        PresentationGroup::Box,
        DeclGrammar::Keywords(POSITION_KEYWORDS),
    ),
    (
        "overflow",
        PresentationGroup::Box,
        DeclGrammar::Keywords(OVERFLOW_KEYWORDS),
    ),
    (
        "isolate",
        PresentationGroup::Box,
        DeclGrammar::Keywords(ISOLATE_KEYWORDS),
    ),
    ("width", PresentationGroup::Box, DeclGrammar::Length),
    ("height", PresentationGroup::Box, DeclGrammar::Length),
    ("min-width", PresentationGroup::Box, DeclGrammar::Length),
    ("min-height", PresentationGroup::Box, DeclGrammar::Length),
    ("max-width", PresentationGroup::Box, DeclGrammar::Length),
    ("max-height", PresentationGroup::Box, DeclGrammar::Length),
    ("top", PresentationGroup::Box, DeclGrammar::Length),
    ("left", PresentationGroup::Box, DeclGrammar::Length),
    ("right", PresentationGroup::Box, DeclGrammar::Length),
    ("bottom", PresentationGroup::Box, DeclGrammar::Length),
    ("margin-top", PresentationGroup::Box, DeclGrammar::Length),
    ("margin-right", PresentationGroup::Box, DeclGrammar::Length),
    ("margin-bottom", PresentationGroup::Box, DeclGrammar::Length),
    ("margin-left", PresentationGroup::Box, DeclGrammar::Length),
    ("padding-top", PresentationGroup::Box, DeclGrammar::Length),
    ("padding-right", PresentationGroup::Box, DeclGrammar::Length),
    (
        "padding-bottom",
        PresentationGroup::Box,
        DeclGrammar::Length,
    ),
    ("padding-left", PresentationGroup::Box, DeclGrammar::Length),
    (
        "border-top-width",
        PresentationGroup::Box,
        DeclGrammar::Length,
    ),
    (
        "border-right-width",
        PresentationGroup::Box,
        DeclGrammar::Length,
    ),
    (
        "border-bottom-width",
        PresentationGroup::Box,
        DeclGrammar::Length,
    ),
    (
        "border-left-width",
        PresentationGroup::Box,
        DeclGrammar::Length,
    ),
    (
        "flex-direction",
        PresentationGroup::Flex,
        DeclGrammar::Keywords(FLEX_DIRECTION_KEYWORDS),
    ),
    (
        "flex-wrap",
        PresentationGroup::Flex,
        DeclGrammar::Keywords(FLEX_WRAP_KEYWORDS),
    ),
    (
        "flex-grow",
        PresentationGroup::Flex,
        DeclGrammar::NonNegativeNumber,
    ),
    (
        "flex-shrink",
        PresentationGroup::Flex,
        DeclGrammar::NonNegativeNumber,
    ),
    (
        "flex-basis",
        PresentationGroup::Flex,
        DeclGrammar::FlexBasis,
    ),
    ("order", PresentationGroup::Flex, DeclGrammar::Integer),
    (
        "justify-content",
        PresentationGroup::Flex,
        DeclGrammar::Keywords(JUSTIFY_CONTENT_KEYWORDS),
    ),
    (
        "align-items",
        PresentationGroup::Flex,
        DeclGrammar::Keywords(ALIGN_ITEMS_KEYWORDS),
    ),
    (
        "align-self",
        PresentationGroup::Flex,
        DeclGrammar::Keywords(ALIGN_SELF_KEYWORDS),
    ),
    (
        "align-content",
        PresentationGroup::Flex,
        DeclGrammar::Keywords(ALIGN_CONTENT_KEYWORDS),
    ),
    ("row-gap", PresentationGroup::Flex, DeclGrammar::Length),
    ("column-gap", PresentationGroup::Flex, DeclGrammar::Length),
    ("opacity", PresentationGroup::Visual, DeclGrammar::Opacity),
    (
        "background-color",
        PresentationGroup::Visual,
        DeclGrammar::Color,
    ),
    (
        "border-top-color",
        PresentationGroup::Visual,
        DeclGrammar::Color,
    ),
    (
        "border-right-color",
        PresentationGroup::Visual,
        DeclGrammar::Color,
    ),
    (
        "border-bottom-color",
        PresentationGroup::Visual,
        DeclGrammar::Color,
    ),
    (
        "border-left-color",
        PresentationGroup::Visual,
        DeclGrammar::Color,
    ),
    (
        "outline-color",
        PresentationGroup::Visual,
        DeclGrammar::Color,
    ),
    (
        "outline-width",
        PresentationGroup::Visual,
        DeclGrammar::Length,
    ),
    (
        "outline-offset",
        PresentationGroup::Visual,
        DeclGrammar::Length,
    ),
    ("caret-color", PresentationGroup::Visual, DeclGrammar::Color),
    (
        "border-top-left-radius",
        PresentationGroup::Visual,
        DeclGrammar::Length,
    ),
    (
        "border-top-right-radius",
        PresentationGroup::Visual,
        DeclGrammar::Length,
    ),
    (
        "border-bottom-right-radius",
        PresentationGroup::Visual,
        DeclGrammar::Length,
    ),
    (
        "border-bottom-left-radius",
        PresentationGroup::Visual,
        DeclGrammar::Length,
    ),
    (
        "transform",
        PresentationGroup::Transform,
        DeclGrammar::Transform,
    ),
    (
        "transition-property",
        PresentationGroup::TransitionLonghands,
        DeclGrammar::TransitionPropertyList,
    ),
    (
        "transition-duration",
        PresentationGroup::TransitionLonghands,
        DeclGrammar::TransitionTimeList,
    ),
    (
        "transition-timing-function",
        PresentationGroup::TransitionLonghands,
        DeclGrammar::TransitionTimingList,
    ),
    (
        "transition-delay",
        PresentationGroup::TransitionLonghands,
        DeclGrammar::TransitionSignedTimeList,
    ),
];

impl PresentationProperty {
    /// Number of defined `PresentationProperty` variants.
    pub const COUNT: usize = 68;

    /// Every variant, in enum order.
    pub const ALL: [PresentationProperty; Self::COUNT] = [
        PresentationProperty::FontSize,
        PresentationProperty::LineHeight,
        PresentationProperty::FontFamily,
        PresentationProperty::FontWeight,
        PresentationProperty::FontStyle,
        PresentationProperty::TextAlign,
        PresentationProperty::Direction,
        PresentationProperty::WhiteSpace,
        PresentationProperty::OverflowWrap,
        PresentationProperty::WordBreak,
        PresentationProperty::Color,
        PresentationProperty::Display,
        PresentationProperty::Position,
        PresentationProperty::Overflow,
        PresentationProperty::Isolate,
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
        PresentationProperty::PaddingTop,
        PresentationProperty::PaddingRight,
        PresentationProperty::PaddingBottom,
        PresentationProperty::PaddingLeft,
        PresentationProperty::BorderTopWidth,
        PresentationProperty::BorderRightWidth,
        PresentationProperty::BorderBottomWidth,
        PresentationProperty::BorderLeftWidth,
        PresentationProperty::FlexDirection,
        PresentationProperty::FlexWrap,
        PresentationProperty::FlexGrow,
        PresentationProperty::FlexShrink,
        PresentationProperty::FlexBasis,
        PresentationProperty::Order,
        PresentationProperty::JustifyContent,
        PresentationProperty::AlignItems,
        PresentationProperty::AlignSelf,
        PresentationProperty::AlignContent,
        PresentationProperty::RowGap,
        PresentationProperty::ColumnGap,
        PresentationProperty::Opacity,
        PresentationProperty::BackgroundColor,
        PresentationProperty::BorderTopColor,
        PresentationProperty::BorderRightColor,
        PresentationProperty::BorderBottomColor,
        PresentationProperty::BorderLeftColor,
        PresentationProperty::OutlineColor,
        PresentationProperty::OutlineWidth,
        PresentationProperty::OutlineOffset,
        PresentationProperty::CaretColor,
        PresentationProperty::BorderTopLeftRadius,
        PresentationProperty::BorderTopRightRadius,
        PresentationProperty::BorderBottomRightRadius,
        PresentationProperty::BorderBottomLeftRadius,
        PresentationProperty::Transform,
        PresentationProperty::TransitionProperty,
        PresentationProperty::TransitionDuration,
        PresentationProperty::TransitionTimingFunction,
        PresentationProperty::TransitionDelay,
    ];

    /// Returns the fixed table index for this property.
    #[must_use]
    pub(crate) const fn index(self) -> usize {
        match self {
            Self::FontSize => 0,
            Self::LineHeight => 1,
            Self::FontFamily => 2,
            Self::FontWeight => 3,
            Self::FontStyle => 4,
            Self::TextAlign => 5,
            Self::Direction => 6,
            Self::WhiteSpace => 7,
            Self::OverflowWrap => 8,
            Self::WordBreak => 9,
            Self::Color => 10,
            Self::Display => 11,
            Self::Position => 12,
            Self::Overflow => 13,
            Self::Isolate => 14,
            Self::Width => 15,
            Self::Height => 16,
            Self::MinWidth => 17,
            Self::MinHeight => 18,
            Self::MaxWidth => 19,
            Self::MaxHeight => 20,
            Self::Top => 21,
            Self::Left => 22,
            Self::Right => 23,
            Self::Bottom => 24,
            Self::MarginTop => 25,
            Self::MarginRight => 26,
            Self::MarginBottom => 27,
            Self::MarginLeft => 28,
            Self::PaddingTop => 29,
            Self::PaddingRight => 30,
            Self::PaddingBottom => 31,
            Self::PaddingLeft => 32,
            Self::BorderTopWidth => 33,
            Self::BorderRightWidth => 34,
            Self::BorderBottomWidth => 35,
            Self::BorderLeftWidth => 36,
            Self::FlexDirection => 37,
            Self::FlexWrap => 38,
            Self::FlexGrow => 39,
            Self::FlexShrink => 40,
            Self::FlexBasis => 41,
            Self::Order => 42,
            Self::JustifyContent => 43,
            Self::AlignItems => 44,
            Self::AlignSelf => 45,
            Self::AlignContent => 46,
            Self::RowGap => 47,
            Self::ColumnGap => 48,
            Self::Opacity => 49,
            Self::BackgroundColor => 50,
            Self::BorderTopColor => 51,
            Self::BorderRightColor => 52,
            Self::BorderBottomColor => 53,
            Self::BorderLeftColor => 54,
            Self::OutlineColor => 55,
            Self::OutlineWidth => 56,
            Self::OutlineOffset => 57,
            Self::CaretColor => 58,
            Self::BorderTopLeftRadius => 59,
            Self::BorderTopRightRadius => 60,
            Self::BorderBottomRightRadius => 61,
            Self::BorderBottomLeftRadius => 62,
            Self::Transform => 63,
            Self::TransitionProperty => 64,
            Self::TransitionDuration => 65,
            Self::TransitionTimingFunction => 66,
            Self::TransitionDelay => 67,
        }
    }

    /// Canonical kebab-case name.
    #[must_use]
    pub fn name(self) -> &'static str {
        METADATA[self.index()].0
    }

    /// Looks up a property by canonical name (linear scan over `ALL`).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|property| property.name() == name)
    }

    /// Cascade inheritance group.
    #[must_use]
    pub fn group(self) -> PresentationGroup {
        METADATA[self.index()].1
    }

    /// True for the inherited font/text/color groups.
    #[must_use]
    pub fn inherited(self) -> bool {
        matches!(
            self.group(),
            PresentationGroup::Font | PresentationGroup::Text | PresentationGroup::Color
        )
    }

    /// Declared-value grammar.
    #[must_use]
    pub fn grammar(self) -> DeclGrammar {
        METADATA[self.index()].2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_is_sixty_eight_and_all_round_trips() {
        assert_eq!(PresentationProperty::COUNT, 68);
        assert_eq!(PresentationProperty::ALL.len(), 68);
        for property in PresentationProperty::ALL {
            assert_eq!(
                PresentationProperty::from_name(property.name()),
                Some(property)
            );
        }
    }

    #[test]
    fn names_are_unique() {
        let mut seen: Vec<&str> = Vec::new();
        for property in PresentationProperty::ALL {
            let name = property.name();
            assert!(!seen.contains(&name), "duplicate name `{name}`");
            seen.push(name);
        }
        assert_eq!(seen.len(), PresentationProperty::COUNT);
    }

    #[test]
    fn group_and_inherited_spot_checks() {
        assert_eq!(
            PresentationProperty::FontSize.group(),
            PresentationGroup::Font
        );
        assert!(PresentationProperty::FontSize.inherited());
        assert_eq!(PresentationProperty::Width.group(), PresentationGroup::Box);
        assert!(!PresentationProperty::Width.inherited());
        assert_eq!(
            PresentationProperty::TransitionDelay.group(),
            PresentationGroup::TransitionLonghands
        );
        assert!(!PresentationProperty::TransitionDelay.inherited());
    }

    #[test]
    fn grammar_spot_checks() {
        assert_eq!(
            PresentationProperty::FontSize.grammar(),
            DeclGrammar::LengthNoUnset
        );
        assert_eq!(PresentationProperty::Color.grammar(), DeclGrammar::Color);

        let DeclGrammar::Keywords(align_self_keywords) = PresentationProperty::AlignSelf.grammar()
        else {
            panic!("AlignSelf should use the Keywords grammar");
        };
        assert!(align_self_keywords.contains(&Keyword::Auto));
        assert!(align_self_keywords.contains(&Keyword::Stretch));
        assert!(!align_self_keywords.contains(&Keyword::SpaceBetween));
    }
}
