//! Render-facing text-layout helpers.
//!
//! This file holds the render-side font-family selection, asset-path mapping,
//! and wrapped-buffer construction so `text_layout::mod` stays focused on
//! measurement-oriented behavior. It owns the seam's face resolution: the
//! requested family's first installed face (against the shared catalog), else
//! the language-routed embedded face.

use cosmic_text::{Buffer, Family as CosmicFamily, FamilyOwned, FontSystem};

use crate::domains::style::computed::TextFamilyList;

use super::text_font_catalog::{
    IBM_PLEX_SANS_FAMILY, SOURCE_HAN_SANS_FAMILY, SOURCE_HAN_SANS_K_FAMILY,
    SOURCE_HAN_SANS_SC_FAMILY,
};
use super::{
    FontFaceId, TextAlign, TextFontCatalog, TextLanguageId, TextLayoutRequest, TextWrapMode,
    build_text_buffer, effective_line_height_px,
};

pub(crate) const UI_DOM_FONT_IBM_PLEX_SANS_PATH: &str =
    "fonts/vendor/ibm-plex/IBMPlexSans-Variable.ttf";
pub(crate) const UI_DOM_FONT_SOURCE_HAN_SANS_J_PATH: &str =
    "fonts/vendor/source-han-sans/SourceHanSansJ-Regular.otf";
pub(crate) const UI_DOM_FONT_SOURCE_HAN_SANS_SC_PATH: &str =
    "fonts/vendor/source-han-sans/SourceHanSansSC-Regular.otf";
pub(crate) const UI_DOM_FONT_SOURCE_HAN_SANS_K_PATH: &str =
    "fonts/vendor/source-han-sans/SourceHanSansK-Regular.otf";

#[must_use]
pub(crate) fn build_render_text_buffer(
    font_system: &mut FontSystem,
    font_catalog: &TextFontCatalog,
    request: &TextLayoutRequest<'_>,
    height_px: Option<f32>,
) -> Buffer {
    let line_height_px = effective_line_height_px(font_system, request);
    build_text_buffer(
        font_system,
        font_catalog,
        request,
        line_height_px,
        request.width_px.map(|width_px| width_px.max(0.0)),
        height_px,
    )
}

/// The resolved family for one request: the shaping-ready owned family. The
/// owned family is an inline string for typical names, so resolution does not
/// allocate. The face id is resolved separately by [`resolve_face_id`] for the
/// render world's registration gate.
#[must_use]
pub(crate) fn resolve_requested_family(
    request: &TextLayoutRequest<'_>,
    font_catalog: &TextFontCatalog,
) -> FamilyOwned {
    if let Some(name) = request.family.first_family_name()
        && let Some((_, family)) = font_catalog.resolve_face(name)
    {
        return family;
    }
    let (_, family_name) = preferred_repo_font_face(request.language_hint_id);
    FamilyOwned::new(CosmicFamily::Name(family_name))
}

/// The face id the render world checks registration readiness against for one
/// run record: the record's first family if installed, else the language
/// default. Mirrors [`resolve_requested_family`] so the readiness gate and the
/// shaping resolution agree.
#[must_use]
pub(crate) fn resolve_face_id(
    family: &TextFamilyList,
    language_hint_id: TextLanguageId,
    font_catalog: &TextFontCatalog,
) -> FontFaceId {
    if let Some(name) = family.first_family_name()
        && let Some((face_id, _)) = font_catalog.resolve_face(name)
    {
        return face_id;
    }
    preferred_repo_font_face(language_hint_id).0
}

