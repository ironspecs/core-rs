//! The font fallback policy table for `ui-dom` (text seam S4,
//! `TEXT_SEAM_DESIGN.md` D5).
//!
//! This file owns the WHOLE family-resolution policy — the only place a text
//! request's family is resolved. It replaces the pre-S4 resolution that lived
//! in `text_render.rs` (`resolve_requested_family`, `resolve_face_id`,
//! `preferred_repo_font_face`) plus the Thai wrap override, and it NAMES the
//! glyph-id-0 debug-rect demotion whose observability is the S4 counter.
//!
//! The four rules, in order:
//!
//! 1. **Requested names** — each named family in the request, ASCII
//!    case-insensitive, first installed match wins → `ResolutionPath::
//!    RequestedName`.
//! 2. **Generic mapping** — the first generic keyword maps through the static
//!    table (every generic routes to the guaranteed-final face) →
//!    `ResolutionPath::GenericMapped`. The vendor directory holds real Plex
//!    Mono faces but they are NOT embedded; a consumer wanting real monospace
//!    installs one via the S3 door (registry note).
//! 3. **Script coverage** — J/SC/K text routes to the Source Han faces,
//!    English/Latin to IBM Plex Sans (absorbs the pre-S4 `preferred_repo_font_face`
//!    language routing) → `ResolutionPath::ScriptCoverage`.
//! 4. **No-face outcome** — Thai has no installed face that covers it, so
//!    resolution still returns the guaranteed-final face (IBM Plex Sans is
//!    ALWAYS installed — the invariant `fallback_final_face_is_always_installed`
//!    asserts) and records `ResolutionPath::NoCoverage`.
//!
//! Invariants and rules:
//!
//! - this file owns `resolve`, `resolve_face_id`, the Thai wrap override
//!   (`effective_cosmic_wrap_mode`), and the demotion naming below; no other
//!   file may resolve a family
//! - the guaranteed-final face is IBM Plex Sans, always installed (embedded
//!   id `FontFaceId::EMBEDDED_IBM_PLEX_SANS`)
//!
//! ## The glyph-coverage demotion (named policy)
//!
//! The glyph-id-0 debug-rect demotion is the render world's honest answer to a
//! `NoCoverage` resolution: a face with no glyph for a script yields glyph id 0
//! (notdef), and the render world demotes the run to a debug rect
//! (`glyph_requires_debug_fallback` in `domains/layer_pool`) — never a silent
//! empty fallback. This is NAMED here as the policy that pairs with
//! `NoCoverage`, and its production observability is
//! `LayerPoolMetricName::TextRunGlyphCoverageDemotion` (the zero-allocation
//! counter that makes the demotion policy countable).

use cosmic_text::{Family, FamilyOwned};

use crate::domains::style::computed::{TextFamilyItem, TextFamilyList};

#[cfg(test)]
use crate::domains::style::computed::FontFamilyGeneric;

use super::text_font_catalog::{
    IBM_PLEX_SANS_FAMILY, SOURCE_HAN_SANS_FAMILY, SOURCE_HAN_SANS_K_FAMILY,
    SOURCE_HAN_SANS_SC_FAMILY,
};
use super::{FontFaceId, TextFontCatalog, TextLanguageId, TextLayoutRequest, TextWrapMode};

/// Which rule of the fallback policy table produced a resolution. Public seam
/// surface (read via the seam's resolve API in tests, never emission).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ResolutionPath {
    /// A requested family name matched an installed face (rule 1).
    RequestedName,
    /// A generic keyword mapped through the static table (rule 2).
    GenericMapped,
    /// The script hint routed to an embedded script face (rule 3).
    ScriptCoverage,
    /// No installed face covers the script; the guaranteed-final face stands in
    /// (rule 4).
    NoCoverage,
}

/// The resolved face for one text request: the shaping-ready owned family plus
/// the policy path that chose it.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedFace {
    /// The shaping-ready owned family (an inline string for typical names, so
    /// resolution does not allocate).
    pub family: FamilyOwned,
    /// The policy-table rule that produced this resolution.
    pub policy: ResolutionPath,
}

