//! Render-facing text-layout helpers.
//!
//! This file holds the render-side buffer construction, asset-path mapping,
//! and the cosmic wrap/align mappings so `text_layout::mod` stays focused on
//! measurement-oriented behavior. Family resolution no longer lives here: the
//! fallback policy table moved to `font_fallback_policy.rs` (text seam S4), and
//! this file only maps resolved inputs into cosmic-text.

use cosmic_text::{Buffer, FontSystem};

use super::font_fallback_policy;
use super::text_font_catalog::IBM_PLEX_SANS_FAMILY;
use super::{
    TextAlign, TextFontCatalog, TextLayoutRequest, TextWrapMode, build_text_buffer,
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
    match font_fallback_policy::effective_cosmic_wrap_mode(request) {
        TextWrapMode::None => cosmic_text::Wrap::None,
        TextWrapMode::Word => cosmic_text::Wrap::Word,
        TextWrapMode::Glyph => cosmic_text::Wrap::Glyph,
        TextWrapMode::WordOrGlyph => cosmic_text::Wrap::WordOrGlyph,
    }
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
    fn cosmic_wrap_delegates_the_thai_word_override_to_the_policy_table() {
        // Thai normal-wrap requests become WordOrGlyph: the named locale
        // decision owned by `font_fallback_policy::effective_cosmic_wrap_mode`.
        let thai_word = TextLayoutRequest::for_text("เมนู", 14.0);
        assert_eq!(cosmic_wrap(&thai_word), cosmic_text::Wrap::WordOrGlyph);

        let english_word = TextLayoutRequest::for_text("hello", 14.0);
        assert_eq!(cosmic_wrap(&english_word), cosmic_text::Wrap::Word);

        // The override is Word-specific: a Thai glyph-wrap request stays Glyph.
        let thai_glyph = TextLayoutRequest {
            wrap_mode: TextWrapMode::Glyph,
            ..TextLayoutRequest::for_text("เมนู", 14.0)
        };
        assert_eq!(cosmic_wrap(&thai_glyph), cosmic_text::Wrap::Glyph);
    }

    #[test]
    fn cosmic_align_maps_each_align_variant() {
        assert_eq!(cosmic_align(TextAlign::Left), cosmic_text::Align::Left);
        assert_eq!(cosmic_align(TextAlign::Center), cosmic_text::Align::Center);
        assert_eq!(cosmic_align(TextAlign::Right), cosmic_text::Align::Right);
        assert_eq!(
            cosmic_align(TextAlign::Justified),
            cosmic_text::Align::Justified
        );
    }
}
