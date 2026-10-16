//! Public DOM attribute vocabulary for `ui-dom`.
//!
//! This file owns the authored attribute identifier enum and its `TryFrom`
//! parsing rules. Its purpose is to centralize the attribute-name vocabulary
//! and its strict string parsing so callers can look up attribute behavior
//! without re-deriving name tables in each app.
//!
//! Invariants and rules:
//!
//! - this file owns only the `DomAttribute` enum and its `impl` blocks
//! - attribute-name parsing must stay strict and deterministic
//! - app-local styling or scene policy must not leak into the vocabulary

use std::convert::TryFrom;

use serde::{Deserialize, Serialize};

use crate::types::DomError;

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum DomAttribute {
    Id,
    Style,
    Class,
    Transition,
    TransitionProperty,
    TransitionDuration,
    TransitionTimingFunction,
    TransitionDelay,
    ReadOnly,
    Disabled,
    Required,
    Checked,
    Selected,
    Multiple,
    /// Focusability override; non-negative values are focusable in v1.
    TabIndex,
    Display,
    Overflow,
    Transform,
    Isolate,
    /// Controls whether authored DOM positioning semantics should create a layer.
    Position,
    /// Authored flex-direction attribute; parsed later by flex layout work.
    FlexDirection,
    /// Authored flex-wrap attribute; parsed later by flex layout work.
    FlexWrap,
    /// Authored flex-flow attribute; parsed later by flex layout work.
    FlexFlow,
    /// Authored flex-grow attribute; parsed later by flex layout work.
    FlexGrow,
    /// Authored flex-shrink attribute; parsed later by flex layout work.
    FlexShrink,
    /// Authored flex-basis attribute; parsed later by flex layout work.
    FlexBasis,
    /// Authored flex shorthand attribute; parsed later by flex layout work.
    Flex,
    /// Authored order attribute; parsed later by flex layout work.
    Order,
    /// Authored justify-content attribute; parsed later by flex layout work.
    JustifyContent,
    /// Authored align-items attribute; parsed later by flex layout work.
    AlignItems,
    /// Authored align-self attribute; parsed later by flex layout work.
    AlignSelf,
    /// Authored align-content attribute; parsed later by flex layout work.
    AlignContent,
    /// Authored gap attribute; parsed later by flex layout work.
    Gap,
    /// Authored row-gap attribute; parsed later by flex layout work.
    RowGap,
    /// Authored column-gap attribute; parsed later by flex layout work.
    ColumnGap,
    /// Authored place-content attribute; parsed later by flex layout work.
    PlaceContent,
    /// Authored place-items attribute; parsed later by flex layout work.
    PlaceItems,
    /// Authored place-self attribute; parsed later by flex layout work.
    PlaceSelf,
    /// Authored width attribute; parsed later by layout work.
    Width,
    /// Authored height attribute; parsed later by layout work.
    Height,
    /// Authored minimum width attribute; parsed later by layout work.
    MinWidth,
    /// Authored minimum height attribute; parsed later by layout work.
    MinHeight,
    /// Authored maximum width attribute; parsed later by layout work.
    MaxWidth,
    /// Authored maximum height attribute; parsed later by layout work.
    MaxHeight,
    /// Authored top edge attribute; parsed later by layout work.
    Top,
    /// Authored left edge attribute; parsed later by layout work.
    Left,
    /// Authored right edge attribute; parsed later by layout work.
    Right,
    /// Authored bottom edge attribute; parsed later by layout work.
    Bottom,
    /// Authored top margin attribute; parsed later by layout work.
    MarginTop,
    /// Authored left margin attribute; parsed later by layout work.
    MarginLeft,
    /// Authored right margin attribute; parsed later by layout work.
    MarginRight,
    /// Authored bottom margin attribute; parsed later by layout work.
    MarginBottom,
    /// Authored top padding attribute; parsed later by layout work.
    PaddingTop,
    /// Authored left padding attribute; parsed later by layout work.
    PaddingLeft,
    /// Authored right padding attribute; parsed later by layout work.
    PaddingRight,
    /// Authored bottom padding attribute; parsed later by layout work.
    PaddingBottom,
    /// Authored top border width attribute; parsed later by layout work.
    BorderTop,
    /// Authored left border width attribute; parsed later by layout work.
    BorderLeft,
    /// Authored right border width attribute; parsed later by layout work.
    BorderRight,
    /// Authored bottom border width attribute; parsed later by layout work.
    BorderBottom,
    /// Authored font family attribute; parsed later by style and text work.
    FontFamily,
    /// Authored font weight attribute; parsed later by style and text work.
    FontWeight,
    /// Authored font style attribute; parsed later by style and text work.
    FontStyle,
    /// Authored font size attribute; parsed later by style and text work.
    FontSize,
    /// Authored line-height attribute; parsed later by style and text work.
    LineHeight,
    /// Authored text alignment attribute; parsed later by style and text work.
    TextAlign,
    /// Authored white-space attribute; parsed later by style and text work.
    WhiteSpace,
    /// Authored overflow-wrap attribute; parsed later by style and text work.
    OverflowWrap,
    /// Authored word-break attribute; parsed later by style and text work.
    WordBreak,
    /// Authored language hint attribute; parsed later by text work.
    Lang,
    /// Authored direction attribute; parsed later by text work.
    Dir,
    /// Authored CSS direction property; parsed later by text work.
    Direction,
    Src,
    List,
    Label,
    Max,
    MaxLength,
    Min,
    MinLength,
    Size,
    Placeholder,
    PresentedOptionsRole,
    PresentedOptionsState,
    PresentedOptionsHighlighted,
    PresentedOptionsSourceIndex,
    Step,
    Type,
    Value,
    Custom(String),
}