/// Resolves one text request's family through the fallback policy table (the
/// only family-resolution entry point). The resolved family is carried as
/// [`FamilyOwned`] — the non-allocating sibling of `cosmic_text::Family`.
#[must_use]
pub fn resolve(
    requested: &TextFamilyList,
    script: TextLanguageId,
    catalog: &TextFontCatalog,
) -> ResolvedFace {
    let (_, family, policy) = resolve_face_id_and_family(requested, script, catalog);
    ResolvedFace { family, policy }
}

/// Resolves the face id the render world's readiness gate keys on. Mirrors
/// [`resolve`] so the gate and the shaping resolution agree on the same face.
#[must_use]
pub(crate) fn resolve_face_id(
    family: &TextFamilyList,
    script: TextLanguageId,
    catalog: &TextFontCatalog,
) -> FontFaceId {
    resolve_face_id_and_family(family, script, catalog).0
}

/// The table itself: walks the four rules in order and returns the face id,
/// the shaping-ready family, and the path that chose them.
fn resolve_face_id_and_family(
    requested: &TextFamilyList,
    script: TextLanguageId,
    catalog: &TextFontCatalog,
) -> (FontFaceId, FamilyOwned, ResolutionPath) {
    // Rule 1: requested names in order; first installed (ASCII
    // case-insensitive) match wins.
    for item in &requested.items {
        if let TextFamilyItem::FamilyName(name) = item
            && let Some((face_id, family)) = catalog.resolve_face(name)
        {
            return (face_id, family, ResolutionPath::RequestedName);
        }
    }
    // Rule 2: the first generic keyword maps through the static table (every
    // generic routes to the guaranteed-final face).
    for item in &requested.items {
        if let TextFamilyItem::Generic(_) = item {
            return (
                FontFaceId::EMBEDDED_IBM_PLEX_SANS,
                FamilyOwned::new(Family::Name(IBM_PLEX_SANS_FAMILY)),
                ResolutionPath::GenericMapped,
            );
        }
    }
    // Rules 3 & 4: script coverage (J/SC/K → Source Han, English → IBM Plex)
    // and the no-face outcome (Thai → the guaranteed-final face, NoCoverage).
    match script {
        TextLanguageId::Japanese => (
            FontFaceId::EMBEDDED_SOURCE_HAN_SANS_J,
            FamilyOwned::new(Family::Name(SOURCE_HAN_SANS_FAMILY)),
            ResolutionPath::ScriptCoverage,
        ),
        TextLanguageId::SimplifiedChinese => (
            FontFaceId::EMBEDDED_SOURCE_HAN_SANS_SC,
            FamilyOwned::new(Family::Name(SOURCE_HAN_SANS_SC_FAMILY)),
            ResolutionPath::ScriptCoverage,
        ),
        TextLanguageId::Korean => (
            FontFaceId::EMBEDDED_SOURCE_HAN_SANS_K,
            FamilyOwned::new(Family::Name(SOURCE_HAN_SANS_K_FAMILY)),
            ResolutionPath::ScriptCoverage,
        ),
        TextLanguageId::English => (
            FontFaceId::EMBEDDED_IBM_PLEX_SANS,
            FamilyOwned::new(Family::Name(IBM_PLEX_SANS_FAMILY)),
            ResolutionPath::ScriptCoverage,
        ),
        TextLanguageId::Thai => (
            FontFaceId::EMBEDDED_IBM_PLEX_SANS,
            FamilyOwned::new(Family::Name(IBM_PLEX_SANS_FAMILY)),
            ResolutionPath::NoCoverage,
        ),
    }
}

