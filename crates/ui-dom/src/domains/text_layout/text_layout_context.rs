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
//! - the thread-local repo context is created once from
//!   `TextLayoutConfig::repo_default`

use cosmic_text::FontSystem;

use crate::numeric::usize_to_f32;

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
use crate::types::{DomOverflowWrap, DomWhiteSpace, DomWordBreak};

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
        if request.font_size_px == 0.0 {
            // Zero-size text is CSS-legal (CSS Fonts 4 §3.5); cosmic-text asserts a
            // non-zero font size, so measure without it: zero-width glyphs.
            return TextIntrinsicMetrics {
                min_content_width_px: 0.0,
                max_content_width_px: 0.0,
                line_height_px,
            };
        }
        let max_content_width_px =
            build_text_buffer(&mut self.font_system, request, line_height_px, None, None)
                .layout_runs()
                .fold(0.0f32, |width_px, run| width_px.max(run.line_w));
        let min_content_width_px = build_text_buffer(
            &mut self.font_system,
            request,
            line_height_px,
            Some(0.0),
            None,
        )
        .layout_runs()
        .fold(0.0f32, |width_px, run| width_px.max(run.line_w));
        TextIntrinsicMetrics {
            min_content_width_px,
            max_content_width_px,
            line_height_px,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    #[must_use]
    pub(crate) fn layout_wrapped_text(
        &mut self,
        request: &TextLayoutRequest<'_>,
    ) -> WrappedTextLayout {
        if request.font_size_px == 0.0 {
            // Same zero guard: zero-width text occupies exactly one line.
            let line_height_px = effective_line_height_px(&mut self.font_system, request);
            return WrappedTextLayout {
                width_px: 0.0,
                height_px: line_height_px,
                line_count: 1,
                lines: vec![WrappedTextLineLayout {
                    width_px: 0.0,
                    baseline_y_px: 0.0,
                    height_px: line_height_px,
                }],
            };
        }
        wrapped_text_layout_from_buffer(build_wrapped_text_buffer(&mut self.font_system, request))
    }

    #[must_use]
    pub(crate) fn measure_wrapped_text_height(&mut self, request: &TextLayoutRequest<'_>) -> f32 {
        if request.font_size_px == 0.0 {
            // Same zero guard: zero-width text occupies exactly one line.
            return effective_line_height_px(&mut self.font_system, request);
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

/// The caret map for zero-size text: one line per source line, every stop
/// at x = 0 (positions coincide without glyph data; cosmic-text asserts a
/// non-zero font size, so the map is built without it).
fn zero_size_caret_map(request: &TextLayoutRequest<'_>, line_height_px: f32) -> TextCaretMap {
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
