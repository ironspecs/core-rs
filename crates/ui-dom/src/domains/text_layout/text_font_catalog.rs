//! The shared text font catalog and the consumer-install door for `ui-dom`.
//!
//! This file owns `TextFontCatalog`, the single source of truth for "which
//! faces exist" (text seam S3, `TEXT_SEAM_DESIGN.md` D4). It retains the four
//! embedded vendor faces plus any consumer-installed faces, exposes the public
//! `install_face` door, and carries the revision counter the main-world context
//! checks at seam entry.
//!
//! Invariants and rules:
//!
//! - this file owns `TextFontCatalog`, `FaceSource`, and the embedded vendor
//!   font bytes it loads; `FontFaceId` and the report types live in their own
//!   files
//! - the catalog is context-scoped and shared: a `TextFontCatalog` clone wraps
//!   one `Arc` of shared state, so `install_face` on any clone is visible to
//!   every other clone (the main-world thread-local context and the render
//!   world both read the same state)
//! - `install_face` parses boundary bytes with fontdb and NEVER panics on them:
//!   a malformed face is a `Rejected` report
//! - every mutation bumps `revision`; the context re-installs only the newly
//!   appended sources when the revision changes (never re-loads embedded faces)
//! - the four embedded faces own the fixed ids `0..4`; consumer faces receive
//!   freshly allocated ids

use std::sync::{Arc, OnceLock};

use arrayvec::ArrayVec;
use cosmic_text::fontdb::{Database, Source as FontDbSource};
use parking_lot::RwLock;

use super::face_source::FaceSource;
use super::{FontFaceId, FontInstallReport, FontRejectReason};

const IBM_PLEX_SANS_BYTES: &[u8] =
    include_bytes!("../../../../../assets/fonts/vendor/ibm-plex/IBMPlexSans-Variable.ttf");
const SOURCE_HAN_SANS_J_BYTES: &[u8] =
    include_bytes!("../../../../../assets/fonts/vendor/source-han-sans/SourceHanSansJ-Regular.otf");
const SOURCE_HAN_SANS_SC_BYTES: &[u8] = include_bytes!(
    "../../../../../assets/fonts/vendor/source-han-sans/SourceHanSansSC-Regular.otf"
);
const SOURCE_HAN_SANS_K_BYTES: &[u8] =
    include_bytes!("../../../../../assets/fonts/vendor/source-han-sans/SourceHanSansK-Regular.otf");

/// The four embedded faces' family names, in catalog order. This is the source
/// of truth for the embedded set (text seam S3); the fallback table (S4) and
/// the render world resolve against these plus the consumer faces' names.
pub(crate) const IBM_PLEX_SANS_FAMILY: &str = "IBM Plex Sans";
pub(crate) const SOURCE_HAN_SANS_FAMILY: &str = "Source Han Sans";
pub(crate) const SOURCE_HAN_SANS_SC_FAMILY: &str = "Source Han Sans SC";
pub(crate) const SOURCE_HAN_SANS_K_FAMILY: &str = "Source Han Sans K";

/// The embedded family names, index-aligned with the embedded sources.
pub(crate) const EMBEDDED_FAMILY_NAMES: [&str; 4] = [
    IBM_PLEX_SANS_FAMILY,
    SOURCE_HAN_SANS_FAMILY,
    SOURCE_HAN_SANS_SC_FAMILY,
    SOURCE_HAN_SANS_K_FAMILY,
];

/// The embedded face sources, in catalog order (the initial `sources` list).
pub(crate) const EMBEDDED_SOURCES: [&[u8]; 4] = [
    IBM_PLEX_SANS_BYTES,
    SOURCE_HAN_SANS_J_BYTES,
    SOURCE_HAN_SANS_SC_BYTES,
    SOURCE_HAN_SANS_K_BYTES,
];

static REPO_TEXT_FONT_CATALOG: OnceLock<TextFontCatalog> = OnceLock::new();

/// The shared state behind a `TextFontCatalog` clone.
#[derive(Debug)]
struct CatalogState {
    sources: Vec<FaceSource>,
    /// Primary-then-alias family names per face, index-aligned with `sources`.
    family_names: Vec<ArrayVec<String, 4>>,
    revision: u64,
    next_face_id: u32,
}

