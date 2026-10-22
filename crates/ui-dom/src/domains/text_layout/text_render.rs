//! Render-facing text-layout helpers.
//!
//! This file holds the render-side font-family selection, asset-path mapping,
//! and wrapped-buffer construction so `text_layout::mod` stays focused on
//! measurement-oriented behavior.

use cosmic_text::{Buffer, Family as CosmicFamily, FontSystem};

use super::{
    TextAlign, TextLanguageId, TextLayoutRequest, TextWrapMode, build_text_buffer,
    effective_line_height_px,
};

pub(crate) const UI_DOM_FONT_IBM_PLEX_SANS_PATH: &str =
    "fonts/vendor/ibm-plex/IBMPlexSans-Variable.ttf";
pub(crate) const UI_DOM_FONT_SOURCE_HAN_SANS_J_PATH: &str =
    "fonts/vendor/source-han-sans/SourceHanSansJ-Regular.otf";
pub(crate) const UI_DOM_FONT_SOURCE_HAN_SANS_SC_PATH: &str =
    "fonts/vendor/source-han-sans/SourceHanSansSC-Regular.otf";
pub(crate) const UI_DOM_FONT_SOURCE_HAN_SANS_K_PATH: &str =
    "fonts/vendor/source-han-sans/SourceHanSansK-Regular.otf";

const UI_TEXT_PREFERRED_SANS_FAMILY: &str = "IBM Plex Sans";
const UI_TEXT_JAPANESE_PREFERRED_FAMILY: &str = "Source Han Sans";
const UI_TEXT_SIMPLIFIED_CHINESE_PREFERRED_FAMILY: &str = "Source Han Sans SC";
const UI_TEXT_KOREAN_PREFERRED_FAMILY: &str = "Source Han Sans K";

/// The four embedded families, by name (the installed set S2's "first family if
/// installed" checks against; the S4 table generalizes this to the catalog).
const INSTALLED_FAMILY_NAMES: &[&str] = &[
    UI_TEXT_PREFERRED_SANS_FAMILY,
    UI_TEXT_JAPANESE_PREFERRED_FAMILY,
    UI_TEXT_SIMPLIFIED_CHINESE_PREFERRED_FAMILY,
    UI_TEXT_KOREAN_PREFERRED_FAMILY,
];

#[must_use]
pub(crate) fn build_render_text_buffer(
    font_system: &mut FontSystem,
    request: &TextLayoutRequest<'_>,
    height_px: Option<f32>,
) -> Buffer {
    let line_height_px = effective_line_height_px(font_system, request);
    build_text_buffer(
        font_system,
        request,
        line_height_px,
        request.width_px.map(|width_px| width_px.max(0.0)),
        height_px,
    )
}

/// S2's attrs family resolution (the S4 table's rule-1 seed): the request's
/// first family name if installed, else today's language-routing. S4
/// generalizes this into the full policy table (generics, named paths,
/// per-script tests).
#[must_use]
pub(crate) fn resolve_requested_family(request: &TextLayoutRequest<'_>) -> CosmicFamily<'static> {
    if let Some(name) = request.family.first_family_name()
        && let Some(family) = installed_family_by_name(name)
    {
        return family;
    }
    preferred_repo_font_family(request.language_hint_id)
}

/// Returns the `CosmicFamily` for one requested family name when that name is
/// installed (ASCII case-insensitive, CSS Fonts 4 §3.1); `None` otherwise.
#[must_use]
pub(crate) fn installed_family_by_name(name: &str) -> Option<CosmicFamily<'static>> {
    INSTALLED_FAMILY_NAMES
        .iter()
        .find(|candidate| candidate.eq_ignore_ascii_case(name))
        .map(|candidate| CosmicFamily::Name(candidate))
}

#[must_use]
pub(crate) fn preferred_repo_font_family(
    language_hint_id: TextLanguageId,
) -> CosmicFamily<'static> {
    match language_hint_id {
        TextLanguageId::Japanese => CosmicFamily::Name(UI_TEXT_JAPANESE_PREFERRED_FAMILY),
        TextLanguageId::SimplifiedChinese => {
            CosmicFamily::Name(UI_TEXT_SIMPLIFIED_CHINESE_PREFERRED_FAMILY)
        }
        TextLanguageId::Korean => CosmicFamily::Name(UI_TEXT_KOREAN_PREFERRED_FAMILY),
        TextLanguageId::English | TextLanguageId::Thai => {
            CosmicFamily::Name(UI_TEXT_PREFERRED_SANS_FAMILY)
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
    UI_TEXT_PREFERRED_SANS_FAMILY
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
