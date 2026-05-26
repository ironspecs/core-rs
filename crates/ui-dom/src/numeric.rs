#![allow(dead_code)]

use num_traits::ToPrimitive;

#[must_use]
pub(crate) fn u32_to_usize(value: u32) -> usize {
    usize::try_from(value).expect("u32 should always fit into usize on supported targets")
}

#[must_use]
pub(crate) fn u16_to_u8(value: u16) -> u8 {
    u8::try_from(value).expect("u16 channel should be clamped to 0..=255")
}

#[must_use]
pub(crate) fn u32_to_u16(value: u32) -> u16 {
    u16::try_from(value).expect("u32 glyph id should fit into u16 for swash rendering")
}

#[must_use]
pub(crate) fn u32_to_f32(value: u32) -> f32 {
    value
        .to_f32()
        .expect("u32 should convert to f32 for ui-dom math ranges")
}

#[must_use]
pub(crate) fn i32_to_f32(value: i32) -> f32 {
    value
        .to_f32()
        .expect("i32 should convert to f32 for ui-dom math ranges")
}

#[must_use]
pub(crate) fn usize_to_f32(value: usize) -> f32 {
    value
        .to_f32()
        .expect("usize should convert to f32 for ui-dom math ranges")
}

#[must_use]
pub(crate) fn f32_to_u32(value: f32) -> u32 {
    value
        .to_u32()
        .expect("f32 should be finite and in u32 range before conversion")
}

#[must_use]
pub(crate) fn u64_to_f32(value: u64) -> f32 {
    value
        .to_f32()
        .expect("u64 should convert to f32 for transition timing ranges")
}

#[must_use]
pub(crate) fn u64_to_f64(value: u64) -> f64 {
    value
        .to_f64()
        .expect("u64 should convert to f64 for ui-dom image diff ranges")
}

#[must_use]
pub(crate) fn i64_to_f32(value: i64) -> f32 {
    value
        .to_f32()
        .expect("i64 should convert to f32 for transition interpolation ranges")
}

#[must_use]
pub(crate) fn usize_to_f64(value: usize) -> f64 {
    value
        .to_f64()
        .expect("usize should convert to f64 for ui-dom math ranges")
}
