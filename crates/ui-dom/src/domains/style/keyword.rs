//! Closed CSS keyword vocabulary for the `ui-dom` style system.
//!
//! This file owns the `Keyword` enum and its canonical name mapping. The
//! vocabulary is closed: it holds exactly the property keywords the
//! presentation registry admits, each serialized to its canonical kebab-case
//! name. No parsing or property-specific policy lives here; admission sets are
//! declared in `presentation_property.rs` and passed in via `from_name_in`.
//!
//! Invariants and rules:
//!
//! - every variant has exactly one canonical name; `name` is a total match
//!   with no fallback arm and no allocation
//! - `from_name_in` is a linear scan over a caller-supplied admission set and
//!   never accepts a name outside that set

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Keyword {
    Block,
    Inline,
    InlineBlock,
    None,
    Flex,
    InlineFlex,
    Static,
    Relative,
    Absolute,
    Fixed,
    Visible,
    Scroll,
    Auto,
    Hidden,
    Clip,
    Isolate,
    Start,
    Left,
    Center,
    Right,
    Justified,
    End,
    Ltr,
    Rtl,
    Normal,
    Nowrap,
    Pre,
    PreWrap,
    PreLine,
    BreakSpaces,
    BreakWord,
    Anywhere,
    BreakAll,
    KeepAll,
    Italic,
    Bold,
    Bolder,
    Lighter,
    Row,
    RowReverse,
    Column,
    ColumnReverse,
    Wrap,
    WrapReverse,
    FlexStart,
    FlexEnd,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
    Stretch,
    Content,
    Linear,
    Ease,
    EaseIn,
    EaseOut,
    EaseInOut,
    All,
}

impl Keyword {
    /// The canonical kebab-case serialization.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Keyword::Block => "block",
            Keyword::Inline => "inline",
            Keyword::InlineBlock => "inline-block",
            Keyword::None => "none",
            Keyword::Flex => "flex",
            Keyword::InlineFlex => "inline-flex",
            Keyword::Static => "static",
            Keyword::Relative => "relative",
            Keyword::Absolute => "absolute",
            Keyword::Fixed => "fixed",
            Keyword::Visible => "visible",
            Keyword::Scroll => "scroll",
            Keyword::Auto => "auto",
            Keyword::Hidden => "hidden",
            Keyword::Clip => "clip",
            Keyword::Isolate => "isolate",
            Keyword::Start => "start",
            Keyword::Left => "left",
            Keyword::Center => "center",
            Keyword::Right => "right",
            Keyword::Justified => "justify",
            Keyword::End => "end",
            Keyword::Ltr => "ltr",
            Keyword::Rtl => "rtl",
            Keyword::Normal => "normal",
            Keyword::Nowrap => "nowrap",
            Keyword::Pre => "pre",
            Keyword::PreWrap => "pre-wrap",
            Keyword::PreLine => "pre-line",
            Keyword::BreakSpaces => "break-spaces",
            Keyword::BreakWord => "break-word",
            Keyword::Anywhere => "anywhere",
            Keyword::BreakAll => "break-all",
            Keyword::KeepAll => "keep-all",
            Keyword::Italic => "italic",
            Keyword::Bold => "bold",
            Keyword::Bolder => "bolder",
            Keyword::Lighter => "lighter",
            Keyword::Row => "row",
            Keyword::RowReverse => "row-reverse",
            Keyword::Column => "column",
            Keyword::ColumnReverse => "column-reverse",
            Keyword::Wrap => "wrap",
            Keyword::WrapReverse => "wrap-reverse",
            Keyword::FlexStart => "flex-start",
            Keyword::FlexEnd => "flex-end",
            Keyword::SpaceBetween => "space-between",
            Keyword::SpaceAround => "space-around",
            Keyword::SpaceEvenly => "space-evenly",
            Keyword::Stretch => "stretch",
            Keyword::Content => "content",
            Keyword::Linear => "linear",
            Keyword::Ease => "ease",
            Keyword::EaseIn => "ease-in",
            Keyword::EaseOut => "ease-out",
            Keyword::EaseInOut => "ease-in-out",
            Keyword::All => "all",
        }
    }

    /// Looks up a keyword by canonical name within one property's admitted set.
    #[must_use]
    pub fn from_name_in(set: &'static [Keyword], name: &str) -> Option<Keyword> {
        set.iter().copied().find(|keyword| keyword.name() == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_KEYWORDS: &[Keyword] = &[
        Keyword::Block,
        Keyword::Inline,
        Keyword::InlineBlock,
        Keyword::None,
        Keyword::Flex,
        Keyword::InlineFlex,
        Keyword::Static,
        Keyword::Relative,
        Keyword::Absolute,
        Keyword::Fixed,
        Keyword::Visible,
        Keyword::Scroll,
        Keyword::Auto,
        Keyword::Hidden,
        Keyword::Clip,
        Keyword::Isolate,
        Keyword::Start,
        Keyword::Left,
        Keyword::Center,
        Keyword::Right,
        Keyword::Justified,
        Keyword::End,
        Keyword::Ltr,
        Keyword::Rtl,
        Keyword::Normal,
        Keyword::Nowrap,
        Keyword::Pre,
        Keyword::PreWrap,
        Keyword::PreLine,
        Keyword::BreakSpaces,
        Keyword::BreakWord,
        Keyword::Anywhere,
        Keyword::BreakAll,
        Keyword::KeepAll,
        Keyword::Italic,
        Keyword::Bold,
        Keyword::Bolder,
        Keyword::Lighter,
        Keyword::Row,
        Keyword::RowReverse,
        Keyword::Column,
        Keyword::ColumnReverse,
        Keyword::Wrap,
        Keyword::WrapReverse,
        Keyword::FlexStart,
        Keyword::FlexEnd,
        Keyword::SpaceBetween,
        Keyword::SpaceAround,
        Keyword::SpaceEvenly,
        Keyword::Stretch,
        Keyword::Content,
        Keyword::Linear,
        Keyword::Ease,
        Keyword::EaseIn,
        Keyword::EaseOut,
        Keyword::EaseInOut,
        Keyword::All,
    ];

    #[test]
    fn name_round_trips_for_every_keyword() {
        for keyword in ALL_KEYWORDS {
            assert_eq!(
                Keyword::from_name_in(ALL_KEYWORDS, keyword.name()),
                Some(*keyword)
            );
        }
    }

    #[test]
    fn justified_uses_the_crate_canonical_name() {
        assert_eq!(Keyword::Justified.name(), "justify");
    }

    #[test]
    fn from_name_in_only_accepts_admitted_members() {
        let set: &[Keyword] = &[Keyword::Auto, Keyword::Isolate];
        assert_eq!(Keyword::from_name_in(set, "auto"), Some(Keyword::Auto));
        assert_eq!(
            Keyword::from_name_in(set, "isolate"),
            Some(Keyword::Isolate)
        );
        assert_eq!(Keyword::from_name_in(set, "hidden"), None);
        assert_eq!(Keyword::from_name_in(set, "nonsense"), None);
    }
}