/// The shared, context-scoped text font catalog.
#[derive(Clone, Debug)]
pub struct TextFontCatalog {
    inner: Arc<RwLock<CatalogState>>,
}

impl TextFontCatalog {
    /// The shared repo-default catalog: the four embedded faces, available to
    /// every document and both `FontSystem`s through one shared state.
    #[must_use]
    pub fn repo_default() -> Self {
        REPO_TEXT_FONT_CATALOG
            .get_or_init(Self::from_embedded_sources)
            .clone()
    }

    /// Installs one consumer-supplied face (bytes parsed by fontdb at install).
    ///
    /// A malformed buffer is a `Rejected` report, never a panic (boundary
    /// data). On success the face is appended to the catalog, receives a fresh
    /// [`FontFaceId`], and the catalog revision is bumped so the main-world
    /// context re-installs at its next seam entry.
    #[must_use]
    pub fn install_face(&self, bytes: &[u8]) -> FontInstallReport {
        let Some(family_names) = parse_family_names(bytes) else {
            return FontInstallReport::Rejected(FontRejectReason::NotAFont);
        };
        let mut state = self.inner.write();
        let id = FontFaceId::from_raw(state.next_face_id);
        state.next_face_id = state
            .next_face_id
            .checked_add(1)
            .expect("consumer face id should not overflow");
        state.sources.push(FaceSource::ConsumerInstalled {
            bytes: Arc::new(bytes.to_vec()),
            id,
        });
        state.family_names.push(family_names.clone());
        state.revision = state
            .revision
            .checked_add(1)
            .expect("catalog revision should not overflow");
        FontInstallReport::Installed { id, family_names }
    }

    /// The catalog revision; bumps on every successful `install_face`. The
    /// main-world context compares this at seam entry to decide whether to
    /// install newly appended sources.
    #[must_use]
    pub(crate) fn revision(&self) -> u64 {
        self.inner.read().revision
    }

    /// A snapshot of the current sources (embedded + consumer), in catalog
    /// order. Allocation happens here, on the rare revision-change path only.
    #[must_use]
    pub(crate) fn sources_snapshot(&self) -> Vec<FaceSource> {
        self.inner.read().sources.clone()
    }

    /// Resolves one requested family name to the installed face it routes to
    /// (ASCII case-insensitive, CSS Fonts 4 §3.1). Returns the face id and the
    /// face's primary family name. This is the seam's rule-1 resolution, made
    /// public for proofs and consumers (S4 generalizes the fallback table).
    #[must_use]
    pub fn resolve_family(&self, name: &str) -> Option<(FontFaceId, String)> {
        let state = self.inner.read();
        installed_face_index(&state, name).map(|index| {
            let face_id = state.sources[index].face_id(index);
            let primary = state.family_names[index]
                .first()
                .cloned()
                .unwrap_or_else(|| name.to_owned());
            (face_id, primary)
        })
    }

    /// The installed faces' ids, in catalog order (embedded then consumer).
    /// The render world's source list enumerates this same set.
    #[must_use]
    pub fn installed_face_ids(&self) -> Vec<FontFaceId> {
        let state = self.inner.read();
        state
            .sources
            .iter()
            .enumerate()
            .map(|(index, source)| source.face_id(index))
            .collect()
    }

    /// The consumer-installed faces' ids and bytes, in install order. The
    /// render world's extract reconciles these into its `FontSystem` (the
    /// render-world side of the consumer door).
    #[must_use]
    pub(crate) fn consumer_faces(&self) -> Vec<(FontFaceId, Arc<Vec<u8>>)> {
        let state = self.inner.read();
        state
            .sources
            .iter()
            .filter_map(|source| match source {
                FaceSource::ConsumerInstalled { bytes, id } => Some((*id, Arc::clone(bytes))),
                FaceSource::Embedded(_) => None,
            })
            .collect()
    }

    /// Resolves one requested family name to the installed face, returning the
    /// shaping-ready owned family. This is the seam's hot-path resolution; the
    /// owned family is a small inline string for typical names (no allocation).
    #[must_use]
    pub(crate) fn resolve_face(
        &self,
        name: &str,
    ) -> Option<(FontFaceId, cosmic_text::FamilyOwned)> {
        let state = self.inner.read();
        installed_face_index(&state, name).map(|index| {
            let face_id = state.sources[index].face_id(index);
            let primary = state.family_names[index]
                .first()
                .map_or(name, String::as_str);
            (
                face_id,
                cosmic_text::FamilyOwned::new(cosmic_text::Family::Name(primary)),
            )
        })
    }

