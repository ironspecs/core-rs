use std::borrow::Cow;

use crate::types::{DomAttributeValue, DomError};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum DomTextAlign {
    #[default]
    Start,
    Left,
    Center,
    Right,
    Justified,
    End,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum DomTextDirection {
    #[default]
    Ltr,
    Rtl,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum DomOverflowWrap {
    #[default]
    Normal,
    BreakWord,
    Anywhere,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum DomWordBreak {
    #[default]
    Normal,
    BreakAll,
    BreakWord,
    KeepAll,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum DomWhiteSpace {
    #[default]
    Normal,
    Nowrap,
    Pre,
    PreWrap,
    PreLine,
    BreakSpaces,
}

pub(crate) fn parse_white_space_keyword(value: &str) -> Option<DomWhiteSpace> {
    match value {
        "normal" => Some(DomWhiteSpace::Normal),
        "nowrap" => Some(DomWhiteSpace::Nowrap),
        "pre" => Some(DomWhiteSpace::Pre),
        "pre-wrap" => Some(DomWhiteSpace::PreWrap),
        "pre-line" => Some(DomWhiteSpace::PreLine),
        "break-spaces" => Some(DomWhiteSpace::BreakSpaces),
        _ => None,
    }
}

pub(crate) fn parse_text_align_keyword(value: &str) -> Option<DomTextAlign> {
    match value {
        "left" => Some(DomTextAlign::Left),
        "start" => Some(DomTextAlign::Start),
        "center" => Some(DomTextAlign::Center),
        "right" => Some(DomTextAlign::Right),
        "justify" => Some(DomTextAlign::Justified),
        "end" => Some(DomTextAlign::End),
        _ => None,
    }
}

pub(crate) fn parse_overflow_wrap_keyword(value: &str) -> Option<DomOverflowWrap> {
    match value {
        "normal" => Some(DomOverflowWrap::Normal),
        "break-word" => Some(DomOverflowWrap::BreakWord),
        "anywhere" => Some(DomOverflowWrap::Anywhere),
        _ => None,
    }
}

pub(crate) fn parse_word_break_keyword(value: &str) -> Option<DomWordBreak> {
    match value {
        "normal" => Some(DomWordBreak::Normal),
        "break-all" => Some(DomWordBreak::BreakAll),
        "break-word" => Some(DomWordBreak::BreakWord),
        "keep-all" => Some(DomWordBreak::KeepAll),
        _ => None,
    }
}

pub(crate) fn parse_text_direction_keyword(value: &str) -> Option<DomTextDirection> {
    match value {
        "ltr" => Some(DomTextDirection::Ltr),
        "rtl" => Some(DomTextDirection::Rtl),
        _ => None,
    }
}

pub(crate) fn text_align_from_attr_value(
    value: &DomAttributeValue,
) -> Result<DomTextAlign, DomError> {
    let DomAttributeValue::String(value) = value else {
        return Err(DomError::InvalidAttributeValue);
    };
    parse_text_align_keyword(value).ok_or(DomError::InvalidAttributeValue)
}

pub(crate) fn white_space_from_attr_value(
    value: &DomAttributeValue,
) -> Result<DomWhiteSpace, DomError> {
    let DomAttributeValue::String(value) = value else {
        return Err(DomError::InvalidAttributeValue);
    };
    parse_white_space_keyword(value).ok_or(DomError::InvalidAttributeValue)
}

pub(crate) fn text_direction_from_attr_value(
    value: &DomAttributeValue,
) -> Result<DomTextDirection, DomError> {
    let DomAttributeValue::String(value) = value else {
        return Err(DomError::InvalidAttributeValue);
    };
    parse_text_direction_keyword(value).ok_or(DomError::InvalidAttributeValue)
}

pub(crate) fn overflow_wrap_from_attr_value(
    value: &DomAttributeValue,
) -> Result<DomOverflowWrap, DomError> {
    let DomAttributeValue::String(value) = value else {
        return Err(DomError::InvalidAttributeValue);
    };
    parse_overflow_wrap_keyword(value).ok_or(DomError::InvalidAttributeValue)
}

pub(crate) fn word_break_from_attr_value(
    value: &DomAttributeValue,
) -> Result<DomWordBreak, DomError> {
    let DomAttributeValue::String(value) = value else {
        return Err(DomError::InvalidAttributeValue);
    };
    parse_word_break_keyword(value).ok_or(DomError::InvalidAttributeValue)
}

#[must_use]
pub(crate) fn text_layout_input_for_white_space(
    text: &str,
    white_space: DomWhiteSpace,
) -> Cow<'_, str> {
    match white_space {
        DomWhiteSpace::Normal | DomWhiteSpace::Nowrap => collapse_all_ascii_whitespace(text),
        DomWhiteSpace::Pre | DomWhiteSpace::PreWrap | DomWhiteSpace::BreakSpaces => {
            preserve_space_run_advances(text)
        }
        DomWhiteSpace::PreLine => collapse_spaces_preserving_segment_breaks(text),
    }
}

#[must_use]
pub(crate) fn text_layout_input_and_byte_map_for_white_space(
    text: &str,
    white_space: DomWhiteSpace,
) -> (Cow<'_, str>, Vec<usize>) {
    match white_space {
        DomWhiteSpace::Normal | DomWhiteSpace::Nowrap => {
            collapse_all_ascii_whitespace_with_byte_map(text)
        }
        DomWhiteSpace::Pre | DomWhiteSpace::PreWrap | DomWhiteSpace::BreakSpaces => {
            preserve_space_run_advances_with_byte_map(text)
        }
        DomWhiteSpace::PreLine => collapse_spaces_preserving_segment_breaks_with_byte_map(text),
    }
}

fn collapse_all_ascii_whitespace(text: &str) -> Cow<'_, str> {
    if text_has_no_collapsible_ascii_whitespace(text) {
        return Cow::Borrowed(text);
    }
    let mut collapsed = String::with_capacity(text.len());
    let mut previous_was_space = false;
    for character in text.chars() {
        if is_collapsible_ascii_whitespace(character) {
            if !previous_was_space {
                collapsed.push(' ');
                previous_was_space = true;
            }
            continue;
        }
        collapsed.push(character);
        previous_was_space = false;
    }
    Cow::Owned(collapsed)
}

fn collapse_all_ascii_whitespace_with_byte_map(text: &str) -> (Cow<'_, str>, Vec<usize>) {
    if text_has_no_collapsible_ascii_whitespace(text) {
        return (Cow::Borrowed(text), identity_text_byte_map(text));
    }
    let mut collapsed = String::with_capacity(text.len());
    let mut transformed_to_original_byte = vec![0usize];
    let mut previous_was_space = false;
    for (byte_index, character) in text.char_indices() {
        let next_byte_index = byte_index + character.len_utf8();
        if is_collapsible_ascii_whitespace(character) {
            if previous_was_space {
                set_mapped_end_boundary(
                    &mut transformed_to_original_byte,
                    collapsed.len(),
                    next_byte_index,
                );
            } else {
                push_mapped_char(
                    &mut collapsed,
                    &mut transformed_to_original_byte,
                    ' ',
                    byte_index,
                    next_byte_index,
                );
                previous_was_space = true;
            }
            continue;
        }
        push_mapped_char(
            &mut collapsed,
            &mut transformed_to_original_byte,
            character,
            byte_index,
            next_byte_index,
        );
        previous_was_space = false;
    }
    (Cow::Owned(collapsed), transformed_to_original_byte)
}

fn collapse_spaces_preserving_segment_breaks(text: &str) -> Cow<'_, str> {
    if text_has_no_pre_line_collapsible_ascii_whitespace(text) {
        return Cow::Borrowed(text);
    }
    let mut collapsed = String::with_capacity(text.len());
    let mut previous_was_space = false;
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\r' => {
                if matches!(characters.peek(), Some('\n')) {
                    characters.next();
                }
                collapsed.push('\n');
                previous_was_space = false;
            }
            '\n' => {
                collapsed.push('\n');
                previous_was_space = false;
            }
            character if is_collapsible_space_without_segment_break(character) => {
                if !previous_was_space {
                    collapsed.push(' ');
                    previous_was_space = true;
                }
            }
            character => {
                collapsed.push(character);
                previous_was_space = false;
            }
        }
    }
    Cow::Owned(collapsed)
}

