pub(crate) mod text_render;

use std::{
    borrow::Cow,
    cell::RefCell,
    sync::{Arc, OnceLock},
};

use cosmic_text::{
    Attrs, Buffer, FontSystem, LayoutRun, Metrics as CosmicMetrics, Shaping,
    fontdb::Source as FontSource,
};

use crate::types::{
    DomOverflowWrap, DomTextAlign, DomTextDirection, DomWhiteSpace, DomWordBreak,
    text_layout_input_and_byte_map_for_white_space, text_layout_input_for_white_space,
};

pub(crate) const DEFAULT_TEXT_FONT_SIZE_PX: f32 = 14.0;
const WORD_JOINER: char = '\u{2060}';
const ZERO_WIDTH_SPACE: char = '\u{200B}';
const SOFT_HYPHEN: char = '\u{00AD}';

const IBM_PLEX_SANS_BYTES: &[u8] =
    include_bytes!("../../../../../assets/fonts/vendor/ibm-plex/IBMPlexSans-Variable.ttf");
const SOURCE_HAN_SANS_J_BYTES: &[u8] =
    include_bytes!("../../../../../assets/fonts/vendor/source-han-sans/SourceHanSansJ-Regular.otf");
const SOURCE_HAN_SANS_SC_BYTES: &[u8] = include_bytes!(
    "../../../../../assets/fonts/vendor/source-han-sans/SourceHanSansSC-Regular.otf"
);
const SOURCE_HAN_SANS_K_BYTES: &[u8] =
    include_bytes!("../../../../../assets/fonts/vendor/source-han-sans/SourceHanSansK-Regular.otf");

thread_local! {
    static REPO_TEXT_LAYOUT_CONTEXT: RefCell<TextLayoutContext> =
        RefCell::new(TextLayoutContext::new(TextLayoutConfig::repo_default()));
}

static REPO_TEXT_FONT_CATALOG: OnceLock<TextFontCatalog> = OnceLock::new();

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum TextLanguageId {
    English,
    Japanese,
    SimplifiedChinese,
    Korean,
    Thai,
}

