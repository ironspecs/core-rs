// DIRECT_RECT_GPU_BOUNDARY: retained non-icon rect paint contract; keep this
// payload flat and shader-oriented.

/// Flat retained rect paint payload lowered directly into the GPU rect path.
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct RectPaintOp {
    /// Layer-local top-left position in pixels.
    pub position_px: [f32; 2],
    /// Layer-local width and height in pixels.
    pub size_px: [f32; 2],
    /// Fill color in normalized RGBA.
    pub fill_rgba: [f32; 4],
    /// Corner radii in CSS order: top-left, top-right, bottom-right, bottom-left.
    pub corner_radii_px: [f32; 4],
    /// Border widths in CSS side order: top, right, bottom, left.
    pub border_widths_px: [f32; 4],
    /// Top border color in normalized RGBA.
    pub border_top_rgba: [f32; 4],
    /// Right border color in normalized RGBA.
    pub border_right_rgba: [f32; 4],
    /// Bottom border color in normalized RGBA.
    pub border_bottom_rgba: [f32; 4],
    /// Left border color in normalized RGBA.
    pub border_left_rgba: [f32; 4],
}
