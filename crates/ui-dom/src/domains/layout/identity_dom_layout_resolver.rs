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

/// Falls back to the current node-owned box, or the matched image size when geometry is absent.
#[derive(Clone, Copy, Debug, Default)]
pub struct IdentityDomLayoutResolver;

impl DomLayoutResolver for IdentityDomLayoutResolver {
    fn measure_box(&mut self, input: DomLayoutInput<'_>) -> Result<DomBoxModel, DomError> {
        style::intrinsic_box_for_node(input.node, input.image_size_px)
    }
}
