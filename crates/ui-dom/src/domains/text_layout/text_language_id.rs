//! Text language hint for `ui-dom`.
//!
//! This file owns the `TextLanguageId` enum and the locale detection that maps
//! a text sample to the script hint used for font selection and line-breaking
//! policy. It stays free of layout and buffer construction.
//!
//! Invariants and rules:
//!
//! - this file owns only the `TextLanguageId` enum and its `impl` block
//! - detection is script-range based and ordered: Thai, Korean, Japanese,
//!   Simplified Chinese, then English fallback

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum TextLanguageId {
    English,
    Japanese,
    SimplifiedChinese,
    Korean,
    Thai,
}

impl TextLanguageId {
    #[must_use]
    pub(crate) fn from_text_hint(text: &str) -> Self {
        if text
            .chars()
            .any(|character| ('\u{0e00}'..='\u{0e7f}').contains(&character))
        {
            return Self::Thai;
        }
        if text
            .chars()
            .any(|character| ('\u{ac00}'..='\u{d7af}').contains(&character))
        {
            return Self::Korean;
        }
        if text
            .chars()
            .any(|character| ('\u{3040}'..='\u{30ff}').contains(&character))
        {
            return Self::Japanese;
        }
        if text
            .chars()
            .any(|character| ('\u{4e00}'..='\u{9fff}').contains(&character))
        {
            return Self::SimplifiedChinese;
        }
        Self::English
    }
}