fn collapse_spaces_preserving_segment_breaks_with_byte_map(
    text: &str,
) -> (Cow<'_, str>, Vec<usize>) {
    if text_has_no_pre_line_collapsible_ascii_whitespace(text) {
        return (Cow::Borrowed(text), identity_text_byte_map(text));
    }
    let mut collapsed = String::with_capacity(text.len());
    let mut transformed_to_original_byte = vec![0usize];
    let mut previous_was_space = false;
    let mut characters = text.char_indices().peekable();
    while let Some((byte_index, character)) = characters.next() {
        match character {
            '\r' => {
                let mut next_byte_index = byte_index + character.len_utf8();
                if let Some((line_feed_byte_index, '\n')) = characters.peek().copied() {
                    next_byte_index = line_feed_byte_index + '\n'.len_utf8();
                    characters.next();
                }
                push_mapped_char(
                    &mut collapsed,
                    &mut transformed_to_original_byte,
                    '\n',
                    byte_index,
                    next_byte_index,
                );
                previous_was_space = false;
            }
            '\n' => {
                let next_byte_index = byte_index + character.len_utf8();
                push_mapped_char(
                    &mut collapsed,
                    &mut transformed_to_original_byte,
                    '\n',
                    byte_index,
                    next_byte_index,
                );
                previous_was_space = false;
            }
            character if is_collapsible_space_without_segment_break(character) => {
                let next_byte_index = byte_index + character.len_utf8();
                if previous_was_space {
                    set_mapped_end_boundary(
                        &mut transformed_to_original_byte,
                        collapsed.len(),
                        next_byte_index,
                    );
                } else {
                    push_mapped_char(
                        &mut collapsed,
                        &mut transformed_to_original_byte,
                        ' ',
                        byte_index,
                        next_byte_index,
                    );
                    previous_was_space = true;
                }
            }
            character => {
                let next_byte_index = byte_index + character.len_utf8();
                push_mapped_char(
                    &mut collapsed,
                    &mut transformed_to_original_byte,
                    character,
                    byte_index,
                    next_byte_index,
                );
                previous_was_space = false;
            }
        }
    }
    (Cow::Owned(collapsed), transformed_to_original_byte)
}

