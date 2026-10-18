//! Public CONTENT attribute vocabulary for `ui-dom`.
//!
//! This file owns the authored attribute identifier enum and its `TryFrom`
//! parsing rules for the CONTENT attribute vocabulary only (PLAN.md approach
//! C: the vocabulary split completes in spine step 3.4). Presentation
//! properties — `width`, `display`, the flex surface, the box model, the font/
//! text properties, and the transition longhands — are no longer members of
//! this enum; they are owned by `PresentationProperty`
//! (`domains/style/presentation_property.rs`) and written through `style_mut`.
//! This enum keeps the DOM content attributes (identity, form state, text,
//! media, and the presented-options control surface), the `Style` seam (the
//! style-attribute write that routes into the inline declaration store), and
//! `Custom` for non-presentation unknown names.
//!
//! Invariants and rules:
//!
//! - this file owns only the `DomAttribute` enum and its `impl` blocks
//! - attribute-name parsing must stay strict and deterministic
//! - presentation names never parse into this vocabulary: the name-based D5
//!   enforcement gate (`name_is_presentation`) intercepts them before
//!   `TryFrom`, and `set_attr` rejects `Custom` presentation names at runtime
//! - app-local styling or scene policy must not leak into the vocabulary

use std::convert::TryFrom;

use serde::{Deserialize, Serialize};

use crate::types::DomError;

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum DomAttribute {
    Id,
    /// The style-attribute write seam: routes into the inline declaration
    /// store, never stored as a content attr.
    Style,
    Class,
    ReadOnly,
    Disabled,
    Required,
    Checked,
    Selected,
    Multiple,
    /// Focusability override; non-negative values are focusable in v1.
    TabIndex,
    Lang,
    Dir,
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
    /// Non-presentation unknown name (the documented content-data escape
    /// hatch). A presentation name in this position is rejected at runtime.
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
            "readonly" => Self::ReadOnly,
            "disabled" => Self::Disabled,
            "required" => Self::Required,
            "checked" => Self::Checked,
            "selected" => Self::Selected,
            "multiple" => Self::Multiple,
            "tab-index" => Self::TabIndex,
            "lang" => Self::Lang,
            "dir" => Self::Dir,
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
    fn dom_attribute_recognizes_content_vocabulary() {
        let expected = [
            ("id", DomAttribute::Id),
            ("style", DomAttribute::Style),
            ("class", DomAttribute::Class),
            ("readonly", DomAttribute::ReadOnly),
            ("disabled", DomAttribute::Disabled),
            ("required", DomAttribute::Required),
            ("checked", DomAttribute::Checked),
            ("selected", DomAttribute::Selected),
            ("multiple", DomAttribute::Multiple),
            ("tab-index", DomAttribute::TabIndex),
            ("lang", DomAttribute::Lang),
            ("dir", DomAttribute::Dir),
            ("src", DomAttribute::Src),
            ("list", DomAttribute::List),
            ("label", DomAttribute::Label),
            ("max", DomAttribute::Max),
            ("maxlength", DomAttribute::MaxLength),
            ("min", DomAttribute::Min),
            ("minlength", DomAttribute::MinLength),
            ("size", DomAttribute::Size),
            ("placeholder", DomAttribute::Placeholder),
            ("presented-options-role", DomAttribute::PresentedOptionsRole),
            (
                "presented-options-state",
                DomAttribute::PresentedOptionsState,
            ),
            (
                "presented-options-highlighted",
                DomAttribute::PresentedOptionsHighlighted,
            ),
            (
                "presented-options-source-index",
                DomAttribute::PresentedOptionsSourceIndex,
            ),
            ("step", DomAttribute::Step),
            ("type", DomAttribute::Type),
            ("value", DomAttribute::Value),
        ];

        for (name, attribute) in expected {
            assert_eq!(
                DomAttribute::try_from(name).expect("content name should parse"),
                attribute
            );
        }
    }

    #[test]
    fn dom_attribute_falls_through_to_custom_for_unknown_names() {
        assert_eq!(
            DomAttribute::try_from("data-role").expect("unknown name should parse"),
            DomAttribute::Custom("data-role".to_string())
        );
        assert_eq!(
            DomAttribute::try_from("slot").expect("unknown name should parse"),
            DomAttribute::Custom("slot".to_string())
        );
    }
}
