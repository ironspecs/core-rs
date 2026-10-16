//! Identity DOM layout resolver for `ui-dom`.
//!
//! This file owns the `IdentityDomLayoutResolver` resolver that falls back to
//! the current node-owned box, or the matched image size when geometry is
//! absent.
//!
//! Invariants and rules:
//!
//! - this file owns only the `IdentityDomLayoutResolver` struct and its
//!   `DomLayoutResolver` impl block
//! - measurement delegates to `style::intrinsic_box_for_node` and holds no
//!   state of its own

use super::style;
use super::{DomLayoutInput, DomLayoutResolver};
use crate::types::{DomBoxModel, DomError};

#[cfg(test)]
use crate::types::{DomIntrinsic, DomNode, DomNodeSlotKey, DomTag, ImgIntrinsic};

/// Falls back to the current node-owned box, or the matched image size when geometry is absent.
#[derive(Clone, Copy, Debug, Default)]
pub struct IdentityDomLayoutResolver;

impl DomLayoutResolver for IdentityDomLayoutResolver {
    fn measure_box(&mut self, input: DomLayoutInput<'_>) -> Result<DomBoxModel, DomError> {
        style::intrinsic_box_for_node(input.node, input.image_size_px)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= 0.001,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn identity_resolver_delegates_to_intrinsic_box_for_node() {
        let mut resolver = IdentityDomLayoutResolver;

        // A freshly created plain element has a zero intrinsic box; the
        // resolver must return exactly that box.
        let element = DomNode::element(DomNodeSlotKey::default(), DomTag::Div);
        let element_input = DomLayoutInput {
            node_key: DomNodeSlotKey::default(),
            node: &element,
            parent_box: None,
            image_size_px: (320.0, 240.0),
        };
        let element_measured = resolver
            .measure_box(element_input)
            .expect("plain element should measure");
        let element_intrinsic = style::intrinsic_box_for_node(&element, (320.0, 240.0))
            .expect("plain element intrinsic");
        assert_eq!(element_measured, element_intrinsic);

        // An intrinsic img node measures to its matched image size; the
        // resolver must return exactly that intrinsic box.
        let mut img = DomNode::element(DomNodeSlotKey::default(), DomTag::Img);
        img.set_intrinsic_for_store(DomIntrinsic::Img(ImgIntrinsic {
            width_px: 48.0,
            height_px: 32.0,
            aspect_ratio: 1.5,
            generation: 0,
        }));
        let img_input = DomLayoutInput {
            node_key: DomNodeSlotKey::default(),
            node: &img,
            parent_box: None,
            image_size_px: (48.0, 32.0),
        };
        let img_measured = resolver.measure_box(img_input).expect("img should measure");
        let img_intrinsic =
            style::intrinsic_box_for_node(&img, (48.0, 32.0)).expect("img intrinsic");
        assert_eq!(img_measured, img_intrinsic);
        assert_close(img_measured.border_box.width_px, 48.0);
        assert_close(img_measured.border_box.height_px, 32.0);
    }
}
