//! Shared after-attribute-write intrinsic update policy for `ui-dom`.
//!
//! This file owns the enum that declares which attribute writes should
//! invalidate a built-in element's intrinsic size. Its purpose is to keep the
//! authored attribute list explicit instead of scattering the invalidation
//! list across layout or tag-default code.
//!
//! Invariants and rules:
//!
//! - this file owns only the `DomAfterAttrWriteIntrinsicUpdate` enum and its
//!   `impl` blocks
//! - the attribute list is fixed per built-in tag and stays crate-owned

use smallvec::SmallVec;

use crate::types::dom_attribute::DomAttribute;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum DomAfterAttrWriteIntrinsicUpdate {
    #[default]
    None,
    AttributeList(SmallVec<[DomAttribute; 4]>),
}

impl DomAfterAttrWriteIntrinsicUpdate {
    #[must_use]
    pub fn matches(&self, attribute: &DomAttribute) -> bool {
        match self {
            Self::None => false,
            Self::AttributeList(attributes) => {
                attributes.iter().any(|candidate| candidate == attribute)
            }
        }
    }
}