impl TextLanguageId {
    #[must_use]
    pub(crate) fn from_text_hint(text: &str) -> Self {
        if text
            .chars()
            .any(|character| ('\u{0e00}'..='\u{0e7f}').contains(&character))
        {
            return Self::Thai;
        }
        if text
            .chars()
            .any(|character| ('\u{ac00}'..='\u{d7af}').contains(&character))
        {
            return Self::Korean;
        }
        if text
            .chars()
            .any(|character| ('\u{3040}'..='\u{30ff}').contains(&character))
        {
            return Self::Japanese;
        }
        if text
            .chars()
            .any(|character| ('\u{4e00}'..='\u{9fff}').contains(&character))
        {
            return Self::SimplifiedChinese;
        }
        Self::English
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum TextFontFamilyId {
    UiSans,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum TextWrapMode {
    None,
    Word,
    Glyph,
    WordOrGlyph,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum TextAlign {
    Left,
    Center,
    Right,
    Justified,
}

#[derive(Clone, Debug)]
pub(crate) struct TextLayoutRequest<'a> {
    pub(crate) text: &'a str,
    pub(crate) white_space: DomWhiteSpace,
    pub(crate) word_break: DomWordBreak,
    pub(crate) font_family_id: TextFontFamilyId,
    pub(crate) language_hint_id: TextLanguageId,
    pub(crate) font_size_px: f32,
    pub(crate) line_height_px: Option<f32>,
    pub(crate) width_px: Option<f32>,
    pub(crate) wrap_mode: TextWrapMode,
    pub(crate) alignment: TextAlign,
}

impl<'a> TextLayoutRequest<'a> {
    #[must_use]
    pub(crate) fn for_text(text: &'a str, font_size_px: f32) -> Self {
        Self {
            text,
            white_space: DomWhiteSpace::Normal,
            word_break: DomWordBreak::Normal,
            font_family_id: TextFontFamilyId::UiSans,
            language_hint_id: TextLanguageId::from_text_hint(text),
            font_size_px,
            line_height_px: None,
            width_px: None,
            wrap_mode: TextWrapMode::Word,
            alignment: TextAlign::Left,
        }
    }

    #[must_use]
    pub(crate) fn for_text_with_white_space(
        text: &'a str,
        font_size_px: f32,
        white_space: DomWhiteSpace,
    ) -> Self {
        Self {
            white_space,
            word_break: DomWordBreak::Normal,
            wrap_mode: text_wrap_mode_for_css_layout(
                white_space,
                DomOverflowWrap::Normal,
                DomWordBreak::Normal,
            ),
            ..Self::for_text(text, font_size_px)
        }
    }
}

pub(crate) fn text_align_for_css_value(
    text_align: DomTextAlign,
    text_direction: DomTextDirection,
) -> TextAlign {
    // CSS Text defines start/end against the line box start/end sides, and
    // Blink resolves those keywords to physical left/right from direction
    // before computing line offsets.
    match text_align {
        DomTextAlign::Start => text_align_for_start(text_direction),
        DomTextAlign::Left => TextAlign::Left,
        DomTextAlign::Center => TextAlign::Center,
        DomTextAlign::Right => TextAlign::Right,
        DomTextAlign::Justified => TextAlign::Justified,
        DomTextAlign::End => text_align_for_end(text_direction),
    }
}

fn text_align_for_start(text_direction: DomTextDirection) -> TextAlign {
    match text_direction {
        DomTextDirection::Ltr => TextAlign::Left,
        DomTextDirection::Rtl => TextAlign::Right,
    }
}

fn text_align_for_end(text_direction: DomTextDirection) -> TextAlign {
    match text_direction {
        DomTextDirection::Ltr => TextAlign::Right,
        DomTextDirection::Rtl => TextAlign::Left,
    }
}

#[derive(Clone, Debug, PartialEq)]
#[allow(clippy::struct_field_names)]
pub(crate) struct TextIntrinsicMetrics {
    pub(crate) min_content_width_px: f32,
    pub(crate) max_content_width_px: f32,
    pub(crate) line_height_px: f32,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Debug, PartialEq)]
#[allow(clippy::struct_field_names)]
pub(crate) struct WrappedTextLineLayout {
    pub(crate) width_px: f32,
    pub(crate) baseline_y_px: f32,
    pub(crate) height_px: f32,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct WrappedTextLayout {
    pub(crate) width_px: f32,
    pub(crate) height_px: f32,
    pub(crate) line_count: usize,
    pub(crate) lines: Vec<WrappedTextLineLayout>,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CaretStop {
    pub(crate) byte_index: usize,
    pub(crate) x_px: f32,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TextCaretLine {
    pub(crate) top_px: f32,
    pub(crate) height_px: f32,
    pub(crate) stops: Vec<CaretStop>,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TextCaretMap {
    pub(crate) lines: Vec<TextCaretLine>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(clippy::struct_field_names)]
pub(crate) struct TextCaretLayout {
    pub(crate) x_px: f32,
    pub(crate) y_px: f32,
    pub(crate) height_px: f32,
}

#[derive(Clone, Debug)]
pub(crate) struct TextFontCatalog {
    ui_sans_font_sources: Vec<Arc<Vec<u8>>>,
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

#[derive(Clone, Debug)]
pub(crate) struct TextLayoutConfig {
    font_catalog: TextFontCatalog,
}

impl TextLayoutConfig {
    #[must_use]
    pub(crate) fn repo_default() -> Self {
        Self {
            font_catalog: TextFontCatalog::repo_default(),
        }
    }
}

pub(crate) struct TextLayoutContext {
    font_system: FontSystem,
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
        wrapped_text_layout_from_buffer(build_wrapped_text_buffer(&mut self.font_system, request))
    }

    #[must_use]
    pub(crate) fn measure_wrapped_text_height(&mut self, request: &TextLayoutRequest<'_>) -> f32 {
        wrapped_text_height_from_buffer(build_wrapped_text_buffer(&mut self.font_system, request))
    }

    #[must_use]
    pub(crate) fn layout_text_caret(
        &mut self,
        request: &TextLayoutRequest<'_>,
        scalar_offset: usize,
    ) -> TextCaretLayout {
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
        let (buffer, transformed_text, transformed_to_original_byte) =
            build_wrapped_text_buffer_and_byte_map(&mut self.font_system, request);
        let caret_map =
            text_caret_map_from_buffer(&transformed_text, &transformed_to_original_byte, &buffer);
        text_caret_stop_byte_index_from_map(&caret_map, x_px, y_px)
    }
}

pub(crate) fn with_repo_text_layout_context<R>(
    callback: impl FnOnce(&mut TextLayoutContext) -> R,
) -> R {
    REPO_TEXT_LAYOUT_CONTEXT.with_borrow_mut(callback)
}

#[must_use]
pub(crate) fn count_unicode_scalars(text: &str) -> usize {
    text.chars().count()
}

#[must_use]
pub(crate) fn text_caret_layout(
    request: &TextLayoutRequest<'_>,
    scalar_offset: usize,
) -> TextCaretLayout {
    with_repo_text_layout_context(|context| context.layout_text_caret(request, scalar_offset))
}

#[allow(dead_code)]
#[must_use]
pub(crate) fn text_caret_map(request: &TextLayoutRequest<'_>) -> TextCaretMap {
    with_repo_text_layout_context(|context| context.layout_text_caret_map(request))
}

#[allow(dead_code)]
#[must_use]
pub(crate) fn text_caret_stop_byte_index_at_point(
    request: &TextLayoutRequest<'_>,
    x_px: f32,
    y_px: f32,
) -> usize {
    with_repo_text_layout_context(|context| {
        context.layout_text_caret_stop_byte_index_at_point(request, x_px, y_px)
    })
}

pub(super) fn build_text_buffer(
    font_system: &mut FontSystem,
    request: &TextLayoutRequest<'_>,
    line_height_px: f32,
    width_px: Option<f32>,
    height_px: Option<f32>,
) -> Buffer {
    let mut buffer = Buffer::new(
        font_system,
        CosmicMetrics::new(request.font_size_px, line_height_px.max(1.0)),
    );
    let text = text_layout_input_for_wrap_policy(
        request.text,
        request.white_space,
        request.word_break,
        request.language_hint_id,
    );
    buffer.set_wrap(font_system, text_render::cosmic_wrap(request));
    buffer.set_size(font_system, width_px, height_px);
    buffer.set_text(
        font_system,
        text.as_ref(),
        &text_layout_attrs(request),
        Shaping::Advanced,
        Some(text_render::cosmic_align(request.alignment)),
    );
    buffer
}

#[must_use]
pub(crate) fn text_wrap_mode_for_css_layout(
    white_space: DomWhiteSpace,
    overflow_wrap: DomOverflowWrap,
    word_break: DomWordBreak,
) -> TextWrapMode {
    if white_space_suppresses_soft_wrap(white_space) {
        return TextWrapMode::None;
    }
    match word_break {
        DomWordBreak::BreakAll => TextWrapMode::Glyph,
        DomWordBreak::BreakWord => TextWrapMode::WordOrGlyph,
        DomWordBreak::Normal | DomWordBreak::KeepAll => match overflow_wrap {
            DomOverflowWrap::Normal => TextWrapMode::Word,
            DomOverflowWrap::BreakWord | DomOverflowWrap::Anywhere => TextWrapMode::WordOrGlyph,
        },
    }
}

pub(crate) fn text_wrap_mode_for_css_intrinsic(
    white_space: DomWhiteSpace,
    overflow_wrap: DomOverflowWrap,
    word_break: DomWordBreak,
) -> TextWrapMode {
    if white_space_suppresses_soft_wrap(white_space) {
        return TextWrapMode::None;
    }
    match word_break {
        DomWordBreak::BreakAll => TextWrapMode::Glyph,
        DomWordBreak::BreakWord => TextWrapMode::WordOrGlyph,
        DomWordBreak::Normal | DomWordBreak::KeepAll => match overflow_wrap {
            DomOverflowWrap::Anywhere => TextWrapMode::WordOrGlyph,
            DomOverflowWrap::Normal | DomOverflowWrap::BreakWord => TextWrapMode::Word,
        },
    }
}

fn white_space_suppresses_soft_wrap(white_space: DomWhiteSpace) -> bool {
    matches!(white_space, DomWhiteSpace::Nowrap | DomWhiteSpace::Pre)
}

fn build_wrapped_text_buffer(
    font_system: &mut FontSystem,
    request: &TextLayoutRequest<'_>,
) -> Buffer {
    let line_height_px = effective_line_height_px(font_system, request);
    build_text_buffer(
        font_system,
        request,
        line_height_px,
        request.width_px.map(|width_px| width_px.max(0.0)),
        None,
    )
}

fn build_wrapped_text_buffer_and_byte_map(
    font_system: &mut FontSystem,
    request: &TextLayoutRequest<'_>,
) -> (Buffer, String, Vec<usize>) {
    let line_height_px = effective_line_height_px(font_system, request);
    let (transformed_text, transformed_to_original_byte) =
        text_layout_input_and_byte_map_for_wrap_policy(
            request.text,
            request.white_space,
            request.word_break,
            request.language_hint_id,
        );
    let transformed_text = transformed_text.into_owned();
    let mut buffer = Buffer::new(
        font_system,
        CosmicMetrics::new(request.font_size_px, line_height_px.max(1.0)),
    );
    buffer.set_wrap(font_system, text_render::cosmic_wrap(request));
    buffer.set_size(
        font_system,
        request.width_px.map(|width_px| width_px.max(0.0)),
        None,
    );
    buffer.set_text(
        font_system,
        transformed_text.as_str(),
        &text_layout_attrs(request),
        Shaping::Advanced,
        Some(text_render::cosmic_align(request.alignment)),
    );
    (buffer, transformed_text, transformed_to_original_byte)
}

pub(crate) fn text_layout_input_for_wrap_policy(
    text: &str,
    white_space: DomWhiteSpace,
    word_break: DomWordBreak,
    language_hint_id: TextLanguageId,
) -> Cow<'_, str> {
    let white_space_text = text_layout_input_for_white_space(text, white_space);
    keep_all_text_layout_input(white_space_text, word_break, language_hint_id)
}

pub(crate) fn text_layout_input_and_byte_map_for_wrap_policy(
    text: &str,
    white_space: DomWhiteSpace,
    word_break: DomWordBreak,
    language_hint_id: TextLanguageId,
) -> (Cow<'_, str>, Vec<usize>) {
    let (white_space_text, transformed_to_original_byte) =
        text_layout_input_and_byte_map_for_white_space(text, white_space);
    keep_all_text_layout_input_and_byte_map(
        white_space_text,
        transformed_to_original_byte,
        word_break,
        language_hint_id,
    )
}

fn keep_all_text_layout_input(
    text: Cow<'_, str>,
    word_break: DomWordBreak,
    language_hint_id: TextLanguageId,
) -> Cow<'_, str> {
    if !keep_all_transform_is_active(word_break, language_hint_id) {
        return text;
    }
    let mut transformed = String::with_capacity(text.len() + 8);
    let mut previous_character = None;
    for character in text.chars() {
        if previous_character
            .is_some_and(|previous| should_insert_keep_all_joiner(previous, character))
        {
            transformed.push(WORD_JOINER);
        }
        transformed.push(character);
        previous_character = Some(character);
    }
    Cow::Owned(transformed)
}

fn keep_all_text_layout_input_and_byte_map(
    text: Cow<'_, str>,
    transformed_to_original_byte: Vec<usize>,
    word_break: DomWordBreak,
    language_hint_id: TextLanguageId,
) -> (Cow<'_, str>, Vec<usize>) {
    if !keep_all_transform_is_active(word_break, language_hint_id) {
        return (text, transformed_to_original_byte);
    }
    let mut transformed = String::with_capacity(text.len() + 8);
    let mut keep_all_to_original_byte = vec![0usize];
    let mut previous_character = None;
    for (byte_index, character) in text.char_indices() {
        let original_start_byte_index = transformed_to_original_byte[byte_index];
        let original_end_byte_index =
            transformed_to_original_byte[byte_index + character.len_utf8()];
        if previous_character
            .is_some_and(|previous| should_insert_keep_all_joiner(previous, character))
        {
            push_keep_all_mapped_char(
                &mut transformed,
                &mut keep_all_to_original_byte,
                WORD_JOINER,
                original_start_byte_index,
                original_start_byte_index,
            );
        }
        push_keep_all_mapped_char(
            &mut transformed,
            &mut keep_all_to_original_byte,
            character,
            original_start_byte_index,
            original_end_byte_index,
        );
        previous_character = Some(character);
    }
    (Cow::Owned(transformed), keep_all_to_original_byte)
}

fn keep_all_transform_is_active(
    word_break: DomWordBreak,
    language_hint_id: TextLanguageId,
) -> bool {
    matches!(word_break, DomWordBreak::KeepAll)
        && matches!(
            language_hint_id,
            TextLanguageId::Japanese | TextLanguageId::SimplifiedChinese | TextLanguageId::Korean
        )
}

fn should_insert_keep_all_joiner(previous: char, current: char) -> bool {
    is_keep_all_protected_character(previous) && is_keep_all_protected_character(current)
}

fn is_keep_all_protected_character(character: char) -> bool {
    if character.is_whitespace()
        || matches!(character, WORD_JOINER | ZERO_WIDTH_SPACE | SOFT_HYPHEN)
    {
        return false;
    }
    character.is_ascii_alphanumeric()
        || is_japanese_kana(character)
        || is_cjk_ideograph(character)
        || is_hangul_syllable(character)
}

fn is_japanese_kana(character: char) -> bool {
    matches!(character, '\u{3040}'..='\u{30ff}')
}

fn is_cjk_ideograph(character: char) -> bool {
    matches!(
        character,
        '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}' | '\u{f900}'..='\u{faff}'
    )
}

fn is_hangul_syllable(character: char) -> bool {
    matches!(character, '\u{ac00}'..='\u{d7af}')
}

fn push_keep_all_mapped_char(
    transformed: &mut String,
    transformed_to_original_byte: &mut Vec<usize>,
    character: char,
    original_start_byte_index: usize,
    original_end_byte_index: usize,
) {
    let transformed_start_byte_index = transformed.len();
    if transformed_to_original_byte.len() <= transformed_start_byte_index {
        transformed_to_original_byte
            .resize(transformed_start_byte_index + 1, original_start_byte_index);
    }
    transformed_to_original_byte[transformed_start_byte_index] = original_start_byte_index;
    transformed.push(character);
    if transformed_to_original_byte.len() <= transformed.len() {
        transformed_to_original_byte.resize(transformed.len() + 1, original_end_byte_index);
    }
    transformed_to_original_byte[transformed.len()] = original_end_byte_index;
}

fn text_caret_layout_from_buffer(
    text: &str,
    transformed_text: &str,
    transformed_to_original_byte: &[usize],
    scalar_offset: usize,
    buffer: &Buffer,
) -> TextCaretLayout {
    let caret_map =
        text_caret_map_from_buffer(transformed_text, transformed_to_original_byte, buffer);
    text_caret_layout_from_map(text, scalar_offset, &caret_map)
}

fn text_caret_map_from_buffer(
    transformed_text: &str,
    transformed_to_original_byte: &[usize],
    buffer: &Buffer,
) -> TextCaretMap {
    let line_start_byte_indices = line_start_byte_indices(transformed_text);
    let mut lines = Vec::new();
    for run in buffer.layout_runs() {
        let transformed_line_start_byte_index = *line_start_byte_indices
            .get(run.line_i)
            .expect("layout run line should stay within the source text");
        let stops = line_caret_stops(
            transformed_line_start_byte_index,
            transformed_to_original_byte,
            &run,
        );
        lines.push(TextCaretLine {
            top_px: run.line_top,
            height_px: run.line_height,
            stops,
        });
    }
    assert!(
        !lines.is_empty(),
        "text caret map should contain at least one laid out line",
    );
    TextCaretMap { lines }
}

fn line_start_byte_indices(text: &str) -> Vec<usize> {
    let mut line_start_byte_indices = vec![0usize];
    for (byte_index, character) in text.char_indices() {
        if character == '\n' {
            line_start_byte_indices.push(byte_index + character.len_utf8());
        }
    }
    line_start_byte_indices
}

fn line_caret_stops(
    transformed_line_start_byte_index: usize,
    transformed_to_original_byte: &[usize],
    run: &LayoutRun<'_>,
) -> Vec<CaretStop> {
    if run.glyphs.is_empty() {
        return vec![CaretStop {
            byte_index: transformed_byte_index_to_original_byte_index(
                transformed_to_original_byte,
                transformed_line_start_byte_index,
            ),
            x_px: 0.0,
        }];
    }
    let mut stops = Vec::with_capacity(run.glyphs.len() + 1);
    let first_glyph = run
        .glyphs
        .first()
        .expect("non-empty glyph run should expose a first glyph");
    push_caret_stop(
        &mut stops,
        transformed_byte_index_to_original_byte_index(
            transformed_to_original_byte,
            transformed_line_start_byte_index
                + glyph_left_boundary_byte_index(run.rtl, first_glyph),
        ),
        glyph_left_boundary_x_px(run.rtl, first_glyph),
    );
    for glyph in run.glyphs {
        push_caret_stop(
            &mut stops,
            transformed_byte_index_to_original_byte_index(
                transformed_to_original_byte,
                transformed_line_start_byte_index + glyph_right_boundary_byte_index(run.rtl, glyph),
            ),
            glyph_right_boundary_x_px(run.rtl, glyph),
        );
    }
    assert!(
        !stops.is_empty(),
        "laid out text line should expose at least one caret stop",
    );
    stops
}

fn push_caret_stop(stops: &mut Vec<CaretStop>, byte_index: usize, x_px: f32) {
    let stop = CaretStop { byte_index, x_px };
    if stops.last() == Some(&stop) {
        return;
    }
    stops.push(stop);
}

fn transformed_byte_index_to_original_byte_index(
    transformed_to_original_byte: &[usize],
    transformed_byte_index: usize,
) -> usize {
    *transformed_to_original_byte
        .get(transformed_byte_index)
        .expect("transformed text byte index should stay within the mapped white-space input")
}

fn glyph_left_boundary_byte_index(run_is_rtl: bool, glyph: &cosmic_text::LayoutGlyph) -> usize {
    if run_is_rtl { glyph.end } else { glyph.start }
}

fn glyph_right_boundary_byte_index(run_is_rtl: bool, glyph: &cosmic_text::LayoutGlyph) -> usize {
    if run_is_rtl { glyph.start } else { glyph.end }
}

fn glyph_left_boundary_x_px(run_is_rtl: bool, glyph: &cosmic_text::LayoutGlyph) -> f32 {
    if run_is_rtl {
        glyph.x + glyph.w
    } else {
        glyph.x
    }
}

fn glyph_right_boundary_x_px(run_is_rtl: bool, glyph: &cosmic_text::LayoutGlyph) -> f32 {
    if run_is_rtl {
        glyph.x
    } else {
        glyph.x + glyph.w
    }
}

fn text_caret_layout_from_map(
    text: &str,
    scalar_offset: usize,
    caret_map: &TextCaretMap,
) -> TextCaretLayout {
    let (line, stop) = nearest_caret_stop_for_scalar_offset(text, caret_map, scalar_offset);
    TextCaretLayout {
        x_px: stop.x_px,
        y_px: line.top_px,
        height_px: line.height_px,
    }
}

fn nearest_caret_stop_for_scalar_offset<'a>(
    text: &str,
    caret_map: &'a TextCaretMap,
    scalar_offset: usize,
) -> (&'a TextCaretLine, &'a CaretStop) {
    assert!(
        scalar_offset <= count_unicode_scalars(text),
        "text caret offset should stay within the current text",
    );
    let first_line = caret_map
        .lines
        .first()
        .expect("text caret map should contain at least one line");
    let first_stop = first_line
        .stops
        .first()
        .expect("text caret line should contain at least one stop");
    let first_stop_scalar_offset =
        unicode_scalar_offset_for_byte_index(text, first_stop.byte_index);
    let mut nearest_line = first_line;
    let mut nearest_stop = first_stop;
    let mut nearest_distance = scalar_offset.abs_diff(first_stop_scalar_offset);
    for line in &caret_map.lines {
        for stop in &line.stops {
            let candidate_scalar_offset =
                unicode_scalar_offset_for_byte_index(text, stop.byte_index);
            let candidate_distance = scalar_offset.abs_diff(candidate_scalar_offset);
            if candidate_distance < nearest_distance
                || (candidate_distance == 0 && stop.byte_index == nearest_stop.byte_index)
                || (candidate_distance == nearest_distance
                    && stop.byte_index < nearest_stop.byte_index)
            {
                nearest_line = line;
                nearest_stop = stop;
                nearest_distance = candidate_distance;
            }
        }
    }
    (nearest_line, nearest_stop)
}

fn unicode_scalar_offset_for_byte_index(text: &str, byte_index: usize) -> usize {
    assert!(
        byte_index <= text.len(),
        "text byte index should stay within the current text",
    );
    if byte_index == text.len() {
        return count_unicode_scalars(text);
    }
    for (scalar_offset, (character_byte_index, _)) in text.char_indices().enumerate() {
        if character_byte_index == byte_index {
            return scalar_offset;
        }
    }
    panic!("text byte index should stay aligned to a unicode scalar boundary");
}

fn text_caret_stop_byte_index_from_map(caret_map: &TextCaretMap, x_px: f32, y_px: f32) -> usize {
    nearest_caret_stop_for_point(caret_map, x_px, y_px).byte_index
}

fn nearest_caret_stop_for_point(caret_map: &TextCaretMap, x_px: f32, y_px: f32) -> &CaretStop {
    let nearest_line = nearest_caret_line_for_y(caret_map, y_px);
    nearest_stop_for_x(nearest_line, x_px)
}

fn nearest_caret_line_for_y(caret_map: &TextCaretMap, y_px: f32) -> &TextCaretLine {
    let first_line = caret_map
        .lines
        .first()
        .expect("text caret map should contain at least one line");
    let mut nearest_line = first_line;
    let mut nearest_distance = line_vertical_distance(first_line, y_px);
    for line in &caret_map.lines[1..] {
        let candidate_distance = line_vertical_distance(line, y_px);
        if candidate_distance < nearest_distance {
            nearest_line = line;
            nearest_distance = candidate_distance;
        }
    }
    nearest_line
}

fn line_vertical_distance(line: &TextCaretLine, y_px: f32) -> f32 {
    if y_px < line.top_px {
        return line.top_px - y_px;
    }
    let line_bottom_px = line.top_px + line.height_px;
    if y_px > line_bottom_px {
        return y_px - line_bottom_px;
    }
    0.0
}

fn nearest_stop_for_x(line: &TextCaretLine, x_px: f32) -> &CaretStop {
    let first_stop = line
        .stops
        .first()
        .expect("text caret line should contain at least one stop");
    let mut nearest_stop = first_stop;
    let mut nearest_distance = (x_px - first_stop.x_px).abs();
    for stop in &line.stops[1..] {
        let candidate_distance = (x_px - stop.x_px).abs();
        let candidate_order = candidate_distance.total_cmp(&nearest_distance);
        if candidate_order.is_lt()
            || (candidate_order.is_eq() && stop.byte_index < nearest_stop.byte_index)
        {
            nearest_stop = stop;
            nearest_distance = candidate_distance;
        }
    }
    nearest_stop
}

pub(super) fn effective_line_height_px(
    font_system: &mut FontSystem,
    request: &TextLayoutRequest<'_>,
) -> f32 {
    let _ = font_system;
    if let Some(line_height_px) = request.line_height_px {
        return line_height_px.max(1.0);
    }
    request.font_size_px.max(1.0)
}

fn text_layout_attrs(request: &TextLayoutRequest<'_>) -> Attrs<'static> {
    Attrs::new().family(text_render::preferred_repo_font_family(
        request.font_family_id,
        request.language_hint_id,
    ))
}

fn install_text_font_catalog(font_system: &mut FontSystem, font_catalog: TextFontCatalog) {
    for font_source in font_catalog.ui_sans_font_sources {
        font_system
            .db_mut()
            .load_font_source(FontSource::Binary(font_source));
    }
}

#[cfg_attr(not(test), allow(dead_code))]
#[allow(clippy::needless_pass_by_value)]
fn wrapped_text_layout_from_buffer(buffer: Buffer) -> WrappedTextLayout {
    let mut width_px: f32 = 0.0;
    let mut height_px: f32 = 0.0;
    let mut lines = Vec::new();
    for run in buffer.layout_runs() {
        width_px = width_px.max(run.line_w);
        height_px += run.line_height;
        lines.push(WrappedTextLineLayout {
            width_px: run.line_w,
            baseline_y_px: run.line_y,
            height_px: run.line_height,
        });
    }
    WrappedTextLayout {
        width_px,
        height_px,
        line_count: lines.len(),
        lines,
    }
}

#[allow(clippy::needless_pass_by_value)]
fn wrapped_text_height_from_buffer(buffer: Buffer) -> f32 {
    let mut height_px: f32 = 0.0;
    for run in buffer.layout_runs() {
        height_px += run.line_height;
    }
    height_px
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use std::collections::BTreeMap;

    use crate::types::{
        DomOverflowWrap, DomTextAlign, DomTextDirection, DomWhiteSpace, DomWordBreak,
    };

    use super::{
        DEFAULT_TEXT_FONT_SIZE_PX, TextCaretMap, TextLanguageId, TextLayoutContext,
        TextLayoutRequest, TextWrapMode, build_wrapped_text_buffer,
        build_wrapped_text_buffer_and_byte_map, text_caret_map_from_buffer,
        text_wrap_mode_for_css_intrinsic, text_wrap_mode_for_css_layout,
    };
    use cosmic_text::{Affinity, Buffer, Cursor};

    fn default_context() -> TextLayoutContext {
        TextLayoutContext::new(super::TextLayoutConfig::repo_default())
    }

    fn build_test_buffer(
        context: &mut TextLayoutContext,
        request: &TextLayoutRequest<'_>,
    ) -> Buffer {
        build_wrapped_text_buffer(&mut context.font_system, request)
    }

    fn assert_non_empty_caret_lines(caret_map: &TextCaretMap) {
        assert!(!caret_map.lines.is_empty());
        for line in &caret_map.lines {
            assert!(!line.stops.is_empty());
        }
    }

    #[test]
    fn text_align_start_resolves_to_right_in_rtl() {
        // CSS Text `start` follows the inline base direction, not physical
        // left. In RTL it resolves to the physical right side.
        assert_eq!(
            super::text_align_for_css_value(DomTextAlign::Start, DomTextDirection::Rtl),
            super::TextAlign::Right
        );
    }

    #[test]
    fn text_align_end_resolves_to_left_in_rtl() {
        // Blink maps `end` to physical left in RTL before line offset
        // calculation. This keeps `end` distinct from physical `right`.
        assert_eq!(
            super::text_align_for_css_value(DomTextAlign::End, DomTextDirection::Rtl),
            super::TextAlign::Left
        );
    }

    #[test]
    fn text_align_left_remains_physical_left_in_rtl() {
        assert_eq!(
            super::text_align_for_css_value(DomTextAlign::Left, DomTextDirection::Rtl),
            super::TextAlign::Left
        );
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
    fn caret_layout_roundtrips_wrapped_normalized_white_space_stop_positions() {
        let mut context = default_context();
        let request = TextLayoutRequest {
            text: "Alpha    Beta Gamma",
            width_px: Some(52.0),
            ..TextLayoutRequest::for_text_with_white_space(
                "Alpha    Beta Gamma",
                DEFAULT_TEXT_FONT_SIZE_PX,
                DomWhiteSpace::Normal,
            )
        };
        let caret_map = context.layout_text_caret_map(&request);

        for line in &caret_map.lines {
            for stop in &line.stops {
                let scalar_offset =
                    super::unicode_scalar_offset_for_byte_index(request.text, stop.byte_index);
                let caret = context.layout_text_caret(&request, scalar_offset);
                assert_eq!(caret.x_px, stop.x_px);
                assert_eq!(caret.y_px, line.top_px);
                assert_eq!(caret.height_px, line.height_px);
            }
        }
    }

    #[test]
    fn caret_layout_roundtrips_canonical_wrapped_break_spaces_stop_positions() {
        let mut context = default_context();
        let request = TextLayoutRequest {
            text: "Alpha   Beta   Gamma",
            width_px: Some(52.0),
            ..TextLayoutRequest::for_text_with_white_space(
                "Alpha   Beta   Gamma",
                DEFAULT_TEXT_FONT_SIZE_PX,
                DomWhiteSpace::BreakSpaces,
            )
        };
        let caret_map = context.layout_text_caret_map(&request);

        let mut canonical_stops_by_byte_index = BTreeMap::new();
        for line in &caret_map.lines {
            for stop in &line.stops {
                canonical_stops_by_byte_index.insert(stop.byte_index, (line, stop));
            }
        }
        for (byte_index, (line, stop)) in canonical_stops_by_byte_index {
            let scalar_offset =
                super::unicode_scalar_offset_for_byte_index(request.text, byte_index);
            let caret = context.layout_text_caret(&request, scalar_offset);
            assert_eq!(caret.x_px, stop.x_px);
            assert_eq!(caret.y_px, line.top_px);
            assert_eq!(caret.height_px, line.height_px);
        }
    }

    #[test]
    fn point_selected_break_spaces_wrapped_line_start_redraws_on_the_same_line() {
        let mut context = default_context();
        let request = TextLayoutRequest {
            text: "Alpha   Beta   Gamma",
            width_px: Some(52.0),
            ..TextLayoutRequest::for_text_with_white_space(
                "Alpha   Beta   Gamma",
                DEFAULT_TEXT_FONT_SIZE_PX,
                DomWhiteSpace::BreakSpaces,
            )
        };
        let caret_map = context.layout_text_caret_map(&request);
        let wrapped_line = &caret_map.lines[1];
        let wrapped_line_start_stop = wrapped_line
            .stops
            .first()
            .expect("wrapped break-spaces line should expose a first stop");

        let byte_index = context.layout_text_caret_stop_byte_index_at_point(
            &request,
            wrapped_line_start_stop.x_px,
            wrapped_line.top_px + (wrapped_line.height_px / 2.0),
        );
        let scalar_offset = super::unicode_scalar_offset_for_byte_index(request.text, byte_index);
        let caret = context.layout_text_caret(&request, scalar_offset);

        assert_eq!(byte_index, wrapped_line_start_stop.byte_index);
        assert_eq!(caret.x_px, wrapped_line_start_stop.x_px);
        assert_eq!(caret.y_px, wrapped_line.top_px);
        assert_eq!(caret.height_px, wrapped_line.height_px);
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
    fn caret_map_only_emits_exact_layout_cursor_boundaries() {
        let mut context = default_context();
        let request = TextLayoutRequest::for_text("A👩\u{200d}💻B", DEFAULT_TEXT_FONT_SIZE_PX);
        let (buffer, transformed_text, transformed_to_original_byte) =
            build_wrapped_text_buffer_and_byte_map(&mut context.font_system, &request);
        let caret_map =
            text_caret_map_from_buffer(&transformed_text, &transformed_to_original_byte, &buffer);
        let line_start_byte_indices = super::line_start_byte_indices(request.text);
        let runs: Vec<_> = buffer.layout_runs().collect();
        let mut cursor_buffer = build_test_buffer(&mut context, &request);

        assert_eq!(runs.len(), caret_map.lines.len());
        for (run, line) in runs.iter().zip(&caret_map.lines) {
            let line_start_byte_index = line_start_byte_indices[run.line_i];
            for stop in &line.stops {
                let line_byte_index = stop.byte_index - line_start_byte_index;
                let before_cursor =
                    Cursor::new_with_affinity(run.line_i, line_byte_index, Affinity::Before);
                let after_cursor =
                    Cursor::new_with_affinity(run.line_i, line_byte_index, Affinity::After);
                let before_is_boundary = cursor_buffer
                    .layout_cursor(&mut context.font_system, before_cursor)
                    .is_some();
                let after_is_boundary = cursor_buffer
                    .layout_cursor(&mut context.font_system, after_cursor)
                    .is_some();
                assert!(
                    before_is_boundary || after_is_boundary,
                    "caret stop should resolve to an exact layout cursor boundary",
                );
            }
        }
    }
}
