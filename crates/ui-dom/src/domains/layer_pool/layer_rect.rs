//! Retained-layer logical rect for the layer pool.
//!
//! This file owns the `LayerRect` value type that carries one authored layer's
//! logical origin and size, plus its tuple constructor. Its purpose is to keep
//! the rect value and its constructor beside each other instead of growing the
//! shared `layer_pool` owner module.
//!
//! Invariants and rules:
//!
//! - this file owns only the `LayerRect` struct and its `impl` block
//! - retained mapping computation stays in the shared `layer_pool` owner
//!   module free functions

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct LayerRect {
    pub(super) origin_x: f32,
    pub(super) origin_y: f32,
    pub(super) width: f32,
    pub(super) height: f32,
}

impl LayerRect {
    pub(super) fn from_tuple(layer_rect_px: (f32, f32, f32, f32)) -> Self {
        Self {
            origin_x: layer_rect_px.0,
            origin_y: layer_rect_px.1,
            width: layer_rect_px.2,
            height: layer_rect_px.3,
        }
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn from_tuple_maps_fields_in_order() {
        let rect = LayerRect::from_tuple((1.0, 2.0, 30.0, 40.0));
        assert_eq!(rect.origin_x, 1.0);
        assert_eq!(rect.origin_y, 2.0);
        assert_eq!(rect.width, 30.0);
        assert_eq!(rect.height, 40.0);
    }
}
