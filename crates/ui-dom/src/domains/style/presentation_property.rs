//! Closed presentation-property registry for the `ui-dom` style system.
//!
//! This file owns the `PresentationProperty` enum, per-property metadata
//! (canonical name, cascade group, inherited flag, declaration grammar), and
//! name lookup. It is the single admission table for the typed declaration
//! family: the 74 properties, their groups, and the grammar each property's
//! declared value must satisfy (68 at v0.6.0, growing via admission per the D6
//! policy).
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

/// The closed presentation vocabulary: 74 properties across the inherited
/// font/text/color groups and the non-inherited box/flex/grid/visual/transform
/// groups, plus the four transition scheduler slots (68 at v0.6.0, growing via
/// admission per the D6 policy).
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
    BoxSizing,
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
    // Non-inherited — grid group
    GridTemplateColumns,
    GridTemplateRows,
    GridTemplateAreas,
    GridAutoFlow,
    GridAutoColumns,
    GridAutoRows,
    GridColumnStart,
    GridColumnEnd,
    GridRowStart,
    GridRowEnd,
    GridArea,
    JustifyItems,
    JustifySelf,
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
    Grid,
    Visual,
    Transform,
    TransitionLonghands,
}

/// Per-property declaration grammar (the admission table's grammar column).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DeclGrammar {
    /// `unset | 0 | <px> | <em> | <%>` (signed: margins and position insets;
    /// percentages admitted and resolved per PR 1 item 7).
    Length,
    /// `unset | 0 | <non-negative px> | <non-negative em> | <non-negative %>`
    /// (padding, box sizes, font-size).
    NonNegativeLength,
    /// `unset | 0 | <non-negative px> | <non-negative em>` — the CSS
    /// `<line-width>` grammar (CSS Backgrounds 3 §4.3, CSS UI 4 §4.1), which
    /// admits NO percentages. Used by the four border widths, outline-width,
    /// and the four border-radius longhands. For the radius corners CSS
    /// Backgrounds 3 §5.1 admits `<length-percentage>`, but the
    /// circular-corner model (`radius: [f32; 4]`, one radius per corner) cannot
    /// represent the elliptical resolution CSS requires (50% on a non-square
    /// border box computes different horizontal and vertical radii per corner),
    /// so the percentage is rejected at admission exactly like `gap` (see
    /// `parse_gap_longhand`) — admission-gated on a corner-pair (elliptical)
    /// model, never silently resolved.
    LineWidth,
    /// `unset | 0 | <px> | <em>` (signed, NO percentages) — outline-offset
    /// (CSS UI 4 §4.3: a signed CSS `<length>` with no percentage form).
    SignedLineWidth,
    /// `normal | unset | 0 | <non-negative px> | <non-negative em>` (CSS Box
    /// Alignment 3 §8.2; `normal` computes to zero, matching the shorthand).
    Gap,
    /// `normal | <unitless >= 0> | <px> | <em>` (no percentages, no negatives).
    LineHeight,
    /// `transparent | #rrggbb | #rrggbbaa | rgb()/rgba() | white|black|red|green|blue`.
    Color,
    /// `<f32>` clamped to the 0.0–1.0 range — the grammar admits any finite
    /// f32 and clamps (opacity).
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
    /// A grid track-list (`grid-template-columns`/`rows`): space-separated
    /// `<track-size>` tokens with `[name]` line names, expanded at parse,
    /// interned value-deduped into the owning store's track-list arena. Capacity
    /// 12 tracks / 8 name registrations (over-capacity drops).
    TrackList,
    /// A `font-family` list: comma-separated quoted strings (quote pair
    /// stripped) or unquoted custom-ident sequences, with the generic keywords
    /// mapping to `Generic(_)`. Interned value-deduped into the owning store's
    /// family-list arena; empty items, empty lists, and unterminated quotes
    /// drop (text seam S2).
    FamilyList,
    /// A grid template-areas map (`grid-template-areas`): quoted string rows of
    /// dot-runs and `<custom-ident>` cells, validated (uniform columns,
    /// rectangularity, capacity) and interned value-deduped into the owning
    /// store's areas arena.
    GridAreas,
    /// A grid auto-flow (`[row | column] && dense?`), resolved to the four
    /// `GridAutoFlowKw` outcomes.
    GridAutoFlow,
    /// A single grid `<track-size>` leaf (`grid-auto-columns`/`rows`).
    TrackSize,
    /// A single grid `<grid-line>` (`auto | <nonzero-integer> | span
    /// <positive-integer>`) for the four placement longhands.
    GridLine,
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
    Keyword::Grid,
    Keyword::InlineGrid,
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