/// The Thai locale wrap policy, moved here beside the no-coverage reality it
/// names: the backend does not expose the browser's locale-aware Thai line
/// breaker, so `Word` becomes `WordOrGlyph` — the closest available
/// approximation for the normal-wrap path.
pub(crate) fn effective_cosmic_wrap_mode(request: &TextLayoutRequest<'_>) -> TextWrapMode {
    if request.wrap_mode == TextWrapMode::Word && request.language_hint_id == TextLanguageId::Thai {
        return TextWrapMode::WordOrGlyph;
    }
    request.wrap_mode
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn family_list(items: impl IntoIterator<Item = TextFamilyItem>) -> TextFamilyList {
        TextFamilyList {
            items: items.into_iter().collect(),
        }
    }

    fn name(text: &str) -> TextFamilyItem {
        TextFamilyItem::FamilyName(Arc::<str>::from(text))
    }

    fn generic(kind: FontFamilyGeneric) -> TextFamilyItem {
        TextFamilyItem::Generic(kind)
    }

    #[test]
    fn fallback_requested_name_wins() {
        let catalog = TextFontCatalog::repo_default();
        // An installed name resolves case-insensitively (CSS Fonts 4 §3.1),
        // ahead of the script hint that would otherwise route Japanese.
        let requested = family_list([name("ibm plex sans")]);
        let resolved = resolve(&requested, TextLanguageId::Japanese, &catalog);
        assert_eq!(resolved.policy, ResolutionPath::RequestedName);
        assert_eq!(
            resolved.family,
            FamilyOwned::new(Family::Name(IBM_PLEX_SANS_FAMILY)),
        );
    }

    #[test]
    fn fallback_generic_maps() {
        let catalog = TextFontCatalog::repo_default();

        let sans = family_list([generic(FontFamilyGeneric::SansSerif)]);
        let resolved = resolve(&sans, TextLanguageId::English, &catalog);
        assert_eq!(resolved.policy, ResolutionPath::GenericMapped);
        assert_eq!(
            resolved.family,
            FamilyOwned::new(Family::Name(IBM_PLEX_SANS_FAMILY)),
        );

        // `monospace` routes to the guaranteed-final face (Plex Mono is NOT
        // embedded; a consumer installs one via the S3 door).
        let mono = family_list([generic(FontFamilyGeneric::Monospace)]);
        let resolved = resolve(&mono, TextLanguageId::English, &catalog);
        assert_eq!(resolved.policy, ResolutionPath::GenericMapped);
        assert_eq!(
            resolved.family,
            FamilyOwned::new(Family::Name(IBM_PLEX_SANS_FAMILY)),
        );
    }

    #[test]
    fn fallback_script_coverage_routes_cjk() {
        let catalog = TextFontCatalog::repo_default();
        let empty = TextFamilyList::default();

        let english = resolve(&empty, TextLanguageId::English, &catalog);
        assert_eq!(english.policy, ResolutionPath::ScriptCoverage);
        assert_eq!(
            english.family,
            FamilyOwned::new(Family::Name(IBM_PLEX_SANS_FAMILY)),
        );

        let japanese = resolve(&empty, TextLanguageId::Japanese, &catalog);
        assert_eq!(japanese.policy, ResolutionPath::ScriptCoverage);
        assert_eq!(
            japanese.family,
            FamilyOwned::new(Family::Name(SOURCE_HAN_SANS_FAMILY)),
        );

        let simplified = resolve(&empty, TextLanguageId::SimplifiedChinese, &catalog);
        assert_eq!(simplified.policy, ResolutionPath::ScriptCoverage);
        assert_eq!(
            simplified.family,
            FamilyOwned::new(Family::Name(SOURCE_HAN_SANS_SC_FAMILY)),
        );

        let korean = resolve(&empty, TextLanguageId::Korean, &catalog);
        assert_eq!(korean.policy, ResolutionPath::ScriptCoverage);
        assert_eq!(
            korean.family,
            FamilyOwned::new(Family::Name(SOURCE_HAN_SANS_K_FAMILY)),
        );
    }

    #[test]
    fn fallback_no_coverage_is_named() {
        let catalog = TextFontCatalog::repo_default();
        let empty = TextFamilyList::default();
        let thai = resolve(&empty, TextLanguageId::Thai, &catalog);
        assert_eq!(thai.policy, ResolutionPath::NoCoverage);
        assert_eq!(
            thai.family,
            FamilyOwned::new(Family::Name(IBM_PLEX_SANS_FAMILY)),
        );
    }

    #[test]
    fn fallback_final_face_is_always_installed() {
        let catalog = TextFontCatalog::repo_default();
        let (face_id, family) = catalog
            .resolve_face(IBM_PLEX_SANS_FAMILY)
            .expect("the guaranteed-final face should always be installed");
        assert_eq!(face_id, FontFaceId::EMBEDDED_IBM_PLEX_SANS);
        assert_eq!(family, FamilyOwned::new(Family::Name(IBM_PLEX_SANS_FAMILY)));
        assert!(catalog.installed_face_ids().contains(&face_id));
    }
}
