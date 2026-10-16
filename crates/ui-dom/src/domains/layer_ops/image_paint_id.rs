//! Stable retained image placeholder identity for `ui-dom`.
//!
//! This file owns the identity key that banks retained image placeholder ops
//! by authored image name. Its purpose is to keep image placeholder identity
//! stable and hashable without owning texture upload or layer-pool policy.
//!
//! Invariants and rules:
//!
//! - this file owns only the `ImagePaintId` struct and its `impl` block
//! - `from_name` derives the identity from the authored image name, so the
//!   same name always yields the same retained identity

use super::stable_placeholder_id;

#[cfg(test)]
use crate::domains::layer_ops::SvgPaintId;

/// Stable retained image placeholder identity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ImagePaintId(u64);

impl ImagePaintId {
    /// Derives one stable retained image placeholder identity from the authored image name.
    #[must_use]
    pub fn from_name(image_name: &str) -> Self {
        Self(stable_placeholder_id(image_name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_ids_are_stable_for_matching_names() {
        assert_eq!(SvgPaintId::from_name("menu"), SvgPaintId::from_name("menu"));
        assert_eq!(
            ImagePaintId::from_name("avatar.png"),
            ImagePaintId::from_name("avatar.png"),
        );
    }
}
