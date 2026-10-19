//! Text layout context for `ui-dom`.
//!
//! This file owns the `TextLayoutContext` value that wraps a cosmic-text
//! `FontSystem` and exposes the measurement, wrapping, and caret-layout
//! operations the shared text runtime calls. Buffer construction and the
//! caret-map helpers stay in `text_layout::mod`.
//!
//! Invariants and rules:
//!
//! - this file owns only the `TextLayoutContext` struct and its `impl` block
//! - every operation delegates to the parent module's buffer and caret helpers
//! - `measure_intrinsic` derives the fragment-level intrinsic surface (leading
//!   and trailing unbreakable-fragment widths, boundary wrap-opportunity
//!   flags) from the same min-content buffer that supplies min-content width —
//!   no extra cosmic measurement per node
//! - the thread-local repo context is created once from
//!   `TextLayoutConfig::repo_default`

use cosmic_text::FontSystem;

use crate::numeric::usize_to_f32;
use crate::types::DomWhiteSpace;

use super::text_render;
use super::{
    CaretStop, TextCaretLayout, TextCaretLine, TextCaretMap, TextIntrinsicMetrics,
    TextLayoutConfig, TextLayoutRequest, WrappedTextLayout, WrappedTextLineLayout,
    build_text_buffer, build_wrapped_text_buffer, build_wrapped_text_buffer_and_byte_map,
    effective_line_height_px, install_text_font_catalog, text_caret_layout_from_buffer,
    text_caret_map_from_buffer, text_caret_stop_byte_index_from_map,
    wrapped_text_height_from_buffer, wrapped_text_layout_from_buffer,
};

#[cfg(test)]
use super::{
    DEFAULT_TEXT_FONT_SIZE_PX, TextLanguageId, TextWrapMode, text_wrap_mode_for_css_intrinsic,
    text_wrap_mode_for_css_layout,
};
#[cfg(test)]
use crate::types::{DomOverflowWrap, DomWordBreak};

pub(crate) struct TextLayoutContext {
    pub(super) font_system: FontSystem,
}

impl TextLayoutContext {
    #[must_use]
    pub(crate) fn new(config: TextLayoutConfig) -> Self {
        let mut font_system = FontSystem::new();
        font_system
            .db_mut()
            .set_sans_serif_family(text_render::repo_default_sans_serif_family_name());
        install_text_font_catalog(&mut font_system, config.font_catalog);
        Self { font_system }
    }