fn preserve_space_run_advances(text: &str) -> Cow<'_, str> {
    if !text_has_repeated_ascii_spaces(text) {
        return Cow::Borrowed(text);
    }
    let mut preserved = String::with_capacity(text.len());
    let mut previous_was_space = false;
    for character in text.chars() {
        if character == ' ' {
            if previous_was_space {
                // cosmic_text collapses repeated ASCII spaces during shaping;
                // NBSP preserves the authored advance for CSS preserve modes.
                preserved.push('\u{00a0}');
            } else {
                preserved.push(' ');
                previous_was_space = true;
            }
            continue;
        }
        preserved.push(character);
        previous_was_space = false;
    }
    Cow::Owned(preserved)
}

fn preserve_space_run_advances_with_byte_map(text: &str) -> (Cow<'_, str>, Vec<usize>) {
    if !text_has_repeated_ascii_spaces(text) {
        return (Cow::Borrowed(text), identity_text_byte_map(text));
    }
    let mut preserved = String::with_capacity(text.len());
    let mut transformed_to_original_byte = vec![0usize];
    let mut previous_was_space = false;
    for (byte_index, character) in text.char_indices() {
        let next_byte_index = byte_index + character.len_utf8();
        if character == ' ' {
            let transformed_character = if previous_was_space {
                '\u{00a0}'
            } else {
                previous_was_space = true;
                ' '
            };
            push_mapped_char(
                &mut preserved,
                &mut transformed_to_original_byte,
                transformed_character,
                byte_index,
                next_byte_index,
            );
            continue;
        }
        push_mapped_char(
            &mut preserved,
            &mut transformed_to_original_byte,
            character,
            byte_index,
            next_byte_index,
        );
        previous_was_space = false;
    }
    (Cow::Owned(preserved), transformed_to_original_byte)
}

