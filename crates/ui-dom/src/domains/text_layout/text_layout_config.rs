//! Text layout configuration for `ui-dom`.
//!
//! This file owns the `TextLayoutConfig` value that selects the font catalog a
//! text layout context is built from. It is a plain data carrier with a single
//! repo-default constructor.
//!
//! Invariants and rules:
//!
//! - this file owns only the `TextLayoutConfig` struct and its `impl` block
//! - `repo_default` builds the shared repo font catalog

use super::TextFontCatalog;

#[derive(Clone, Debug)]
pub(crate) struct TextLayoutConfig {
    pub(super) font_catalog: TextFontCatalog,
}

impl TextLayoutConfig {
    #[must_use]
    pub(crate) fn repo_default() -> Self {
        Self {
            font_catalog: TextFontCatalog::repo_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_default_config_provides_a_populated_font_catalog() {
        let config = TextLayoutConfig::repo_default();
        assert!(!config.font_catalog.ui_sans_font_sources.is_empty());
    }
}
