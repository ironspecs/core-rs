//! Render-facing text-layout helpers.
//!
//! This file holds the render-side font-family selection, asset-path mapping,
//! and wrapped-buffer construction so `text_layout::mod` stays focused on
//! measurement-oriented behavior.

use cosmic_text::{Buffer, Family as CosmicFamily, FontSystem};

use super::{
    TextAlign, TextFontFamilyId, TextLanguageId, TextLayoutRequest, TextWrapMode,
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

const UI_TEXT_PREFERRED_SANS_FAMILY: &str = "IBM Plex Sans";
const UI_TEXT_JAPANESE_PREFERRED_FAMILY: &str = "Source Han Sans";
const UI_TEXT_SIMPLIFIED_CHINESE_PREFERRED_FAMILY: &str = "Source Han Sans SC";
const UI_TEXT_KOREAN_PREFERRED_FAMILY: &str = "Source Han Sans K";

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

#[must_use]
pub(crate) fn preferred_repo_font_family(
    font_family_id: TextFontFamilyId,
    language_hint_id: TextLanguageId,
) -> CosmicFamily<'static> {
    match (font_family_id, language_hint_id) {
        (TextFontFamilyId::UiSans, TextLanguageId::Japanese) => {
            CosmicFamily::Name(UI_TEXT_JAPANESE_PREFERRED_FAMILY)
        }
        (TextFontFamilyId::UiSans, TextLanguageId::SimplifiedChinese) => {
            CosmicFamily::Name(UI_TEXT_SIMPLIFIED_CHINESE_PREFERRED_FAMILY)
        }
        (TextFontFamilyId::UiSans, TextLanguageId::Korean) => {
            CosmicFamily::Name(UI_TEXT_KOREAN_PREFERRED_FAMILY)
        }
        (TextFontFamilyId::UiSans, TextLanguageId::English | TextLanguageId::Thai) => {
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