/// The language-routed embedded face (the pre-S4 fallback): returns the face id
/// and its static family name.
#[must_use]
fn preferred_repo_font_face(language_hint_id: TextLanguageId) -> (FontFaceId, &'static str) {
    match language_hint_id {
        TextLanguageId::Japanese => (
            FontFaceId::EMBEDDED_SOURCE_HAN_SANS_J,
            SOURCE_HAN_SANS_FAMILY,
        ),
        TextLanguageId::SimplifiedChinese => (
            FontFaceId::EMBEDDED_SOURCE_HAN_SANS_SC,
            SOURCE_HAN_SANS_SC_FAMILY,
        ),
        TextLanguageId::Korean => (
            FontFaceId::EMBEDDED_SOURCE_HAN_SANS_K,
            SOURCE_HAN_SANS_K_FAMILY,
        ),
        TextLanguageId::English | TextLanguageId::Thai => {
            (FontFaceId::EMBEDDED_IBM_PLEX_SANS, IBM_PLEX_SANS_FAMILY)
        }
    }
}

#[must_use]
pub(crate) fn ui_sans_repo_font_asset_paths() -> &'static [&'static str] {
    &[
        UI_DOM_FONT_IBM_PLEX_SANS_PATH,
        UI_DOM_FONT_SOURCE_HAN_SANS_J_PATH,
        UI_DOM_FONT_SOURCE_HAN_SANS_SC_PATH,
        UI_DOM_FONT_SOURCE_HAN_SANS_K_PATH,
    ]
}

#[must_use]
pub(crate) fn repo_default_sans_serif_family_name() -> &'static str {
    IBM_PLEX_SANS_FAMILY
}

#[must_use]
pub(crate) fn cosmic_wrap(request: &TextLayoutRequest<'_>) -> cosmic_text::Wrap {
    match effective_cosmic_wrap_mode(request) {
        TextWrapMode::None => cosmic_text::Wrap::None,
        TextWrapMode::Word => cosmic_text::Wrap::Word,
        TextWrapMode::Glyph => cosmic_text::Wrap::Glyph,
        TextWrapMode::WordOrGlyph => cosmic_text::Wrap::WordOrGlyph,
    }
}

fn effective_cosmic_wrap_mode(request: &TextLayoutRequest<'_>) -> TextWrapMode {
    if request.wrap_mode == TextWrapMode::Word && request.language_hint_id == TextLanguageId::Thai {
        // Blink and Gecko run locale-aware line breaking for Thai text. Our
        // backend does not expose that exact breaker, so `WordOrGlyph` is the
        // closest available approximation for the browser normal-wrap path.
        return TextWrapMode::WordOrGlyph;
    }
    request.wrap_mode
}

#[must_use]
pub(crate) fn cosmic_align(alignment: TextAlign) -> cosmic_text::Align {
    match alignment {
        TextAlign::Left => cosmic_text::Align::Left,
        TextAlign::Center => cosmic_text::Align::Center,
        TextAlign::Right => cosmic_text::Align::Right,
        TextAlign::Justified => cosmic_text::Align::Justified,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_routing_falls_back_to_repo_families() {
        let catalog = TextFontCatalog::repo_default();
        let english =
            resolve_requested_family(&TextLayoutRequest::for_text("hello", 14.0), &catalog);
        assert_eq!(
            english,
            FamilyOwned::new(CosmicFamily::Name(IBM_PLEX_SANS_FAMILY)),
        );

        let japanese =
            resolve_requested_family(&TextLayoutRequest::for_text("メニュー", 14.0), &catalog);
        assert_eq!(
            japanese,
            FamilyOwned::new(CosmicFamily::Name(SOURCE_HAN_SANS_FAMILY)),
        );

        let simplified =
            resolve_requested_family(&TextLayoutRequest::for_text("颜色", 14.0), &catalog);
        assert_eq!(
            simplified,
            FamilyOwned::new(CosmicFamily::Name(SOURCE_HAN_SANS_SC_FAMILY)),
        );

        let korean = resolve_requested_family(&TextLayoutRequest::for_text("메뉴", 14.0), &catalog);
        assert_eq!(
            korean,
            FamilyOwned::new(CosmicFamily::Name(SOURCE_HAN_SANS_K_FAMILY)),
        );
    }
}
