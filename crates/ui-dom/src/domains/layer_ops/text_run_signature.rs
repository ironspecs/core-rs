//! Retained text-run identity signature for `ui-dom`.
//!
//! This file owns the identity key that banks text runs by content, language
//! hint, and font family. Its purpose is to keep text-run deduplication explicit
//! and hashable without owning text rasterization or layer-pool policy.
//!
//! Invariants and rules:
//!
//! - this file owns only the `TextRunSignature` struct and its `impl` block
//! - the signature is `Eq + Hash` so it can key the retained text-run cache

use std::sync::Arc;

use crate::domains::text_layout::{TextFontFamilyId, TextLanguageId};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct TextRunSignature {
    pub(crate) text: Arc<str>,
    pub(crate) language_hint_id: TextLanguageId,
    pub(crate) font_family_id: TextFontFamilyId,
}

impl TextRunSignature {
    #[must_use]
    pub(crate) fn from_text(text: Arc<str>) -> Self {
        Self {
            language_hint_id: TextLanguageId::from_text_hint(text.as_ref()),
            font_family_id: TextFontFamilyId::UiSans,
            text,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_text_detects_language_hints_and_ui_sans_family() {
        let settings = TextRunSignature::from_text("Settings".into());
        assert_eq!(settings.language_hint_id, TextLanguageId::English);
        assert_eq!(settings.font_family_id, TextFontFamilyId::UiSans);
        assert_eq!(settings.text.as_ref(), "Settings");

        assert_eq!(
            TextRunSignature::from_text("メニュー".into()).language_hint_id,
            TextLanguageId::Japanese
        );
        assert_eq!(
            TextRunSignature::from_text("메뉴".into()).language_hint_id,
            TextLanguageId::Korean
        );
        assert_eq!(
            TextRunSignature::from_text("เมนู".into()).language_hint_id,
            TextLanguageId::Thai
        );
        assert_eq!(
            TextRunSignature::from_text("颜色".into()).language_hint_id,
            TextLanguageId::SimplifiedChinese
        );
    }
}
