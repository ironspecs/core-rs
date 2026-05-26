use ui_icons::UiIconId;

/// Flat retained UI icon geometry and stable icon identity.
#[derive(Clone, Debug, PartialEq)]
pub struct IconPaintOp {
    /// Layer-local top-left position in logical pixels.
    pub position_px: [f32; 2],
    /// Layer-local width and height in logical pixels.
    pub size_px: [f32; 2],
    /// Stable curated icon identity.
    pub icon_id: UiIconId,
    /// Icon tint color resolved from the effective retained `color`.
    pub color_rgba: [f32; 4],
}
