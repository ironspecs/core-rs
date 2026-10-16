//! Text font catalog for `ui-dom`.
//!
//! This file owns the `TextFontCatalog` value that retains the embedded font
//! binary sources installed into a text layout `FontSystem`. It also owns the
//! shared repo catalog singleton and the embedded vendor font bytes it loads.
//!
//! Invariants and rules:
//!
//! - this file owns only the `TextFontCatalog` struct and its `impl` block
//! - the repo catalog is a process-wide `OnceLock` initialized once from the
//!   embedded vendor font bytes

use std::sync::{Arc, OnceLock};

const IBM_PLEX_SANS_BYTES: &[u8] =
    include_bytes!("../../../../../assets/fonts/vendor/ibm-plex/IBMPlexSans-Variable.ttf");
const SOURCE_HAN_SANS_J_BYTES: &[u8] =
    include_bytes!("../../../../../assets/fonts/vendor/source-han-sans/SourceHanSansJ-Regular.otf");
const SOURCE_HAN_SANS_SC_BYTES: &[u8] = include_bytes!(
    "../../../../../assets/fonts/vendor/source-han-sans/SourceHanSansSC-Regular.otf"
);
const SOURCE_HAN_SANS_K_BYTES: &[u8] =
    include_bytes!("../../../../../assets/fonts/vendor/source-han-sans/SourceHanSansK-Regular.otf");

static REPO_TEXT_FONT_CATALOG: OnceLock<TextFontCatalog> = OnceLock::new();

#[derive(Clone, Debug)]
pub(crate) struct TextFontCatalog {
    pub(super) ui_sans_font_sources: Vec<Arc<Vec<u8>>>,
}

impl TextFontCatalog {
    #[must_use]
    pub(crate) fn repo_default() -> Self {
        REPO_TEXT_FONT_CATALOG
            .get_or_init(|| Self {
                ui_sans_font_sources: vec![
                    Arc::new(IBM_PLEX_SANS_BYTES.to_vec()),
                    Arc::new(SOURCE_HAN_SANS_J_BYTES.to_vec()),
                    Arc::new(SOURCE_HAN_SANS_SC_BYTES.to_vec()),
                    Arc::new(SOURCE_HAN_SANS_K_BYTES.to_vec()),
                ],
            })
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_default_catalog_is_a_populated_singleton() {
        let catalog = TextFontCatalog::repo_default();
        assert!(!catalog.ui_sans_font_sources.is_empty());

        let again = TextFontCatalog::repo_default();
        assert_eq!(
            catalog.ui_sans_font_sources.len(),
            again.ui_sans_font_sources.len()
        );
        assert_eq!(
            catalog.ui_sans_font_sources[0].len(),
            again.ui_sans_font_sources[0].len()
        );
    }
}
