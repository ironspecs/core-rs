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

/// The script-range language hint the fallback policy table keys script
/// coverage on (text seam S4). It is a closed set: the five scripts the
/// embedded vendor faces cover (or, for Thai, deliberately do not).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TextLanguageId {
    /// English / Latin text.
    English,
    /// Japanese kana/hiragana/katakana text.
    Japanese,
    /// Simplified Chinese Han text.
    SimplifiedChinese,
    /// Korean Hangul text.
    Korean,
    /// Thai text (no embedded face covers it; the `NoCoverage` path).
    Thai,
}

impl TextLanguageId {
    /// Derives the language hint from a text sample by script-range scan
    /// (ordered: Thai, Korean, Japanese, Simplified Chinese, then English).
    #[must_use]
    pub fn from_text_hint(text: &str) -> Self {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_text_hint_detects_each_supported_language() {
        assert_eq!(
            TextLanguageId::from_text_hint("Settings"),
            TextLanguageId::English
        );
        assert_eq!(
            TextLanguageId::from_text_hint("メニュー"),
            TextLanguageId::Japanese
        );
        assert_eq!(
            TextLanguageId::from_text_hint("메뉴"),
            TextLanguageId::Korean
        );
        assert_eq!(TextLanguageId::from_text_hint("เมนู"), TextLanguageId::Thai);
        assert_eq!(
            TextLanguageId::from_text_hint("颜色"),
            TextLanguageId::SimplifiedChinese
        );
        // Mixed ASCII + kana resolves Japanese: kana (U+3040..U+30FF) is
        // checked before Han (U+4E00..U+9FFF), and ASCII has no script range.
        assert_eq!(
            TextLanguageId::from_text_hint("メニュー Settings"),
            TextLanguageId::Japanese
        );
    }
}