    #[must_use]
    pub(crate) fn measure_intrinsic(
        &mut self,
        request: &TextLayoutRequest<'_>,
    ) -> TextIntrinsicMetrics {
        let line_height_px = effective_line_height_px(&mut self.font_system, request);
        // Boundary wrap-opportunity flags are text-shaped, not measurement
        // shaped: a collapsed whitespace/segment-break character at either edge
        // is itself a soft-wrap opportunity, so it separates this node's edge
        // fragment from a neighbor's across the boundary (CSS Text 3 §4).
        let starts_with_wrap_opportunity = white_space_is_collapse_mode(request.white_space)
            && text_starts_with_collapsible_boundary_whitespace(request.text);
        let ends_with_wrap_opportunity = white_space_is_collapse_mode(request.white_space)
            && text_ends_with_collapsible_boundary_whitespace(request.text);
        if request.font_size_px == 0.0 {
            // Zero-size text is CSS-legal (CSS Fonts 4 §3.5); cosmic-text asserts a
            // non-zero font size, so measure without it: zero-width glyphs.
            return TextIntrinsicMetrics {
                min_content_width_px: 0.0,
                max_content_width_px: 0.0,
                line_height_px,
                leading_fragment_width_px: 0.0,
                trailing_fragment_width_px: 0.0,
                starts_with_wrap_opportunity,
                ends_with_wrap_opportunity,
                fragment_line_height_max_px: line_height_px,
            };
        }
        let max_content_width_px =
            build_text_buffer(&mut self.font_system, request, line_height_px, None, None)
                .layout_runs()
                .fold(0.0f32, |width_px, run| width_px.max(run.line_w));
        // The min-content pass already forces a break at every soft-wrap
        // opportunity (width 0), so each layout run is exactly one unbreakable
        // fragment: the first run is the leading fragment, the last run is the
        // trailing fragment, and the fold over `LayoutRun::line_w` is the
        // min-content width. No third buffer and no extra cosmic measurement —
        // the same runs supply all three quantities.
        let min_content_buffer = build_text_buffer(
            &mut self.font_system,
            request,
            line_height_px,
            Some(0.0),
            None,
        );
        let mut min_content_width_px = 0.0f32;
        let mut leading_fragment_width_px = 0.0f32;
        let mut trailing_fragment_width_px = 0.0f32;
        for (run_index, run) in min_content_buffer.layout_runs().enumerate() {
            min_content_width_px = min_content_width_px.max(run.line_w);
            if run_index == 0 {
                leading_fragment_width_px = run.line_w;
            }
            trailing_fragment_width_px = run.line_w;
        }
        TextIntrinsicMetrics {
            min_content_width_px,
            max_content_width_px,
            line_height_px,
            leading_fragment_width_px,
            trailing_fragment_width_px,
            starts_with_wrap_opportunity,
            ends_with_wrap_opportunity,
            // Single-style text today: every fragment shares one line height, so
            // the fragment maximum is the node's own line height.
            fragment_line_height_max_px: line_height_px,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    #[must_use]
    pub(crate) fn layout_wrapped_text(
        &mut self,
        request: &TextLayoutRequest<'_>,
    ) -> WrappedTextLayout {
        if request.font_size_px == 0.0 {
            // Same zero guard: zero-width text occupies one line per source
            // line under newline-preserving white-space, one line otherwise.
            let line_height_px = effective_line_height_px(&mut self.font_system, request);
            let line_count = zero_size_line_count(request);
            let lines = (0..line_count)
                .map(|index| WrappedTextLineLayout {
                    width_px: 0.0,
                    baseline_y_px: usize_to_f32(index) * line_height_px,
                    height_px: line_height_px,
                })
                .collect();
            return WrappedTextLayout {
                width_px: 0.0,
                height_px: usize_to_f32(line_count) * line_height_px,
                line_count,
                lines,
            };
        }
        wrapped_text_layout_from_buffer(build_wrapped_text_buffer(&mut self.font_system, request))
    }

    #[must_use]
    pub(crate) fn measure_wrapped_text_height(&mut self, request: &TextLayoutRequest<'_>) -> f32 {
        if request.font_size_px == 0.0 {
            // Same zero guard: height is the line count for the white-space
            // policy times the line height.
            return usize_to_f32(zero_size_line_count(request))
                * effective_line_height_px(&mut self.font_system, request);
        }
        wrapped_text_height_from_buffer(build_wrapped_text_buffer(&mut self.font_system, request))
    }

    #[must_use]
    pub(crate) fn layout_text_caret(
        &mut self,
        request: &TextLayoutRequest<'_>,
        scalar_offset: usize,
    ) -> TextCaretLayout {
        if request.font_size_px == 0.0 {
            // Zero-size text (CSS Fonts 4 §3.5): every caret position coincides at
            // the origin; cosmic-text asserts a non-zero font size.
            return TextCaretLayout {
                x_px: 0.0,
                y_px: 0.0,
                height_px: effective_line_height_px(&mut self.font_system, request),
            };
        }
        let (buffer, transformed_text, transformed_to_original_byte) =
            build_wrapped_text_buffer_and_byte_map(&mut self.font_system, request);
        text_caret_layout_from_buffer(
            request.text,
            &transformed_text,
            &transformed_to_original_byte,
            scalar_offset,
            &buffer,
        )
    }

    #[cfg_attr(not(test), allow(dead_code))]
    #[must_use]
    pub(crate) fn layout_text_caret_map(
        &mut self,
        request: &TextLayoutRequest<'_>,
    ) -> TextCaretMap {
        if request.font_size_px == 0.0 {
            return zero_size_caret_map(
                request,
                effective_line_height_px(&mut self.font_system, request),
            );
        }
        let (buffer, transformed_text, transformed_to_original_byte) =
            build_wrapped_text_buffer_and_byte_map(&mut self.font_system, request);
        text_caret_map_from_buffer(&transformed_text, &transformed_to_original_byte, &buffer)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    #[must_use]
    pub(crate) fn layout_text_caret_stop_byte_index_at_point(
        &mut self,
        request: &TextLayoutRequest<'_>,
        x_px: f32,
        y_px: f32,
    ) -> usize {
        if request.font_size_px == 0.0 {
            // Zero-size text: every position coincides; the only honest byte index
            // without glyph data is the text start.
            return 0;
        }
        let (buffer, transformed_text, transformed_to_original_byte) =
            build_wrapped_text_buffer_and_byte_map(&mut self.font_system, request);
        let caret_map =
            text_caret_map_from_buffer(&transformed_text, &transformed_to_original_byte, &buffer);
        text_caret_stop_byte_index_from_map(&caret_map, x_px, y_px)
    }
}

/// The line count for zero-size text: newline-preserving wrap policies keep
/// one line per source line; collapsing policies render one line (zero-width
/// text never wraps). Shared by the zero-size guards so the wrapped layout,
/// the height measure, and the caret map cannot disagree.
fn zero_size_line_count(request: &TextLayoutRequest<'_>) -> usize {
    match request.white_space {
        DomWhiteSpace::Pre
        | DomWhiteSpace::PreWrap
        | DomWhiteSpace::PreLine
        | DomWhiteSpace::BreakSpaces => super::line_start_byte_indices(request.text).len().max(1),
        DomWhiteSpace::Normal | DomWhiteSpace::Nowrap => 1,
    }
}

/// The caret map for zero-size text: newline-preserving white-space keeps one
/// line per source line, collapsing white-space keeps one line; every stop
/// sits at x = 0 (positions coincide without glyph data; cosmic-text asserts
/// a non-zero font size, so the map is built without it).
fn zero_size_caret_map(request: &TextLayoutRequest<'_>, line_height_px: f32) -> TextCaretMap {
    if zero_size_line_count(request) == 1 {
        // Collapsing white-space renders newlines as a single zero-width line.
        return TextCaretMap {
            lines: vec![TextCaretLine {
                top_px: 0.0,
                height_px: line_height_px,
                stops: vec![CaretStop {
                    byte_index: 0,
                    x_px: 0.0,
                }],
            }],
        };
    }
    let lines = super::line_start_byte_indices(request.text)
        .iter()
        .enumerate()
        .map(|(index, &byte_index)| TextCaretLine {
            top_px: usize_to_f32(index) * line_height_px,
            height_px: line_height_px,
            stops: vec![CaretStop {
                byte_index,
                x_px: 0.0,
            }],
        })
        .collect();
    TextCaretMap { lines }
}

/// The collapse white-space class (`normal`/`nowrap`/`pre-line`) collapses
/// space runs; the preserve class (`pre`/`pre-wrap`/`break-spaces`) keeps them
/// literal. Mirrors `text_layout_input_for_white_space`'s grouping so the
/// boundary wrap-opportunity flags agree with the shaping path.
fn white_space_is_collapse_mode(white_space: DomWhiteSpace) -> bool {
    matches!(
        white_space,
        DomWhiteSpace::Normal | DomWhiteSpace::Nowrap | DomWhiteSpace::PreLine
    )
}

/// Collapsible whitespace and segment-break characters that become a boundary
/// soft-wrap opportunity (or forced break) under a collapse white-space mode.
/// Matches `white_space::is_collapsible_ascii_whitespace`'s set; a segment
/// break at either edge is a break even in `pre-line`, where it is preserved
/// rather than collapsed.
fn is_collapsible_boundary_whitespace(character: char) -> bool {
    matches!(character, ' ' | '\t' | '\n' | '\r' | '\u{000c}')
}

fn text_starts_with_collapsible_boundary_whitespace(text: &str) -> bool {
    text.chars()
        .next()
        .is_some_and(is_collapsible_boundary_whitespace)
}

fn text_ends_with_collapsible_boundary_whitespace(text: &str) -> bool {
    text.chars()
        .next_back()
        .is_some_and(is_collapsible_boundary_whitespace)
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    fn default_context() -> TextLayoutContext {
        TextLayoutContext::new(super::TextLayoutConfig::repo_default())
    }

    fn assert_non_empty_caret_lines(caret_map: &TextCaretMap) {
        assert!(!caret_map.lines.is_empty());
        for line in &caret_map.lines {
            assert!(!line.stops.is_empty());
        }
    }

    /// The standalone measured width of a single word (max-content equals
    /// min-content for a word, since a word is one unbreakable fragment).
    fn measure_word_width(context: &mut TextLayoutContext, word: &str) -> f32 {
        context
            .measure_intrinsic(&TextLayoutRequest::for_text(
                word,
                DEFAULT_TEXT_FONT_SIZE_PX,
            ))
            .max_content_width_px
    }

    #[test]
    fn intrinsic_width_for_rtmp_server_matches_single_line_wrapped_text_width() {
        let mut context = default_context();
        let request = TextLayoutRequest::for_text("RTMP Server", DEFAULT_TEXT_FONT_SIZE_PX);
        let intrinsic = context.measure_intrinsic(&request);
        let wrapped_text = context.layout_wrapped_text(&TextLayoutRequest {
            line_height_px: Some(intrinsic.line_height_px),
            width_px: Some(intrinsic.max_content_width_px),
            ..request
        });

        assert_eq!(wrapped_text.line_count, 1);
    }

    #[test]
    fn intrinsic_width_for_session_state_matches_single_line_wrapped_text_width() {
        let mut context = default_context();
        let request = TextLayoutRequest::for_text("Session State", DEFAULT_TEXT_FONT_SIZE_PX);
        let intrinsic = context.measure_intrinsic(&request);
        let wrapped_text = context.layout_wrapped_text(&TextLayoutRequest {
            line_height_px: Some(intrinsic.line_height_px),
            width_px: Some(intrinsic.max_content_width_px),
            ..request
        });

        assert_eq!(wrapped_text.line_count, 1);
    }

    #[test]
    fn japanese_wrapped_text_height_grows_when_width_is_constrained() {
        let mut context = default_context();
        let request = TextLayoutRequest::for_text("シーンを選択", DEFAULT_TEXT_FONT_SIZE_PX);
        let intrinsic = context.measure_intrinsic(&request);
        let wrapped_text = context.layout_wrapped_text(&TextLayoutRequest {
            line_height_px: Some(intrinsic.line_height_px),
            width_px: Some(intrinsic.max_content_width_px - 1.0),
            ..request
        });

        assert_eq!(request.language_hint_id, TextLanguageId::Japanese);
        assert!(wrapped_text.line_count > 1);
        assert!(wrapped_text.height_px > intrinsic.line_height_px);
    }

    #[test]
    fn japanese_keep_all_stays_single_line_when_width_is_constrained() {
        let mut context = default_context();
        let request =
            TextLayoutRequest::for_text("窓ぎわのトットちゃん", DEFAULT_TEXT_FONT_SIZE_PX);
        let intrinsic = context.measure_intrinsic(&request);
        let constrained_width_px = (intrinsic.max_content_width_px - 1.0).max(1.0);
        let normal_wrapped_text = context.layout_wrapped_text(&TextLayoutRequest {
            width_px: Some(constrained_width_px),
            ..request.clone()
        });
        let keep_all_wrapped_text = context.layout_wrapped_text(&TextLayoutRequest {
            width_px: Some(constrained_width_px),
            word_break: DomWordBreak::KeepAll,
            wrap_mode: text_wrap_mode_for_css_layout(
                DomWhiteSpace::Normal,
                DomOverflowWrap::Normal,
                DomWordBreak::KeepAll,
            ),
            ..request
        });

        assert!(
            normal_wrapped_text.line_count > 1,
            "Japanese normal wrap should still break under a narrow width",
        );
        assert_eq!(
            keep_all_wrapped_text.line_count, 1,
            "word-break:keep-all should suppress implicit Japanese intra-word wraps",
        );
    }

    #[test]
    fn japanese_keep_all_still_breaks_at_zero_width_space() {
        let mut context = default_context();
        let first_segment_request =
            TextLayoutRequest::for_text("窓ぎわの", DEFAULT_TEXT_FONT_SIZE_PX);
        let first_segment_width_px = context
            .measure_intrinsic(&first_segment_request)
            .max_content_width_px;
        let keep_all_request = TextLayoutRequest {
            text: "窓ぎわの\u{200b}トットちゃん",
            width_px: Some(first_segment_width_px),
            word_break: DomWordBreak::KeepAll,
            wrap_mode: text_wrap_mode_for_css_layout(
                DomWhiteSpace::Normal,
                DomOverflowWrap::Normal,
                DomWordBreak::KeepAll,
            ),
            ..TextLayoutRequest::for_text("窓ぎわの\u{200b}トットちゃん", DEFAULT_TEXT_FONT_SIZE_PX)
        };

        let wrapped_text = context.layout_wrapped_text(&keep_all_request);

        assert_eq!(
            wrapped_text.line_count, 2,
            "word-break:keep-all should preserve explicit zero-width-space breaks",
        );
    }

    #[test]
    fn thai_intrinsic_width_matches_single_line_wrapped_text_width() {
        let mut context = default_context();
        let request = TextLayoutRequest::for_text("เลือกฉาก", DEFAULT_TEXT_FONT_SIZE_PX);
        let intrinsic = context.measure_intrinsic(&request);
        let wrapped_text = context.layout_wrapped_text(&TextLayoutRequest {
            line_height_px: Some(intrinsic.line_height_px),
            width_px: Some(intrinsic.max_content_width_px),
            wrap_mode: TextWrapMode::WordOrGlyph,
            ..request
        });

        assert_eq!(request.language_hint_id, TextLanguageId::Thai);
        assert_eq!(wrapped_text.line_count, 1);
    }

    #[test]
    fn thai_default_wrap_grows_when_width_is_constrained() {
        let mut context = default_context();
        let request = TextLayoutRequest {
            text: "สถานะเซสชันกำลังทำงานและมีรายละเอียดเพิ่มเติม",
            width_px: Some(36.0),
            ..TextLayoutRequest::for_text(
                "สถานะเซสชันกำลังทำงานและมีรายละเอียดเพิ่มเติม",
                DEFAULT_TEXT_FONT_SIZE_PX,
            )
        };

        let wrapped_text = context.layout_wrapped_text(&request);

        assert_eq!(request.language_hint_id, TextLanguageId::Thai);
        assert!(
            wrapped_text.line_count > 1,
            "Thai default wrapping should grow when the available width is constrained",
        );
    }

    #[test]
    fn wrapped_text_height_measurement_matches_full_wrapped_text_layout_height() {
        let mut context = default_context();
        let request = TextLayoutRequest {
            text: "Diagnostics Menu",
            width_px: Some(48.0),
            ..TextLayoutRequest::for_text("Diagnostics Menu", DEFAULT_TEXT_FONT_SIZE_PX)
        };

        let wrapped_text_height_px = context.measure_wrapped_text_height(&request);
        let wrapped_text = context.layout_wrapped_text(&request);

        assert_eq!(wrapped_text_height_px, wrapped_text.height_px);
        assert!(wrapped_text.line_count > 1);
    }

    #[test]
    fn caret_map_builds_ordered_stops_for_single_line_text() {
        let mut context = default_context();
        let request = TextLayoutRequest::for_text("Test", DEFAULT_TEXT_FONT_SIZE_PX);

        let caret_map = context.layout_text_caret_map(&request);

        assert_eq!(caret_map.lines.len(), 1);
        let line = &caret_map.lines[0];
        assert_eq!(line.stops.first().map(|stop| stop.byte_index), Some(0));
        assert_eq!(
            line.stops.last().map(|stop| stop.byte_index),
            Some(request.text.len())
        );
        for window in line.stops.windows(2) {
            assert!(window[0].x_px <= window[1].x_px);
        }
    }

    #[test]
    fn caret_map_keeps_original_end_offset_when_normal_collapses_spaces() {
        let mut context = default_context();
        let request = TextLayoutRequest::for_text_with_white_space(
            "A    B",
            DEFAULT_TEXT_FONT_SIZE_PX,
            DomWhiteSpace::Normal,
        );

        let caret_map = context.layout_text_caret_map(&request);
        let line = caret_map
            .lines
            .first()
            .expect("single-line collapsed-space text should keep one line");

        assert_eq!(
            line.stops.last().map(|stop| stop.byte_index),
            Some(request.text.len()),
            "collapsed-space caret map should still end at the original text byte length",
        );
    }

    #[test]
    fn caret_map_keeps_original_end_offset_when_break_spaces_preserves_runs() {
        let mut context = default_context();
        let request = TextLayoutRequest::for_text_with_white_space(
            "A    B",
            DEFAULT_TEXT_FONT_SIZE_PX,
            DomWhiteSpace::BreakSpaces,
        );

        let caret_map = context.layout_text_caret_map(&request);
        let line = caret_map
            .lines
            .first()
            .expect("single-line preserved-space text should keep one line");

        assert_eq!(
            line.stops.last().map(|stop| stop.byte_index),
            Some(request.text.len()),
            "preserved-space caret map should still end at the original text byte length",
        );
    }

    #[test]
    fn default_wrap_does_not_split_ascii_words_into_glyph_columns() {
        let mut context = default_context();
        let word_request = TextLayoutRequest::for_text("Alpha", DEFAULT_TEXT_FONT_SIZE_PX);
        let word_width_px = context
            .measure_intrinsic(&word_request)
            .max_content_width_px;
        let request = TextLayoutRequest {
            text: "Alpha Beta",
            width_px: Some((word_width_px - 1.0).max(1.0)),
            ..TextLayoutRequest::for_text("Alpha Beta", DEFAULT_TEXT_FONT_SIZE_PX)
        };

        let wrapped_text = context.layout_wrapped_text(&request);

        assert_eq!(
            wrapped_text.line_count, 2,
            "default wrap should keep each ASCII word intact and move it to its own line when the line is narrower than the word",
        );
    }

    #[test]
    fn break_word_falls_back_to_glyph_breaks_only_after_word_wrap_overflow() {
        let mut context = default_context();
        let word_request = TextLayoutRequest::for_text("AlphaBeta", DEFAULT_TEXT_FONT_SIZE_PX);
        let word_width_px = context
            .measure_intrinsic(&word_request)
            .max_content_width_px;
        let constrained_width_px = (word_width_px - 1.0).max(1.0);
        let normal_request = TextLayoutRequest {
            text: "AlphaBeta",
            width_px: Some(constrained_width_px),
            wrap_mode: text_wrap_mode_for_css_layout(
                DomWhiteSpace::Normal,
                DomOverflowWrap::Normal,
                DomWordBreak::Normal,
            ),
            ..TextLayoutRequest::for_text("AlphaBeta", DEFAULT_TEXT_FONT_SIZE_PX)
        };
        let break_word_request = TextLayoutRequest {
            text: "AlphaBeta",
            width_px: Some(constrained_width_px),
            wrap_mode: text_wrap_mode_for_css_layout(
                DomWhiteSpace::Normal,
                DomOverflowWrap::BreakWord,
                DomWordBreak::Normal,
            ),
            ..TextLayoutRequest::for_text("AlphaBeta", DEFAULT_TEXT_FONT_SIZE_PX)
        };

        let normal_layout = context.layout_wrapped_text(&normal_request);
        let break_word_layout = context.layout_wrapped_text(&break_word_request);

        assert_eq!(
            normal_layout.line_count, 1,
            "normal word wrapping should keep an unbreakable ASCII word on one overflowing line",
        );
        assert!(
            break_word_layout.line_count > 1,
            "break-word should enable emergency glyph fallback after the normal word wrap path overflows",
        );
    }

    #[test]
    fn anywhere_changes_min_content_width_but_break_word_does_not() {
        let mut context = default_context();
        let break_word_request = TextLayoutRequest {
            wrap_mode: text_wrap_mode_for_css_intrinsic(
                DomWhiteSpace::Normal,
                DomOverflowWrap::BreakWord,
                DomWordBreak::Normal,
            ),
            ..TextLayoutRequest::for_text("AlphaBeta", DEFAULT_TEXT_FONT_SIZE_PX)
        };
        let anywhere_request = TextLayoutRequest {
            wrap_mode: text_wrap_mode_for_css_intrinsic(
                DomWhiteSpace::Normal,
                DomOverflowWrap::Anywhere,
                DomWordBreak::Normal,
            ),
            ..TextLayoutRequest::for_text("AlphaBeta", DEFAULT_TEXT_FONT_SIZE_PX)
        };

        let break_word_intrinsic = context.measure_intrinsic(&break_word_request);
        let anywhere_intrinsic = context.measure_intrinsic(&anywhere_request);

        assert!(
            anywhere_intrinsic.min_content_width_px < break_word_intrinsic.min_content_width_px,
            "Blink and Gecko treat anywhere as a real min-content break opportunity, while break-word stays on the normal word-wrap min-content path",
        );
    }

    #[test]
    fn caret_map_builds_non_empty_lines_for_wrapped_text() {
        let mut context = default_context();
        let request = TextLayoutRequest {
            text: "Runtime Diagnostics Menu",
            width_px: Some(64.0),
            ..TextLayoutRequest::for_text("Runtime Diagnostics Menu", DEFAULT_TEXT_FONT_SIZE_PX)
        };

        let caret_map = context.layout_text_caret_map(&request);

        assert!(caret_map.lines.len() > 1);
        assert_non_empty_caret_lines(&caret_map);
    }

    #[test]
    fn caret_point_lookup_uses_nearest_stop_on_nearest_line() {
        let mut context = default_context();
        let request = TextLayoutRequest {
            text: "Session Diagnostics",
            width_px: Some(56.0),
            ..TextLayoutRequest::for_text("Session Diagnostics", DEFAULT_TEXT_FONT_SIZE_PX)
        };
        let caret_map = context.layout_text_caret_map(&request);
        let target_line = &caret_map.lines[1];
        let target_stop = target_line
            .stops
            .last()
            .expect("wrapped text line should expose a last stop");

        let byte_index = context.layout_text_caret_stop_byte_index_at_point(
            &request,
            target_stop.x_px,
            target_line.top_px + (target_line.height_px / 2.0),
        );

        assert_eq!(byte_index, target_stop.byte_index);
    }

    #[test]
    fn single_word_intrinsic_is_a_single_fragment() {
        let mut context = default_context();
        let request = TextLayoutRequest::for_text("hello", DEFAULT_TEXT_FONT_SIZE_PX);

        let intrinsic = context.measure_intrinsic(&request);

        assert_eq!(
            intrinsic.leading_fragment_width_px, intrinsic.min_content_width_px,
            "a single word has one fragment, so leading equals min-content"
        );
        assert_eq!(
            intrinsic.trailing_fragment_width_px, intrinsic.min_content_width_px,
            "a single word has one fragment, so trailing equals min-content"
        );
        assert_eq!(
            intrinsic.min_content_width_px, intrinsic.max_content_width_px,
            "a single word never wraps, so min-content equals max-content"
        );
    }

    #[test]
    fn multi_word_intrinsic_splits_fragments_at_soft_wrap_opportunities() {
        let mut context = default_context();
        let request = TextLayoutRequest::for_text("aa bbb cc", DEFAULT_TEXT_FONT_SIZE_PX);

        let intrinsic = context.measure_intrinsic(&request);
        let aa_width = measure_word_width(&mut context, "aa");
        let bbb_width = measure_word_width(&mut context, "bbb");
        let cc_width = measure_word_width(&mut context, "cc");

        assert_eq!(
            intrinsic.leading_fragment_width_px, aa_width,
            "the leading fragment is the first word"
        );
        assert_eq!(
            intrinsic.trailing_fragment_width_px, cc_width,
            "the trailing fragment is the last word"
        );
        assert_eq!(
            intrinsic.min_content_width_px, bbb_width,
            "min-content is the widest fragment (the middle word)"
        );
    }

    #[test]
    fn collapse_mode_flags_boundary_wrap_opportunities_for_surrounding_spaces() {
        let mut context = default_context();
        let request = TextLayoutRequest::for_text_with_white_space(
            " aa ",
            DEFAULT_TEXT_FONT_SIZE_PX,
            DomWhiteSpace::Normal,
        );

        let intrinsic = context.measure_intrinsic(&request);

        assert!(
            intrinsic.starts_with_wrap_opportunity,
            "a collapsed leading space is a boundary wrap opportunity"
        );
        assert!(
            intrinsic.ends_with_wrap_opportunity,
            "a collapsed trailing space is a boundary wrap opportunity"
        );
    }

    #[test]
    fn preserve_mode_does_not_flag_boundary_wrap_opportunities_for_surrounding_spaces() {
        let mut context = default_context();
        let request = TextLayoutRequest::for_text_with_white_space(
            " aa ",
            DEFAULT_TEXT_FONT_SIZE_PX,
            DomWhiteSpace::PreWrap,
        );

        let intrinsic = context.measure_intrinsic(&request);

        assert!(
            !intrinsic.starts_with_wrap_opportunity,
            "pre-wrap preserves the leading space, so the boundary is not a collapse break"
        );
        assert!(
            !intrinsic.ends_with_wrap_opportunity,
            "pre-wrap preserves the trailing space, so the boundary is not a collapse break"
        );
    }

    #[test]
    fn nowrap_mode_is_a_single_fragment_with_no_internal_wrap_opportunities() {
        let mut context = default_context();
        let request = TextLayoutRequest::for_text_with_white_space(
            "aa bbb cc",
            DEFAULT_TEXT_FONT_SIZE_PX,
            DomWhiteSpace::Nowrap,
        );

        let intrinsic = context.measure_intrinsic(&request);

        assert_eq!(
            intrinsic.leading_fragment_width_px, intrinsic.max_content_width_px,
            "nowrap suppresses wrapping, so the whole run is one fragment"
        );
        assert_eq!(
            intrinsic.trailing_fragment_width_px, intrinsic.max_content_width_px,
            "nowrap suppresses wrapping, so the whole run is one fragment"
        );
        assert_eq!(
            intrinsic.min_content_width_px, intrinsic.max_content_width_px,
            "nowrap exposes no internal wrap opportunities"
        );
        assert!(
            !intrinsic.starts_with_wrap_opportunity,
            "no boundary whitespace, so no boundary wrap opportunity"
        );
        assert!(
            !intrinsic.ends_with_wrap_opportunity,
            "no boundary whitespace, so no boundary wrap opportunity"
        );
    }

    #[test]
    fn measure_intrinsic_zero_font_size_returns_zero_widths() {
        let mut context = default_context();
        let request = TextLayoutRequest {
            font_size_px: 0.0,
            line_height_px: Some(20.0),
            ..TextLayoutRequest::for_text("hello", 0.0)
        };

        let intrinsic = context.measure_intrinsic(&request);

        assert_eq!(intrinsic.min_content_width_px, 0.0);
        assert_eq!(intrinsic.max_content_width_px, 0.0);
        assert_eq!(intrinsic.line_height_px, 20.0);
    }

    #[test]
    fn measure_wrapped_text_height_zero_font_size_is_one_line() {
        let mut context = default_context();
        let with_line_height = TextLayoutRequest {
            font_size_px: 0.0,
            line_height_px: Some(20.0),
            ..TextLayoutRequest::for_text("hello", 0.0)
        };
        let without_line_height = TextLayoutRequest::for_text("hello", 0.0);

        assert_eq!(context.measure_wrapped_text_height(&with_line_height), 20.0);
        assert_eq!(
            context.measure_wrapped_text_height(&without_line_height),
            1.0
        );
    }

    #[test]
    fn layout_wrapped_text_zero_font_size_is_one_zero_width_line() {
        let mut context = default_context();
        let request = TextLayoutRequest {
            font_size_px: 0.0,
            line_height_px: Some(20.0),
            ..TextLayoutRequest::for_text("hello", 0.0)
        };

        let wrapped_text = context.layout_wrapped_text(&request);

        assert_eq!(wrapped_text.line_count, 1);
        assert_eq!(wrapped_text.width_px, 0.0);
        assert_eq!(wrapped_text.height_px, 20.0);
        assert_eq!(wrapped_text.lines.len(), 1);
    }

    #[test]
    fn layout_text_caret_zero_font_size_is_origin() {
        let mut context = default_context();
        let request = TextLayoutRequest {
            font_size_px: 0.0,
            line_height_px: Some(20.0),
            ..TextLayoutRequest::for_text("hello", 0.0)
        };

        let caret = context.layout_text_caret(&request, 3);

        assert_eq!(caret.x_px, 0.0);
        assert_eq!(caret.y_px, 0.0);
        assert_eq!(caret.height_px, 20.0);
    }

    #[test]
    fn layout_text_caret_map_zero_font_size_has_one_line_per_source_line() {
        let mut context = default_context();
        let request = TextLayoutRequest {
            font_size_px: 0.0,
            line_height_px: Some(20.0),
            white_space: DomWhiteSpace::Pre,
            ..TextLayoutRequest::for_text("a\nb", 0.0)
        };

        let caret_map = context.layout_text_caret_map(&request);

        assert_eq!(caret_map.lines.len(), 2);
        assert_eq!(caret_map.lines[0].top_px, 0.0);
        assert_eq!(caret_map.lines[1].top_px, 20.0);
        for line in &caret_map.lines {
            assert_eq!(line.stops.len(), 1);
            assert_eq!(line.stops[0].x_px, 0.0);
        }
        assert_eq!(caret_map.lines[0].stops[0].byte_index, 0);
        assert_eq!(caret_map.lines[1].stops[0].byte_index, 2);
    }

    #[test]
    fn zero_size_caret_map_counts_lone_cr_as_segment_break() {
        let mut context = default_context();
        let request = TextLayoutRequest {
            font_size_px: 0.0,
            line_height_px: Some(20.0),
            white_space: DomWhiteSpace::Pre,
            ..TextLayoutRequest::for_text("a\rb", 0.0)
        };

        let caret_map = context.layout_text_caret_map(&request);

        assert_eq!(caret_map.lines.len(), 2);
        assert_eq!(caret_map.lines[1].stops[0].byte_index, 2);
    }

    #[test]
    fn zero_size_multiline_collapsing_modes_render_one_line() {
        let mut context = default_context();
        let request = TextLayoutRequest {
            font_size_px: 0.0,
            line_height_px: Some(20.0),
            white_space: DomWhiteSpace::Normal,
            ..TextLayoutRequest::for_text("a\nb", 0.0)
        };

        let wrapped_text = context.layout_wrapped_text(&request);
        let caret_map = context.layout_text_caret_map(&request);

        assert_eq!(wrapped_text.line_count, 1);
        assert_eq!(context.measure_wrapped_text_height(&request), 20.0);
        assert_eq!(caret_map.lines.len(), 1);
    }

    #[test]
    fn zero_size_multiline_preserving_modes_render_one_line_per_source_line() {
        let mut context = default_context();
        let request = TextLayoutRequest {
            font_size_px: 0.0,
            line_height_px: Some(20.0),
            white_space: DomWhiteSpace::Pre,
            ..TextLayoutRequest::for_text("a\nb", 0.0)
        };

        let wrapped_text = context.layout_wrapped_text(&request);
        let caret_map = context.layout_text_caret_map(&request);

        assert_eq!(wrapped_text.line_count, 2);
        assert_eq!(context.measure_wrapped_text_height(&request), 40.0);
        assert_eq!(caret_map.lines.len(), 2);
        assert_eq!(caret_map.lines[1].top_px, 20.0);
        assert_eq!(caret_map.lines[1].stops[0].byte_index, 2);
    }

    #[test]
    fn caret_stop_index_zero_font_size_is_text_start() {
        let mut context = default_context();
        let request = TextLayoutRequest {
            font_size_px: 0.0,
            line_height_px: Some(20.0),
            ..TextLayoutRequest::for_text("hello", 0.0)
        };

        let byte_index = context.layout_text_caret_stop_byte_index_at_point(&request, 50.0, 30.0);

        assert_eq!(byte_index, 0);
    }
}