    /// Builds the catalog from the four embedded sources (the initial list).
    fn from_embedded_sources() -> Self {
        let sources = EMBEDDED_SOURCES
            .iter()
            .map(|bytes| FaceSource::Embedded(bytes))
            .collect();
        let family_names = EMBEDDED_FAMILY_NAMES
            .iter()
            .map(|name| {
                let mut names = ArrayVec::<String, 4>::new();
                names.push((*name).to_owned());
                names
            })
            .collect();
        Self {
            inner: Arc::new(RwLock::new(CatalogState {
                sources,
                family_names,
                revision: 0,
                next_face_id: FontFaceId::EMBEDDED_FACE_COUNT,
            })),
        }
    }
}

/// The index of the first face whose family names contain `name`
/// (ASCII case-insensitive); `None` when the name is not installed.
fn installed_face_index(state: &CatalogState, name: &str) -> Option<usize> {
    state.family_names.iter().position(|names| {
        names
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(name))
    })
}

/// Parses a font buffer with fontdb and reads up to four distinct family names
/// from the face's name table. `None` when the bytes are not a usable font.
fn parse_family_names(bytes: &[u8]) -> Option<ArrayVec<String, 4>> {
    let mut database = Database::new();
    database.load_font_source(FontDbSource::Binary(Arc::new(bytes.to_vec())));
    let face = database.faces().next()?;
    let mut names = ArrayVec::<String, 4>::new();
    for (name, _language) in &face.families {
        if names.iter().any(|existing| existing == name) {
            continue;
        }
        if names.len() == names.capacity() {
            break;
        }
        names.push(name.clone());
    }
    Some(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_catalog() -> TextFontCatalog {
        TextFontCatalog::from_embedded_sources()
    }

    #[test]
    fn repo_default_is_a_shared_singleton() {
        let catalog = TextFontCatalog::repo_default();
        let again = TextFontCatalog::repo_default();
        assert!(Arc::ptr_eq(&catalog.inner, &again.inner));
    }

    #[test]
    fn fresh_catalog_holds_the_four_embedded_faces() {
        let catalog = fresh_catalog();
        assert_eq!(catalog.installed_face_ids().len(), 4);
        assert_eq!(catalog.revision(), 0);
    }

    #[test]
    fn resolve_family_routes_embedded_names_case_insensitively() {
        let catalog = fresh_catalog();
        let (face_id, family) = catalog
            .resolve_family("ibm plex sans")
            .expect("embedded sans should resolve");
        assert_eq!(face_id, FontFaceId::EMBEDDED_IBM_PLEX_SANS);
        assert_eq!(family, "IBM Plex Sans");

        let (face_id, _) = catalog
            .resolve_family("source han sans sc")
            .expect("embedded SC should resolve");
        assert_eq!(face_id, FontFaceId::EMBEDDED_SOURCE_HAN_SANS_SC);

        assert!(catalog.resolve_family("no such face").is_none());
    }

    #[test]
    fn install_face_rejects_garbage_without_panicking() {
        let catalog = fresh_catalog();
        let report = catalog.install_face(b"this is not a font");
        assert_eq!(
            report,
            FontInstallReport::Rejected(FontRejectReason::NotAFont),
        );
        assert_eq!(catalog.installed_face_ids().len(), 4);
    }

    #[test]
    fn install_face_appends_a_consumer_face_and_bumps_the_revision() {
        let catalog = fresh_catalog();
        let source_bytes = EMBEDDED_SOURCES[0];
        let before = catalog.revision();
        let report = catalog.install_face(source_bytes);

        let FontInstallReport::Installed { id, family_names } = report else {
            panic!("embedded sans bytes should install as a valid face");
        };
        assert_eq!(id, FontFaceId::from_raw(FontFaceId::EMBEDDED_FACE_COUNT));
        assert!(
            family_names
                .iter()
                .any(|name| name.eq_ignore_ascii_case("IBM Plex Sans")),
            "family names should be read from the name table"
        );
        assert_eq!(catalog.revision(), before + 1);
        assert_eq!(catalog.installed_face_ids().len(), 5);
        assert!(catalog.installed_face_ids().contains(&id));
    }
}