fn identity_text_byte_map(text: &str) -> Vec<usize> {
    let mut transformed_to_original_byte = vec![0usize; text.len() + 1];
    for (byte_index, mapped_byte_index) in transformed_to_original_byte.iter_mut().enumerate() {
        *mapped_byte_index = byte_index;
    }
    transformed_to_original_byte
}

fn push_mapped_char(
    transformed: &mut String,
    transformed_to_original_byte: &mut Vec<usize>,
    character: char,
    original_start_byte_index: usize,
    original_end_byte_index: usize,
) {
    let transformed_start_byte_index = transformed.len();
    set_mapped_start_boundary(
        transformed_to_original_byte,
        transformed_start_byte_index,
        original_start_byte_index,
    );
    transformed.push(character);
    set_mapped_end_boundary(
        transformed_to_original_byte,
        transformed.len(),
        original_end_byte_index,
    );
}

fn set_mapped_start_boundary(
    transformed_to_original_byte: &mut Vec<usize>,
    transformed_start_byte_index: usize,
    original_start_byte_index: usize,
) {
    if transformed_to_original_byte.len() <= transformed_start_byte_index {
        transformed_to_original_byte
            .resize(transformed_start_byte_index + 1, original_start_byte_index);
    }
    transformed_to_original_byte[transformed_start_byte_index] = original_start_byte_index;
}

fn set_mapped_end_boundary(
    transformed_to_original_byte: &mut Vec<usize>,
    transformed_end_byte_index: usize,
    original_end_byte_index: usize,
) {
    if transformed_to_original_byte.len() <= transformed_end_byte_index {
        transformed_to_original_byte
            .resize(transformed_end_byte_index + 1, original_end_byte_index);
    }
    transformed_to_original_byte[transformed_end_byte_index] = original_end_byte_index;
}

fn text_has_repeated_ascii_spaces(text: &str) -> bool {
    let mut previous_was_space = false;
    for character in text.chars() {
        if character == ' ' {
            if previous_was_space {
                return true;
            }
            previous_was_space = true;
            continue;
        }
        previous_was_space = false;
    }
    false
}

fn text_has_no_collapsible_ascii_whitespace(text: &str) -> bool {
    let mut previous_was_space = false;
    for character in text.chars() {
        if is_collapsible_ascii_whitespace(character) {
            if character != ' ' || previous_was_space {
                return false;
            }
            previous_was_space = true;
            continue;
        }
        previous_was_space = false;
    }
    true
}

fn text_has_no_pre_line_collapsible_ascii_whitespace(text: &str) -> bool {
    let mut previous_was_space = false;
    for character in text.chars() {
        match character {
            '\r' => return false,
            '\n' => previous_was_space = false,
            character if is_collapsible_space_without_segment_break(character) => {
                if character != ' ' || previous_was_space {
                    return false;
                }
                previous_was_space = true;
            }
            _ => previous_was_space = false,
        }
    }
    true
}

fn is_collapsible_ascii_whitespace(character: char) -> bool {
    matches!(character, ' ' | '\t' | '\n' | '\r' | '\u{000c}')
}

fn is_collapsible_space_without_segment_break(character: char) -> bool {
    matches!(character, ' ' | '\t' | '\u{000c}')
}