const BOX_SIZING_KEYWORDS: &[Keyword] = &[Keyword::ContentBox, Keyword::BorderBox];

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

const JUSTIFY_ITEMS_KEYWORDS: &[Keyword] = &[
    Keyword::Normal,
    Keyword::Stretch,
    Keyword::Start,
    Keyword::End,
    Keyword::Center,
];

const JUSTIFY_SELF_KEYWORDS: &[Keyword] = &[
    Keyword::Auto,
    Keyword::Normal,
    Keyword::Stretch,
    Keyword::Start,
    Keyword::End,
    Keyword::Center,
];

type PropertyMetadata = (&'static str, PresentationGroup, DeclGrammar);

static METADATA: [PropertyMetadata; PresentationProperty::COUNT] = [
    (
        "font-size",
        PresentationGroup::Font,
        DeclGrammar::NonNegativeLength,
    ),
    (
        "line-height",
        PresentationGroup::Font,
        DeclGrammar::LineHeight,
    ),
    (
        "font-family",
        PresentationGroup::Font,
        DeclGrammar::FamilyList,
    ),
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
    (
        "box-sizing",
        PresentationGroup::Box,
        DeclGrammar::Keywords(BOX_SIZING_KEYWORDS),
    ),
    (
        "width",
        PresentationGroup::Box,
        DeclGrammar::NonNegativeLength,
    ),
    (
        "height",
        PresentationGroup::Box,
        DeclGrammar::NonNegativeLength,
    ),
    (
        "min-width",
        PresentationGroup::Box,
        DeclGrammar::NonNegativeLength,
    ),
    (
        "min-height",
        PresentationGroup::Box,
        DeclGrammar::NonNegativeLength,
    ),
    (
        "max-width",
        PresentationGroup::Box,
        DeclGrammar::NonNegativeLength,
    ),
    (
        "max-height",
        PresentationGroup::Box,
        DeclGrammar::NonNegativeLength,
    ),
    ("top", PresentationGroup::Box, DeclGrammar::Length),
    ("left", PresentationGroup::Box, DeclGrammar::Length),
    ("right", PresentationGroup::Box, DeclGrammar::Length),
    ("bottom", PresentationGroup::Box, DeclGrammar::Length),
    ("margin-top", PresentationGroup::Box, DeclGrammar::Length),
    ("margin-right", PresentationGroup::Box, DeclGrammar::Length),
    ("margin-bottom", PresentationGroup::Box, DeclGrammar::Length),
    ("margin-left", PresentationGroup::Box, DeclGrammar::Length),
    (
        "padding-top",
        PresentationGroup::Box,
        DeclGrammar::NonNegativeLength,
    ),
    (
        "padding-right",
        PresentationGroup::Box,
        DeclGrammar::NonNegativeLength,
    ),
    (
        "padding-bottom",
        PresentationGroup::Box,
        DeclGrammar::NonNegativeLength,
    ),
    (
        "padding-left",
        PresentationGroup::Box,
        DeclGrammar::NonNegativeLength,
    ),
    (
        "border-top-width",
        PresentationGroup::Box,
        DeclGrammar::LineWidth,
    ),
    (
        "border-right-width",
        PresentationGroup::Box,
        DeclGrammar::LineWidth,
    ),
    (
        "border-bottom-width",
        PresentationGroup::Box,
        DeclGrammar::LineWidth,
    ),
    (
        "border-left-width",
        PresentationGroup::Box,
        DeclGrammar::LineWidth,
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
    ("row-gap", PresentationGroup::Flex, DeclGrammar::Gap),
    ("column-gap", PresentationGroup::Flex, DeclGrammar::Gap),
    (
        "grid-template-columns",
        PresentationGroup::Grid,
        DeclGrammar::TrackList,
    ),
    (
        "grid-template-rows",
        PresentationGroup::Grid,
        DeclGrammar::TrackList,
    ),
    (
        "grid-template-areas",
        PresentationGroup::Grid,
        DeclGrammar::GridAreas,
    ),
    (
        "grid-auto-flow",
        PresentationGroup::Grid,
        DeclGrammar::GridAutoFlow,
    ),
    (
        "grid-auto-columns",
        PresentationGroup::Grid,
        DeclGrammar::TrackSize,
    ),
    (
        "grid-auto-rows",
        PresentationGroup::Grid,
        DeclGrammar::TrackSize,
    ),
    (
        "grid-column-start",
        PresentationGroup::Grid,
        DeclGrammar::GridLine,
    ),
    (
        "grid-column-end",
        PresentationGroup::Grid,
        DeclGrammar::GridLine,
    ),
    (
        "grid-row-start",
        PresentationGroup::Grid,
        DeclGrammar::GridLine,
    ),
    (
        "grid-row-end",
        PresentationGroup::Grid,
        DeclGrammar::GridLine,
    ),
    ("grid-area", PresentationGroup::Grid, DeclGrammar::Text),
    (
        "justify-items",
        PresentationGroup::Grid,
        DeclGrammar::Keywords(JUSTIFY_ITEMS_KEYWORDS),
    ),
    (
        "justify-self",
        PresentationGroup::Grid,
        DeclGrammar::Keywords(JUSTIFY_SELF_KEYWORDS),
    ),
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
        DeclGrammar::LineWidth,
    ),
    (
        "outline-offset",
        PresentationGroup::Visual,
        DeclGrammar::SignedLineWidth,
    ),
    ("caret-color", PresentationGroup::Visual, DeclGrammar::Color),
    // The four radius longhands use `LineWidth` (no percentages), not
    // `NonNegativeLength`: CSS Backgrounds 3 §5.1 admits `<length-percentage>`,
    // but the circular-corner model (`radius: [f32; 4]`, one radius per corner)
    // cannot represent the elliptical resolution CSS requires (50% on a
    // non-square border box computes different horizontal and vertical radii
    // per corner). Percentages are rejected at admission exactly like `gap`
    // (see `parse_gap_longhand` in `decl_parse.rs`); admission is gated on a
    // corner-pair (elliptical) model — never silently resolved.
    (
        "border-top-left-radius",
        PresentationGroup::Visual,
        DeclGrammar::LineWidth,
    ),
    (
        "border-top-right-radius",
        PresentationGroup::Visual,
        DeclGrammar::LineWidth,
    ),
    (
        "border-bottom-right-radius",
        PresentationGroup::Visual,
        DeclGrammar::LineWidth,
    ),
    (
        "border-bottom-left-radius",
        PresentationGroup::Visual,
        DeclGrammar::LineWidth,
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
    pub const COUNT: usize = 82;

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
        PresentationProperty::BoxSizing,
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
        PresentationProperty::GridTemplateColumns,
        PresentationProperty::GridTemplateRows,
        PresentationProperty::GridTemplateAreas,
        PresentationProperty::GridAutoFlow,
        PresentationProperty::GridAutoColumns,
        PresentationProperty::GridAutoRows,
        PresentationProperty::GridColumnStart,
        PresentationProperty::GridColumnEnd,
        PresentationProperty::GridRowStart,
        PresentationProperty::GridRowEnd,
        PresentationProperty::GridArea,
        PresentationProperty::JustifyItems,
        PresentationProperty::JustifySelf,
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
            Self::BoxSizing => 15,
            Self::Width => 16,
            Self::Height => 17,
            Self::MinWidth => 18,
            Self::MinHeight => 19,
            Self::MaxWidth => 20,
            Self::MaxHeight => 21,
            Self::Top => 22,
            Self::Left => 23,
            Self::Right => 24,
            Self::Bottom => 25,
            Self::MarginTop => 26,
            Self::MarginRight => 27,
            Self::MarginBottom => 28,
            Self::MarginLeft => 29,
            Self::PaddingTop => 30,
            Self::PaddingRight => 31,
            Self::PaddingBottom => 32,
            Self::PaddingLeft => 33,
            Self::BorderTopWidth => 34,
            Self::BorderRightWidth => 35,
            Self::BorderBottomWidth => 36,
            Self::BorderLeftWidth => 37,
            Self::FlexDirection => 38,
            Self::FlexWrap => 39,
            Self::FlexGrow => 40,
            Self::FlexShrink => 41,
            Self::FlexBasis => 42,
            Self::Order => 43,
            Self::JustifyContent => 44,
            Self::AlignItems => 45,
            Self::AlignSelf => 46,
            Self::AlignContent => 47,
            Self::RowGap => 48,
            Self::ColumnGap => 49,
            Self::GridTemplateColumns => 50,
            Self::GridTemplateRows => 51,
            Self::GridTemplateAreas => 52,
            Self::GridAutoFlow => 53,
            Self::GridAutoColumns => 54,
            Self::GridAutoRows => 55,
            Self::GridColumnStart => 56,
            Self::GridColumnEnd => 57,
            Self::GridRowStart => 58,
            Self::GridRowEnd => 59,
            Self::GridArea => 60,
            Self::JustifyItems => 61,
            Self::JustifySelf => 62,
            Self::Opacity => 63,
            Self::BackgroundColor => 64,
            Self::BorderTopColor => 65,
            Self::BorderRightColor => 66,
            Self::BorderBottomColor => 67,
            Self::BorderLeftColor => 68,
            Self::OutlineColor => 69,
            Self::OutlineWidth => 70,
            Self::OutlineOffset => 71,
            Self::CaretColor => 72,
            Self::BorderTopLeftRadius => 73,
            Self::BorderTopRightRadius => 74,
            Self::BorderBottomRightRadius => 75,
            Self::BorderBottomLeftRadius => 76,
            Self::Transform => 77,
            Self::TransitionProperty => 78,
            Self::TransitionDuration => 79,
            Self::TransitionTimingFunction => 80,
            Self::TransitionDelay => 81,
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
    fn count_is_eighty_two_and_all_round_trips() {
        assert_eq!(PresentationProperty::COUNT, 82);
        assert_eq!(PresentationProperty::ALL.len(), 82);
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
            DeclGrammar::NonNegativeLength
        );
        assert_eq!(PresentationProperty::Color.grammar(), DeclGrammar::Color);
        assert_eq!(PresentationProperty::RowGap.grammar(), DeclGrammar::Gap);
        assert_eq!(PresentationProperty::ColumnGap.grammar(), DeclGrammar::Gap);
        assert_eq!(
            PresentationProperty::BorderTopWidth.grammar(),
            DeclGrammar::LineWidth
        );
        assert_eq!(
            PresentationProperty::OutlineWidth.grammar(),
            DeclGrammar::LineWidth
        );
        assert_eq!(
            PresentationProperty::OutlineOffset.grammar(),
            DeclGrammar::SignedLineWidth
        );
        assert_eq!(
            PresentationProperty::BorderTopLeftRadius.grammar(),
            DeclGrammar::LineWidth
        );

        let DeclGrammar::Keywords(align_self_keywords) = PresentationProperty::AlignSelf.grammar()
        else {
            panic!("AlignSelf should use the Keywords grammar");
        };
        assert!(align_self_keywords.contains(&Keyword::Auto));
        assert!(align_self_keywords.contains(&Keyword::Stretch));
        assert!(!align_self_keywords.contains(&Keyword::SpaceBetween));
    }
}