impl TryFrom<&str> for DomAttribute {
    type Error = DomError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if value.is_empty() {
            return Err(DomError::InvalidAttributeValue);
        }
        Ok(match value {
            "id" => Self::Id,
            "style" => Self::Style,
            "class" => Self::Class,
            "transition" => Self::Transition,
            "transition-property" => Self::TransitionProperty,
            "transition-duration" => Self::TransitionDuration,
            "transition-timing-function" => Self::TransitionTimingFunction,
            "transition-delay" => Self::TransitionDelay,
            "readonly" => Self::ReadOnly,
            "disabled" => Self::Disabled,
            "required" => Self::Required,
            "checked" => Self::Checked,
            "selected" => Self::Selected,
            "multiple" => Self::Multiple,
            "tab-index" => Self::TabIndex,
            "display" => Self::Display,
            "overflow" => Self::Overflow,
            "transform" => Self::Transform,
            "isolate" => Self::Isolate,
            "position" => Self::Position,
            "flex-direction" => Self::FlexDirection,
            "flex-wrap" => Self::FlexWrap,
            "flex-flow" => Self::FlexFlow,
            "flex-grow" => Self::FlexGrow,
            "flex-shrink" => Self::FlexShrink,
            "flex-basis" => Self::FlexBasis,
            "flex" => Self::Flex,
            "order" => Self::Order,
            "justify-content" => Self::JustifyContent,
            "align-items" => Self::AlignItems,
            "align-self" => Self::AlignSelf,
            "align-content" => Self::AlignContent,
            "gap" => Self::Gap,
            "row-gap" => Self::RowGap,
            "column-gap" => Self::ColumnGap,
            "place-content" => Self::PlaceContent,
            "place-items" => Self::PlaceItems,
            "place-self" => Self::PlaceSelf,
            "width" => Self::Width,
            "height" => Self::Height,
            "min-width" => Self::MinWidth,
            "min-height" => Self::MinHeight,
            "max-width" => Self::MaxWidth,
            "max-height" => Self::MaxHeight,
            "top" => Self::Top,
            "left" => Self::Left,
            "right" => Self::Right,
            "bottom" => Self::Bottom,
            "margin-top" => Self::MarginTop,
            "margin-left" => Self::MarginLeft,
            "margin-right" => Self::MarginRight,
            "margin-bottom" => Self::MarginBottom,
            "padding-top" => Self::PaddingTop,
            "padding-left" => Self::PaddingLeft,
            "padding-right" => Self::PaddingRight,
            "padding-bottom" => Self::PaddingBottom,
            "border-top" => Self::BorderTop,
            "border-left" => Self::BorderLeft,
            "border-right" => Self::BorderRight,
            "border-bottom" => Self::BorderBottom,
            "font-family" => Self::FontFamily,
            "font-weight" => Self::FontWeight,
            "font-style" => Self::FontStyle,
            "font-size" => Self::FontSize,
            "line-height" => Self::LineHeight,
            "text-align" => Self::TextAlign,
            "white-space" => Self::WhiteSpace,
            "overflow-wrap" => Self::OverflowWrap,
            "word-break" => Self::WordBreak,
            "lang" => Self::Lang,
            "dir" => Self::Dir,
            "direction" => Self::Direction,
            "src" => Self::Src,
            "list" => Self::List,
            "label" => Self::Label,
            "max" => Self::Max,
            "maxlength" => Self::MaxLength,
            "min" => Self::Min,
            "minlength" => Self::MinLength,
            "size" => Self::Size,
            "placeholder" => Self::Placeholder,
            "presented-options-role" => Self::PresentedOptionsRole,
            "presented-options-state" => Self::PresentedOptionsState,
            "presented-options-highlighted" => Self::PresentedOptionsHighlighted,
            "presented-options-source-index" => Self::PresentedOptionsSourceIndex,
            "step" => Self::Step,
            "type" => Self::Type,
            "value" => Self::Value,
            custom => Self::Custom(custom.to_owned()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dom_attribute_recognizes_new_text_and_form_names() {
        assert_eq!(
            DomAttribute::try_from("readonly").expect("readonly should parse"),
            DomAttribute::ReadOnly
        );
        assert_eq!(
            DomAttribute::try_from("font-family").expect("font-family should parse"),
            DomAttribute::FontFamily
        );
        assert_eq!(
            DomAttribute::try_from("font-weight").expect("font-weight should parse"),
            DomAttribute::FontWeight
        );
        assert_eq!(
            DomAttribute::try_from("font-style").expect("font-style should parse"),
            DomAttribute::FontStyle
        );
        assert_eq!(
            DomAttribute::try_from("line-height").expect("line-height should parse"),
            DomAttribute::LineHeight
        );
        assert_eq!(
            DomAttribute::try_from("text-align").expect("text-align should parse"),
            DomAttribute::TextAlign
        );
        assert_eq!(
            DomAttribute::try_from("white-space").expect("white-space should parse"),
            DomAttribute::WhiteSpace
        );
        assert_eq!(
            DomAttribute::try_from("overflow-wrap").expect("overflow-wrap should parse"),
            DomAttribute::OverflowWrap
        );
        assert_eq!(
            DomAttribute::try_from("word-break").expect("word-break should parse"),
            DomAttribute::WordBreak
        );
        assert_eq!(
            DomAttribute::try_from("lang").expect("lang should parse"),
            DomAttribute::Lang
        );
        assert_eq!(
            DomAttribute::try_from("dir").expect("dir should parse"),
            DomAttribute::Dir
        );
        assert_eq!(
            DomAttribute::try_from("direction").expect("direction should parse"),
            DomAttribute::Direction
        );
        assert_eq!(
            DomAttribute::try_from("maxlength").expect("maxlength should parse"),
            DomAttribute::MaxLength
        );
        assert_eq!(
            DomAttribute::try_from("placeholder").expect("placeholder should parse"),
            DomAttribute::Placeholder
        );
    }

    #[test]
    fn dom_attribute_recognizes_flex_surface_names() {
        let expected = [
            ("flex-direction", DomAttribute::FlexDirection),
            ("flex-wrap", DomAttribute::FlexWrap),
            ("flex-flow", DomAttribute::FlexFlow),
            ("flex-grow", DomAttribute::FlexGrow),
            ("flex-shrink", DomAttribute::FlexShrink),
            ("flex-basis", DomAttribute::FlexBasis),
            ("flex", DomAttribute::Flex),
            ("order", DomAttribute::Order),
            ("justify-content", DomAttribute::JustifyContent),
            ("align-items", DomAttribute::AlignItems),
            ("align-self", DomAttribute::AlignSelf),
            ("align-content", DomAttribute::AlignContent),
            ("gap", DomAttribute::Gap),
            ("row-gap", DomAttribute::RowGap),
            ("column-gap", DomAttribute::ColumnGap),
            ("place-content", DomAttribute::PlaceContent),
            ("place-items", DomAttribute::PlaceItems),
            ("place-self", DomAttribute::PlaceSelf),
        ];

        for (name, attribute) in expected {
            assert_eq!(
                DomAttribute::try_from(name).expect("flex surface name should parse"),
                attribute
            );
        }
    }

    #[test]
    fn dom_attribute_recognizes_transition_longhand_names() {
        let expected = [
            ("transition-property", DomAttribute::TransitionProperty),
            ("transition-duration", DomAttribute::TransitionDuration),
            (
                "transition-timing-function",
                DomAttribute::TransitionTimingFunction,
            ),
            ("transition-delay", DomAttribute::TransitionDelay),
        ];

        for (name, attribute) in expected {
            assert_eq!(
                DomAttribute::try_from(name).expect("transition longhand should parse"),
                attribute
            );
        }
    }
}
