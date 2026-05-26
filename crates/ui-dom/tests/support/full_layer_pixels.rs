use crate::support::full_layer_geometry::RectPx;

pub fn assert_border_color_at_corners(
    rgba: &[u8],
    width_px: u32,
    label: &str,
    rect: RectPx,
    expected: [f32; 4],
) {
    for (corner, x_px, y_px) in [
        ("top-left", rect.left, rect.top),
        ("top-right", rect.left + rect.width - 1, rect.top),
        ("bottom-left", rect.left, rect.top + rect.height - 1),
        (
            "bottom-right",
            rect.left + rect.width - 1,
            rect.top + rect.height - 1,
        ),
    ] {
        assert_color_eq_at(
            rgba,
            width_px,
            u32_from_i32_size(x_px),
            u32_from_i32_size(y_px),
            expected,
            &format!("{label} {corner}"),
        );
    }
}

pub fn assert_fill_color_inside_box(
    rgba: &[u8],
    width_px: u32,
    label: &str,
    rect: RectPx,
    border_px: i32,
    expected: [f32; 4],
) {
    let interior_left = u32_from_i32_size(rect.left + border_px);
    let interior_top = u32_from_i32_size(rect.top + border_px);
    assert_color_eq_at(
        rgba,
        width_px,
        interior_left,
        interior_top,
        expected,
        &format!("{label} fill"),
    );
}

pub fn assert_color_eq_at(
    rgba: &[u8],
    width_px: u32,
    x_px: u32,
    y_px: u32,
    expected: [f32; 4],
    context: &str,
) {
    let actual = pixel_color_as_unit(rgba, width_px, x_px, y_px);
    for channel_index in 0..4 {
        assert!(
            (actual[channel_index] - expected[channel_index]).abs() <= (1.0 / 255.0),
            "{context} channel {channel_index} at ({x_px}, {y_px}) expected {expected:?}, got {actual:?}",
            expected[channel_index],
        );
    }
}

pub fn assert_one_of_colors_at(
    rgba: &[u8],
    width_px: u32,
    x_px: u32,
    y_px: u32,
    expected: &[[f32; 4]],
    context: &str,
) {
    let actual = pixel_color_as_unit(rgba, width_px, x_px, y_px);
    assert!(
        expected
            .iter()
            .any(|color| color_matches_expected(actual, *color)),
        "{context} at ({x_px}, {y_px}) expected one of {expected:?}, got {actual:?}",
    );
}

fn color_matches_expected(actual: [f32; 4], expected: [f32; 4]) -> bool {
    (0..4).all(|channel_index| {
        (actual[channel_index] - expected[channel_index]).abs() <= (1.0 / 255.0)
    })
}

fn pixel_color_as_unit(
    rgba: &[u8],
    width_px: u32,
    x_px: u32,
    y_px: u32,
) -> [f32; 4] {
    let start = pixel_start(width_px, x_px, y_px);
    [
        f32::from(rgba[start]) / 255.0,
        f32::from(rgba[start + 1]) / 255.0,
        f32::from(rgba[start + 2]) / 255.0,
        f32::from(rgba[start + 3]) / 255.0,
    ]
}

pub fn pixel_start(width_px: u32, x_px: u32, y_px: u32) -> usize {
    usize::try_from((u64::from(y_px) * u64::from(width_px) + u64::from(x_px)) * 4)
        .expect("pixel byte index should fit usize")
}

pub fn u32_from_i32_size(value: i32) -> u32 {
    u32::try_from(value).expect("pixel coordinate should fit u32")
}

pub fn usize_from_u32_size(value: u32) -> usize {
    usize::try_from(value).expect("pixel size should fit usize")
}
