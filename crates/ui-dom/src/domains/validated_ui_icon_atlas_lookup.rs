//! Validated icon-atlas lookup table for `ui-dom`.
//!
//! This file owns the retained, dense-indexed lookup produced after validating
//! a committed icon-atlas manifest. Its purpose is to keep runtime icon-atlas
//! sampling keyed by `UiIconId` without re-validating the manifest per sample.
//!
//! Invariants and rules:
//!
//! - this file owns only the `ValidatedUiIconAtlasLookup` struct and its `impl`
//!   blocks
//! - the entry table is immutable after construction and stays `Arc`-backed

use std::sync::Arc;

use ui_icons::UiIconId;

use super::icon_atlas::ValidatedUiIconAtlasEntry;

#[derive(Clone)]
pub(crate) struct ValidatedUiIconAtlasLookup {
    pub(crate) entries: Arc<[ValidatedUiIconAtlasEntry]>,
}

impl ValidatedUiIconAtlasLookup {
    #[must_use]
    pub(crate) fn entry(&self, icon_id: UiIconId) -> &ValidatedUiIconAtlasEntry {
        &self.entries[icon_id.dense_index()]
    }
}

#[cfg(test)]
impl ValidatedUiIconAtlasLookup {
    #[must_use]
    pub(crate) fn from_entries_for_tests(entries: Vec<ValidatedUiIconAtlasEntry>) -> Self {
        Self {
            entries: Arc::<[ValidatedUiIconAtlasEntry]>::from(entries),
        }
    }
}
